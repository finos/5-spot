// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # `5spot-kata-config-agent`
//!
//! Node-side DaemonSet binary that delivers a Kata containerd drop-in to the
//! host filesystem (ADR 0002 / ADR 0003, roadmap Phase 3). On each tick it reads
//! its own Node's `5spot.finos.org/kata-config-ref` annotation (stamped by the
//! controller), resolves the named `ConfigMap`/`Secret` from the workload-cluster
//! API, and reconciles the host destination file to match — writing atomically
//! on change and self-healing drift.
//!
//! The agent reads via the kube API rather than a mounted ConfigMap volume
//! because a cluster-wide DaemonSet cannot template a `configMap.name` volume per
//! replica (ADR 0002 §3). The host k0s-service restart (`nsenter`) and the
//! applied-hash node annotation are Phase 4 and not yet wired here — this binary
//! lands and self-heals the file; containerd reloads it once Phase 4 triggers the
//! restart. The drift-watch is a deliberate node-local poll: out-of-band edits to
//! the host file generate no Kubernetes event, so there is nothing to watch on
//! the API for that signal.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::Parser;
use k8s_openapi::api::core::v1::{ConfigMap, Node, Secret};
use kube::api::{Patch, PatchParams};
use kube::{Api, Client};
use serde_json::json;
use tracing::{debug, info, warn};

use five_spot::constants::{
    KATA_CONFIG_APPLIED_ANNOTATION, KATA_CONFIG_LABEL, KATA_CONFIG_REF_ANNOTATION,
};
use five_spot::kata_config_agent::{
    parse_kata_ref, sync_content, KataRef, SyncOutcome, DEFAULT_POLL_INTERVAL_SECS,
};

/// CLI / environment configuration for the kata-config agent.
#[derive(Debug, Parser)]
#[command(
    name = "5spot-kata-config-agent",
    about = "Delivers a Kata containerd drop-in to the host filesystem"
)]
struct Cli {
    /// Name of the Node this pod runs on (downward API `spec.nodeName`). The
    /// agent reads this Node's `5spot.finos.org/kata-config-ref` annotation.
    #[arg(long, env = "NODE_NAME")]
    node_name: String,

    /// Host filesystem mount root inside the pod. The drop-in's absolute
    /// `destPath` is written under here (e.g. `/host` + `/etc/k0s/...`).
    #[arg(long, env = "HOST_ROOT", default_value = "/host")]
    host_root: PathBuf,

    /// Seconds between drift-watch sweeps.
    #[arg(long, env = "POLL_INTERVAL_SECS", default_value_t = DEFAULT_POLL_INTERVAL_SECS)]
    poll_interval_secs: u64,

    /// Run a single reconcile and exit (smoke-test / one-shot use).
    #[arg(long, env = "ONESHOT", default_value_t = false)]
    oneshot: bool,
}

/// Resolve the absolute host `destPath` to the in-pod path under `host_root`
/// (e.g. `host_root=/host`, `dest=/etc/k0s/x.toml` → `/host/etc/k0s/x.toml`).
fn host_path(host_root: &std::path::Path, dest_path: &str) -> PathBuf {
    host_root.join(dest_path.trim_start_matches('/'))
}

/// The two kata annotations the agent reads off its own Node: the controller's
/// reference (present ⇒ deliver) and the agent's own applied-dest record (the
/// host path it last wrote, so it can clean up after tear-down).
struct NodeState {
    kata_ref: Option<KataRef>,
    applied_dest: Option<String>,
}

/// Read this Node's kata-config-ref and kata-config-applied annotations.
async fn read_node_state(nodes: &Api<Node>, node_name: &str) -> Result<NodeState> {
    let node = nodes
        .get(node_name)
        .await
        .with_context(|| format!("GET Node {node_name}"))?;
    let anns = node.metadata.annotations.unwrap_or_default();
    let kata_ref = match anns.get(KATA_CONFIG_REF_ANNOTATION) {
        Some(raw) => Some(parse_kata_ref(raw).with_context(|| {
            format!("parsing {KATA_CONFIG_REF_ANNOTATION} on Node {node_name}")
        })?),
        None => None,
    };
    Ok(NodeState {
        kata_ref,
        applied_dest: anns.get(KATA_CONFIG_APPLIED_ANNOTATION).cloned(),
    })
}

