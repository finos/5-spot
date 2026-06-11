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
//! replica (ADR 0002 §3). After a write it restarts the host k0s service via
//! `nsenter` into host PID 1 (ADR 0003) so containerd reloads the drop-in,
//! recording the applied content hash on its own Node **before** the restart so
//! the SIGKILL the restart delivers does not re-trigger on the next loop. The
//! drift-watch is a deliberate node-local poll: out-of-band edits to the host
//! file generate no Kubernetes event, so there is nothing to watch on the API for
//! that signal.

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
    KATA_CONFIG_APPLIED_ANNOTATION, KATA_CONFIG_DEST_PATH, KATA_CONFIG_LABEL,
    KATA_CONFIG_REF_ANNOTATION,
};
use five_spot::kata_config_agent::{
    confine_dest_path, intended_hash_for, is_drift_correction, needs_restart, nsenter_restart_argv,
    parse_kata_ref, restart_if_needed, sync_content, KataRef, RestartExecutor, SyncOutcome,
    DEFAULT_POLL_INTERVAL_SECS,
};
use five_spot::metrics;

/// Default port for the agent's Prometheus `/metrics` endpoint — matches the
/// controller's `METRICS_PORT` convention.
const DEFAULT_METRICS_PORT: u16 = 8080;

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

    /// Port for the Prometheus `/metrics` endpoint.
    #[arg(long, env = "METRICS_PORT", default_value_t = DEFAULT_METRICS_PORT)]
    metrics_port: u16,

    /// Run a single reconcile and exit (smoke-test / one-shot use).
    #[arg(long, env = "ONESHOT", default_value_t = false)]
    oneshot: bool,
}

/// The two kata annotations the agent reads off its own Node: the controller's
/// reference (present ⇒ deliver) and the agent's own applied content hash (the
/// bare SHA-256 it last restarted for, or the `absent` marker — the restart-loop
/// guard). Neither carries a host path (ADR 0005).
struct NodeState {
    kata_ref: Option<KataRef>,
    applied: Option<String>,
}

/// Read this Node's kata-config-ref and kata-config-applied annotations. The
/// applied value is an opaque hash string — a legacy/garbage value simply never
/// matches a real content hash, so the agent re-applies + restarts (the safe
/// direction).
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
    let applied = anns.get(KATA_CONFIG_APPLIED_ANNOTATION).cloned();
    Ok(NodeState { kata_ref, applied })
}

