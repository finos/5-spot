// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! # 5spot-reclaim-agent — node-side emergency reclaim trigger
//!
//! Small static binary that watches `/proc` on its own host and, on
//! first `process-match` against a user-supplied list, patches the local
//! `Node` object with reclaim annotations so the 5-Spot controller can
//! enter `Phase::EmergencyRemove`.
//!
//! See `docs/roadmaps/5spot-emergency-reclaim-by-process-match.md` for
//! the full design, including the two-rung detection ladder
//! (rung 1 = `/proc` poll, implemented here; rung 2 = netlink proc
//! connector, future work).
//!
//! ## Config source — reactive ConfigMap watch
//!
//! The agent no longer mounts its configuration from a file. Instead it
//! watches the per-node `ConfigMap` named `reclaim-agent-<NODE_NAME>` in
//! [`RECLAIM_AGENT_NAMESPACE`] and reacts to every change:
//!
//! * ConfigMap absent → agent idles (no `/proc` scanning).
//! * ConfigMap applied / updated → `configmap_to_config` parses the
//!   `reclaim.toml` key and the scanner rearms with the new commands on
//!   the next tick.
//! * ConfigMap deleted → agent returns to idle.
//!
//! The controller projects this ConfigMap whenever
//! `ScheduledMachine.spec.killIfCommands` is non-empty; an operator can
//! also hand-create it for manual arming.
//!
//! ## Exit semantics
//!
//! Exits 0 on first successful annotation write, or on a no-op idempotent
//! check (annotation already present). Exits non-zero on unrecoverable
//! errors; kubelet will restart the pod, which re-runs and idempotently
//! exits 0 again if the annotation has been committed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use clap::Parser;
use five_spot::reclaim_agent::{
    already_requested, build_patch_body, configmap_to_config, scan_proc, Config, Match,
};
use futures::StreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Node};
use kube::{
    api::{Patch, PatchParams},
    runtime::watcher,
    Api, Client,
};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

/// Default path the agent reads as `/proc`. Overridable for testing.
const DEFAULT_PROC_ROOT: &str = "/proc";

/// Field manager name used on PATCH. Distinct from the main controller
/// so audit logs can tell apart a controller-side write from an
/// agent-side write.
const FIELD_MANAGER: &str = "5spot-reclaim-agent";

/// How long the scanner sleeps between `/proc` passes when the shared
/// config is `None` (no per-node ConfigMap observed yet). The watcher
/// pushes a wake-up the moment a ConfigMap lands, so this is just a
/// safety net for torn-down watch streams. Kept conservative so an idle
/// agent exerts essentially zero CPU.
const IDLE_WAKEUP_SECS: u64 = 30;

#[derive(Parser, Debug)]
#[clap(
    name = "5spot-reclaim-agent",
    about = "Node-side emergency reclaim trigger for 5-Spot",
    version
)]
struct Cli {
    /// Filesystem root mapped to `/proc` (override for testing / sandboxes).
    #[clap(long, env = "RECLAIM_PROC_ROOT", default_value = DEFAULT_PROC_ROOT)]
    proc_root: PathBuf,

    /// Name of the Node to annotate. Required — supply via the downward
    /// API (`spec.nodeName`) on the `DaemonSet` pod.
    #[clap(long, env = "NODE_NAME")]
    node_name: String,

    /// If set, run the detector once and exit instead of looping. Useful
    /// for one-shot invocations and for smoke tests.
    #[clap(long)]
    oneshot: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let client = Client::try_default()
        .await
        .context("build in-cluster kube client")?;
    let nodes: Api<Node> = Api::all(client.clone());

    if is_already_requested(&nodes, &cli.node_name).await? {
        info!(node = %cli.node_name, "reclaim annotation already present — exiting idempotently");
        return Ok(());
    }

    // Shared config state — `None` means idle, `Some` means arm the scanner.
    // `watch` is ideal here: the scanner reads the current value each tick,
    // the watcher overwrites on every CM event.
    let (tx, rx) = watch::channel::<Option<Config>>(None);

    let cm_name = Arc::new(format!(
        "{prefix}{node}",
        prefix = five_spot::constants::RECLAIM_AGENT_CONFIGMAP_PREFIX,
        node = cli.node_name
    ));
    info!(
        node = %cli.node_name,
        configmap = %cm_name,
        namespace = five_spot::constants::RECLAIM_AGENT_NAMESPACE,
        "reclaim-agent started — watching ConfigMap for arming",
    );

    // Spawn the watcher. It runs until the process exits or the channel
    // receiver drops; a transient apiserver error triggers an internal
    // resubscribe inside `kube::runtime::watcher`.
    let watcher_handle = tokio::spawn(run_config_watcher(client, cm_name, tx));

    let scanner_result = run_scanner(&nodes, &cli.node_name, &cli.proc_root, rx, cli.oneshot).await;
    watcher_handle.abort();
    scanner_result
}

/// Fetch the target `Node` and test whether a prior reclaim request is
/// already present. Used for idempotence on agent restart.
async fn is_already_requested(nodes: &Api<Node>, node_name: &str) -> Result<bool> {
    let node = nodes
        .get(node_name)
        .await
        .with_context(|| format!("fetch Node/{node_name}"))?;
    let ann = node.metadata.annotations.unwrap_or_default();
    let as_btree: std::collections::BTreeMap<String, String> = ann.into_iter().collect();
    Ok(already_requested(&as_btree))
}