/// Record the absolute host `destPath` this agent is responsible for, in the
/// kata-config-applied annotation on its own Node — the durable record cleanup
/// reads after the controller clears the reference annotation.
async fn record_applied(nodes: &Api<Node>, node_name: &str, dest_path: &str) -> Result<()> {
    let patch =
        json!({ "metadata": { "annotations": { KATA_CONFIG_APPLIED_ANNOTATION: dest_path } } });
    nodes
        .patch(node_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .with_context(|| format!("recording applied-dest on Node {node_name}"))?;
    Ok(())
}

/// Clear the applied annotation and remove the opt-in label from this Node so
/// the DaemonSet deschedules — the tear-down handshake completion (ADR 0002).
async fn clear_optin(nodes: &Api<Node>, node_name: &str) -> Result<()> {
    let patch = json!({
        "metadata": {
            "annotations": { KATA_CONFIG_APPLIED_ANNOTATION: null },
            "labels": { KATA_CONFIG_LABEL: null }
        }
    });
    nodes
        .patch(node_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .with_context(|| format!("clearing kata opt-in on Node {node_name}"))?;
    Ok(())
}

/// Fetch the drop-in content referenced by `kref` from the workload API. Returns
/// `None` when the object exists but lacks the key, or the object is absent
/// (404) — both map to a GitOps tear-down of the host file.
async fn fetch_content(client: &Client, kref: &KataRef) -> Result<Option<String>> {
    match kref.kind.as_str() {
        "ConfigMap" => {
            let api: Api<ConfigMap> = Api::namespaced(client.clone(), &kref.namespace);
            let Some(cm) = api.get_opt(&kref.name).await? else {
                return Ok(None);
            };
            Ok(cm.data.and_then(|d| d.get(&kref.key).cloned()))
        }
        "Secret" => {
            let api: Api<Secret> = Api::namespaced(client.clone(), &kref.namespace);
            let Some(secret) = api.get_opt(&kref.name).await? else {
                return Ok(None);
            };
            Ok(secret.data.and_then(|d| {
                d.get(&kref.key)
                    .map(|b| String::from_utf8_lossy(&b.0).into_owned())
            }))
        }
        other => anyhow::bail!("unknown kata source kind {other:?} (expected ConfigMap or Secret)"),
    }
}

/// One reconcile tick.
///
/// - **reference present** → fetch the source, sync the host file, and record
///   the applied dest on the Node (so cleanup can find it later).
/// - **reference absent** → tear-down: unlink the host file the agent recorded
///   (if any), then clear the applied annotation and remove the opt-in label so
///   the DaemonSet deschedules. Doing the unlink here — while still scheduled —
///   is what closes the descheduled-before-cleanup gap (ADR 0002).
async fn reconcile_once(client: &Client, cli: &Cli) -> Result<()> {
    let nodes: Api<Node> = Api::all(client.clone());
    let state = read_node_state(&nodes, &cli.node_name).await?;

    let Some(kref) = state.kata_ref else {
        // Tear-down. If we previously applied a file, unlink it now.
        if let Some(dest_path) = state.applied_dest.as_deref() {
            let dest = host_path(&cli.host_root, dest_path);
            if sync_content(None, &dest)? == SyncOutcome::Deleted {
                info!(node = %cli.node_name, dest = %dest.display(), "removed kata drop-in from host (tear-down)");
                // Phase 4: bounce the host k0s service here (nsenter) so
                // containerd drops the removed runtime.
            }
        }
        clear_optin(&nodes, &cli.node_name).await?;
        debug!(node = %cli.node_name, "kata tear-down complete; opt-in label removed (descheduling)");
        return Ok(());
    };

    // Deliver.
    let content = fetch_content(client, &kref).await?;
    let dest = host_path(&cli.host_root, &kref.dest_path);
    let bytes = content.as_ref().map(String::as_bytes);

    match sync_content(bytes, &dest)? {
        SyncOutcome::Wrote(hash) => info!(
            node = %cli.node_name,
            dest = %dest.display(),
            sha256 = %hash,
            "wrote kata drop-in to host (containerd reload pending Phase 4 restart)"
        ),
        SyncOutcome::Deleted => info!(
            node = %cli.node_name,
            dest = %dest.display(),
            "removed kata drop-in from host (source object/key cleared)"
        ),
        SyncOutcome::Unchanged(_) => {
            debug!(node = %cli.node_name, dest = %dest.display(), "kata drop-in in sync")
        }
    }

    // Record the dest we manage so tear-down can find it after the controller
    // clears the reference annotation.
    record_applied(&nodes, &cli.node_name, &kref.dest_path).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    info!(
        node = %cli.node_name,
        host_root = %cli.host_root.display(),
        poll_interval_secs = cli.poll_interval_secs,
        oneshot = cli.oneshot,
        "kata-config-agent started",
    );

    let client = Client::try_default()
        .await
        .context("building in-cluster kube client")?;

    let interval = Duration::from_secs(cli.poll_interval_secs.max(1));
    loop {
        if let Err(e) = reconcile_once(&client, &cli).await {
            warn!(
                node = %cli.node_name,
                error = %e,
                "kata drop-in reconcile failed; will retry next tick"
            );
        }
        if cli.oneshot {
            break;
        }
        tokio::time::sleep(interval).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::host_path;
    use std::path::Path;

    #[test]
    fn test_host_path_joins_dest_under_root() {
        assert_eq!(
            host_path(
                Path::new("/host"),
                "/etc/k0s/container.d/kata-containers.toml"
            ),
            Path::new("/host/etc/k0s/container.d/kata-containers.toml")
        );
    }

    #[test]
    fn test_host_path_strips_leading_slashes() {
        assert_eq!(
            host_path(Path::new("/host"), "///a/b"),
            Path::new("/host/a/b")
        );
    }
}