/// Record the applied content hash (bare value) in the kata-config-applied
/// annotation on this agent's own Node. Written **before** a restart so a
/// SIGKILL mid-restart cannot re-trigger next tick.
async fn record_applied(nodes: &Api<Node>, node_name: &str, hash: &str) -> Result<()> {
    let patch = json!({ "metadata": { "annotations": { KATA_CONFIG_APPLIED_ANNOTATION: hash } } });
    nodes
        .patch(node_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .with_context(|| format!("recording applied hash on Node {node_name}"))?;
    Ok(())
}

/// Concrete [`RestartExecutor`] that restarts the host k0s service by entering
/// host PID 1's namespaces via `nsenter` and running `systemctl restart` (ADR
/// 0003). Requires the pod to run `privileged: true` + `hostPID: true`.
struct NsenterRestartExecutor;

impl RestartExecutor for NsenterRestartExecutor {
    fn restart(&self, service: &str) -> std::io::Result<()> {
        let argv = nsenter_restart_argv(service);
        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .status()?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "`nsenter … systemctl restart {service}` exited with {status}"
            )));
        }
        Ok(())
    }
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
/// - **reference present** → fetch the source, sync the host file, and — when
///   the content now on the host differs from what was last applied — record the
///   new applied hash on the Node and restart the host k0s service so containerd
///   reloads the drop-in (ADR 0003). The record is written **before** the
///   restart so the SIGKILL it delivers cannot re-trigger next tick.
/// - **reference absent** → tear-down: unlink the host file the agent recorded
///   (if any), then clear the applied annotation and remove the opt-in label so
///   the DaemonSet deschedules. Doing the unlink here — while still scheduled —
///   is what closes the descheduled-before-cleanup gap (ADR 0002).
async fn reconcile_once(client: &Client, cli: &Cli, executor: &dyn RestartExecutor) -> Result<()> {
    let nodes: Api<Node> = Api::all(client.clone());
    let state = read_node_state(&nodes, &cli.node_name).await?;

    let Some(kref) = state.kata_ref else {
        // Tear-down: unlink the fixed drop-in path (idempotent — no-op when
        // already absent). The location is the compile-time constant, resolved
        // through the same ADR 0005 containment as the write path, so no
        // annotation content can steer this root unlink.
        let dest = confine_dest_path(&cli.host_root, KATA_CONFIG_DEST_PATH)?;
        if sync_content(None, &dest)? == SyncOutcome::Deleted {
            metrics::record_kata_config_delete();
            info!(node = %cli.node_name, dest = %dest.display(), "removed kata drop-in from host (tear-down)");
        }
        clear_optin(&nodes, &cli.node_name).await?;
        debug!(node = %cli.node_name, "kata tear-down complete; opt-in label removed (descheduling)");
        return Ok(());
    };

    // Deliver — always to the fixed drop-in path (ADR 0005), resolved through
    // the containment check as defense-in-depth against host-side symlinks.
    let dest = confine_dest_path(&cli.host_root, KATA_CONFIG_DEST_PATH)?;
    let content = fetch_content(client, &kref).await?;
    let bytes = content.as_ref().map(String::as_bytes);
    let outcome = sync_content(bytes, &dest)?;

    let prev_hash = state.applied.as_deref();
    match &outcome {
        SyncOutcome::Wrote(hash) => {
            metrics::record_kata_config_write(is_drift_correction(&outcome, prev_hash));
            info!(
                node = %cli.node_name,
                dest = %dest.display(),
                sha256 = %hash,
                "wrote kata drop-in to host"
            );
        }
        SyncOutcome::Deleted => {
            metrics::record_kata_config_delete();
            info!(
                node = %cli.node_name,
                dest = %dest.display(),
                "removed kata drop-in from host (source object/key cleared)"
            );
        }
        SyncOutcome::Unchanged(_) => {
            metrics::record_kata_config_sync_unchanged();
            debug!(node = %cli.node_name, dest = %dest.display(), "kata drop-in in sync");
        }
    }

    // Restart-loop guard: restart the host service only when the content now on
    // the host differs from what we last applied. Record the new hash BEFORE the
    // restart so the SIGKILL it delivers does not re-trigger on the next loop.
    let intended_hash = intended_hash_for(&outcome);
    if needs_restart(prev_hash, &intended_hash) {
        record_applied(&nodes, &cli.node_name, &intended_hash).await?;
        info!(
            node = %cli.node_name,
            service = %kref.restart_service,
            sha256 = %intended_hash,
            "restarting host k0s service so containerd reloads the kata drop-in"
        );
        if restart_if_needed(executor, prev_hash, &intended_hash, &kref.restart_service)
            .with_context(|| format!("restarting host service {}", kref.restart_service))?
        {
            metrics::record_kata_config_restart();
        }
    }
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
        metrics_port = cli.metrics_port,
        oneshot = cli.oneshot,
        "kata-config-agent started",
    );

    let client = Client::try_default()
        .await
        .context("building in-cluster kube client")?;

    // Oneshot is a smoke-test mode; a lingering server would block exit.
    if !cli.oneshot {
        tokio::spawn(metrics::serve_metrics(cli.metrics_port));
    }

    let executor = NsenterRestartExecutor;
    let interval = Duration::from_secs(cli.poll_interval_secs.max(1));
    loop {
        if let Err(e) = reconcile_once(&client, &cli, &executor).await {
            metrics::record_kata_config_sync_error();
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