/// Subscribe to the per-node `ConfigMap` and push every observed version
/// (or `None` on delete) into the scanner's watch channel.
///
/// Field-selector narrows the server-side watch to just the one CM we
/// care about, so we never receive updates for unrelated ConfigMaps in
/// the namespace. A malformed payload is logged and ignored — the
/// scanner continues to run against whatever last-good config it has.
async fn run_config_watcher(
    client: Client,
    cm_name: Arc<String>,
    tx: watch::Sender<Option<Config>>,
) {
    let cms: Api<ConfigMap> =
        Api::namespaced(client, five_spot::constants::RECLAIM_AGENT_NAMESPACE);
    let wc = watcher::Config::default().fields(&format!("metadata.name={cm_name}"));
    let mut stream = watcher(cms, wc).boxed();
    loop {
        match stream.next().await {
            Some(Ok(event)) => apply_event(event, &tx, &cm_name),
            Some(Err(e)) => {
                // The watcher crate internally resubscribes; this branch
                // is observational so operators see the blip in logs.
                warn!(error = %e, "configmap watch error — the watcher will resubscribe");
            }
            None => {
                info!("configmap watch stream ended");
                return;
            }
        }
    }
}

fn apply_event(
    event: watcher::Event<ConfigMap>,
    tx: &watch::Sender<Option<Config>>,
    cm_name: &str,
) {
    use watcher::Event;
    match event {
        Event::Apply(cm) | Event::InitApply(cm) => push_parsed(&cm, tx, cm_name),
        Event::Delete(_) => {
            info!(configmap = cm_name, "configmap deleted — idling scanner");
            let _ = tx.send(None);
        }
        Event::Init | Event::InitDone => {
            debug!("configmap watcher init boundary");
        }
    }
}

fn push_parsed(cm: &ConfigMap, tx: &watch::Sender<Option<Config>>, cm_name: &str) {
    match configmap_to_config(cm) {
        Ok(Some(cfg)) => {
            info!(
                configmap = cm_name,
                commands = ?cfg.match_commands,
                substrings = ?cfg.match_argv_substrings,
                poll_ms = cfg.poll_interval_ms,
                "configmap applied — rearming scanner",
            );
            let _ = tx.send(Some(cfg));
        }
        Ok(None) => {
            info!(
                configmap = cm_name,
                "configmap applied but data.reclaim.toml missing — idling scanner"
            );
            let _ = tx.send(None);
        }
        Err(e) => {
            // Hold the previous known-good config. A bad edit must not
            // disarm a correctly-armed agent.
            error!(
                configmap = cm_name,
                error = %e,
                "malformed reclaim.toml in configmap — keeping previous config"
            );
        }
    }
}

/// Core detection loop. Reads the shared config, and either scans
/// `/proc` once per `poll_interval_ms` (config = `Some`) or blocks until
/// the config transitions (config = `None`).
async fn run_scanner(
    nodes: &Api<Node>,
    node_name: &str,
    proc_root: &Path,
    mut rx: watch::Receiver<Option<Config>>,
    oneshot: bool,
) -> Result<()> {
    loop {
        let cfg = rx.borrow().clone();
        match cfg {
            None => {
                if oneshot {
                    warn!("oneshot mode: no config present — exiting non-zero");
                    return Err(anyhow!("no configmap observed during oneshot run"));
                }
                // Wait for either a config change or the idle wakeup. The
                // `tokio::select!` covers the case where the watcher dies
                // and we'd otherwise block forever.
                tokio::select! {
                    res = rx.changed() => {
                        if res.is_err() {
                            info!("config channel closed — exiting");
                            return Ok(());
                        }
                    }
                    () = tokio::time::sleep(Duration::from_secs(IDLE_WAKEUP_SECS)) => {}
                }
            }
            Some(cfg) => match scan_proc(proc_root, &cfg) {
                Ok(Some(m)) => {
                    info!(pid = m.pid, pattern = %m.matched_pattern, "match → annotating node");
                    annotate_node(nodes, node_name, &m).await?;
                    return Ok(());
                }
                Ok(None) => {
                    if oneshot {
                        warn!("oneshot mode: no match found, exiting non-zero");
                        return Err(anyhow!("no match on single scan"));
                    }
                    tokio::time::sleep(Duration::from_millis(cfg.poll_interval_ms)).await;
                }
                Err(e) => {
                    error!(error = %e, "scan_proc failed");
                    return Err(e.into());
                }
            },
        }
    }
}

async fn annotate_node(nodes: &Api<Node>, node_name: &str, m: &Match) -> Result<()> {
    let ts = chrono::Utc::now().to_rfc3339();
    let patch = build_patch_body(m, &ts);
    let params = PatchParams::apply(FIELD_MANAGER).force();
    nodes
        .patch(node_name, &params, &Patch::Merge(&patch))
        .await
        .with_context(|| format!("patch Node/{node_name}"))?;
    info!(node = %node_name, "reclaim annotation written");
    Ok(())
}
