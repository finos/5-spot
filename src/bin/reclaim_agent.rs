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

use anyhow::{anyhow, bail, Context as _, Result};
use clap::{Parser, ValueEnum};
use five_spot::netlink_proc::{NetlinkError, ProcEvent, Subscriber as NetlinkSubscriber};
use five_spot::reclaim_agent::{
    already_requested, build_patch_body, compare_machine_ids, configmap_to_config, match_pid,
    read_host_machine_id, scan_proc, Config, Match,
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

/// Default path the agent reads for the host machine-id. Mounted into
/// the DaemonSet via a single-file hostPath; the file is set at host
/// boot by `systemd-machine-id-setup` (or kairos / k0s-installer).
const DEFAULT_MACHINE_ID_PATH: &str = "/etc/machine-id";

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

/// CLI value for `--detector`. Resolved to a [`Detector`] via
/// [`DetectorFlag::resolve`] so `auto` is platform-aware.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum DetectorFlag {
    /// Pick `netlink` on Linux, `poll` elsewhere.
    Auto,
    /// Subscribe to the netlink proc connector (Linux only).
    Netlink,
    /// Walk `/proc` every `poll_interval_ms` (rung-1 fallback).
    Poll,
}

impl DetectorFlag {
    /// Resolve `auto` to a concrete detector based on the build target.
    fn resolve(self) -> Detector {
        match self {
            DetectorFlag::Netlink => Detector::Netlink,
            DetectorFlag::Poll => Detector::Poll,
            DetectorFlag::Auto => {
                if cfg!(target_os = "linux") {
                    Detector::Netlink
                } else {
                    Detector::Poll
                }
            }
        }
    }
}

/// Resolved detector choice — what the scanner loop actually runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Detector {
    /// Netlink proc connector.
    Netlink,
    /// `/proc` poll.
    Poll,
}

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

    /// Path to the host machine-id file. Default `/etc/machine-id`. Mounted
    /// into the DaemonSet via a single-file hostPath; override only for
    /// tests / sandboxes where the file lives elsewhere.
    #[clap(long, env = "MACHINE_ID_PATH", default_value = DEFAULT_MACHINE_ID_PATH)]
    machine_id_path: PathBuf,

    /// Process-event source. `netlink` (default on Linux) subscribes
    /// to the kernel's proc connector for sub-10 ms detection. `poll`
    /// (the rung-1 fallback, default on non-Linux) walks `/proc` every
    /// `poll_interval_ms`. `auto` picks `netlink` on Linux and `poll`
    /// elsewhere.
    ///
    /// Tradeoffs: `netlink` requires `CAP_NET_ADMIN` on the agent pod
    /// but has lower idle CPU and tighter latency. Under heavy-exec
    /// workloads (`make -j32`, compile farms) it can be more expensive
    /// than `poll`, which only sees processes that survive a tick.
    /// See `docs/src/concepts/emergency-reclaim.md` for the full
    /// comparison.
    #[clap(long, env = "RECLAIM_DETECTOR", default_value = "auto", value_enum)]
    detector: DetectorFlag,

    /// Skip the host-identity cross-check before patching the Node.
    ///
    /// Default: false (strict). Before each PATCH the agent fetches the
    /// target Node, reads its `status.nodeInfo.machineID`, and refuses to
    /// proceed if it does not match `/etc/machine-id` from the agent's
    /// host. This closes the "modified DaemonSet hard-codes NODE_NAME"
    /// impersonation vector documented in Phase 4 of the 2026-04-25
    /// security audit roadmap.
    ///
    /// Set to true ONLY for environments where `/etc/machine-id` is
    /// genuinely unavailable (containers without the host file mounted,
    /// dev sandboxes, kubelet variants that do not populate
    /// `status.nodeInfo.machineID`). The strict default is the safe
    /// posture for production.
    #[clap(long, env = "SKIP_HOST_ID_CHECK", default_value_t = false)]
    skip_host_id_check: bool,
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

    // Read host machine-id once at startup. Failing here (file missing,
    // empty, etc.) is fatal in strict mode so an operator notices and
    // either fixes the mount or sets --skip-host-id-check explicitly.
    let host_machine_id: Option<String> = if cli.skip_host_id_check {
        warn!(
            machine_id_path = %cli.machine_id_path.display(),
            "--skip-host-id-check set: agent will trust NODE_NAME without verifying \
             /etc/machine-id. Use only when the file is genuinely unavailable."
        );
        None
    } else {
        let id = read_host_machine_id(&cli.machine_id_path).with_context(|| {
            format!(
                "read host machine-id from {} (set --skip-host-id-check=true to bypass)",
                cli.machine_id_path.display()
            )
        })?;
        info!(
            machine_id_path = %cli.machine_id_path.display(),
            "host machine-id loaded; will cross-check against Node.status.nodeInfo.machineID before each patch"
        );
        Some(id)
    };

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

    let detector = cli.detector.resolve();
    info!(
        detector = ?detector,
        cli_value = ?cli.detector,
        "detector selected",
    );

    let scanner_result = match detector {
        Detector::Poll => {
            run_scanner(
                &nodes,
                &cli.node_name,
                &cli.proc_root,
                rx,
                cli.oneshot,
                host_machine_id.as_deref(),
            )
            .await
        }
        Detector::Netlink => {
            run_netlink_scanner(
                &nodes,
                &cli.node_name,
                &cli.proc_root,
                rx,
                cli.oneshot,
                host_machine_id.as_deref(),
            )
            .await
        }
    };
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
    host_machine_id: Option<&str>,
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
                    annotate_node(nodes, node_name, &m, host_machine_id).await?;
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

/// Netlink-driven detection loop (rung 2).
///
/// Subscribes once to the kernel proc connector and blocks on the
/// socket for every `exec(2)`. The socket is opened *only* when the
/// shared config transitions to `Some` (no point holding kernel
/// resources while idle); a config-clear closes the subscriber and we
/// return to waiting for arming.
///
/// On every `ProcEvent::Exec` we resolve the pid via the existing
/// rung-1 [`match_pid`] helper — this is the architectural promise of
/// the two-rung ladder: only the *event source* changes between rungs,
/// match logic and Node-PATCH path are shared.
///
/// Falls back to `Err` immediately on subscriber-construction failure
/// (e.g. running on macOS, missing `CAP_NET_ADMIN`, or kernel built
/// without `CONFIG_PROC_EVENTS`). The bin's `main` then propagates the
/// error so the operator sees it; rung-1 fallback is selected via
/// `--detector=poll`, not via silent in-process degradation.
#[allow(clippy::too_many_lines)]
async fn run_netlink_scanner(
    nodes: &Api<Node>,
    node_name: &str,
    proc_root: &Path,
    mut rx: tokio::sync::watch::Receiver<Option<Config>>,
    oneshot: bool,
    host_machine_id: Option<&str>,
) -> Result<()> {
    loop {
        // Wait until the per-node ConfigMap arms us (or exit if
        // oneshot and nothing is armed).
        let cfg = rx.borrow().clone();
        let Some(cfg) = cfg else {
            if oneshot {
                warn!("oneshot mode + netlink: no config present — exiting non-zero");
                return Err(anyhow!("no configmap observed during oneshot run"));
            }
            tokio::select! {
                res = rx.changed() => {
                    if res.is_err() {
                        info!("config channel closed — exiting");
                        return Ok(());
                    }
                    continue;
                }
                () = tokio::time::sleep(Duration::from_secs(IDLE_WAKEUP_SECS)) => continue,
            }
        };

        // Open the subscriber. Surface platform / capability errors at
        // this boundary so the operator sees them in the agent's first
        // log lines after arming.
        let mut sub = match NetlinkSubscriber::new() {
            Ok(s) => s,
            Err(NetlinkError::Unsupported) => {
                bail!(
                    "netlink detector requested but the netlink proc connector is not \
                     supported on this platform — pass --detector=poll to use the /proc \
                     fallback or run on Linux"
                );
            }
            Err(e) => {
                error!(error = %e, "failed to open netlink subscriber");
                bail!(
                    "open netlink proc-connector subscriber (CAP_NET_ADMIN required and \
                     CONFIG_PROC_EVENTS=y in kernel): {e}"
                );
            }
        };
        info!(
            commands = ?cfg.match_commands,
            substrings = ?cfg.match_argv_substrings,
            "netlink subscriber armed — waiting for kernel exec events",
        );

        let proc_root = proc_root.to_path_buf();
        let cfg_arc = Arc::new(cfg);

        // The recv loop must run on a blocking thread — `recv(2)` on the
        // netlink socket is synchronous and would otherwise pin the
        // tokio worker. We spawn it and await its result on the
        // current task; cancellation is via the channel `rx.changed()`
        // arm of the select below.
        let recv_handle = tokio::task::spawn_blocking({
            let proc_root = proc_root.clone();
            let cfg_arc = cfg_arc.clone();
            move || -> Result<Option<Match>> {
                loop {
                    match sub.next_event() {
                        Ok(Some(ProcEvent::Exec { pid, tgid: _ })) => {
                            if let Some(m) = match_pid(&proc_root, pid, &cfg_arc) {
                                return Ok(Some(m));
                            }
                        }
                        Ok(Some(ProcEvent::Other { what })) => {
                            tracing::trace!(what = format_args!("{what:#x}"), "ignored proc_event");
                        }
                        Ok(None) => {
                            // Frame parsed-and-dropped (already logged).
                            continue;
                        }
                        Err(e) => return Err(anyhow!("netlink recv: {e}")),
                    }
                }
            }
        });

        let outcome: Result<Option<Match>> = tokio::select! {
            joined = recv_handle => match joined {
                Ok(res) => res,
                Err(join_err) => Err(anyhow!("netlink recv task panicked: {join_err}")),
            },
            res = rx.changed() => {
                if res.is_err() {
                    info!("config channel closed — exiting");
                    return Ok(());
                }
                // Config changed (likely cleared). Drop the subscriber
                // by returning Ok(None); the outer loop will re-evaluate.
                Ok(None)
            }
        };

        match outcome {
            Ok(Some(m)) => {
                info!(
                    pid = m.pid,
                    pattern = %m.matched_pattern,
                    "netlink match → annotating node",
                );
                annotate_node(nodes, node_name, &m, host_machine_id).await?;
                return Ok(());
            }
            Ok(None) => {
                if oneshot {
                    warn!("oneshot mode + netlink: config cleared before any match — exiting non-zero");
                    return Err(anyhow!("config cleared during oneshot run"));
                }
                // Loop: re-evaluate the (possibly new) config.
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

/// PATCH the Node with reclaim annotations.
///
/// Before the PATCH (when `host_machine_id` is `Some`) the agent fetches
/// the target Node and cross-checks `status.nodeInfo.machineID` against
/// the host machine-id loaded at startup — refusing to patch a Node
/// that does not match. This blocks the
/// "modified-DaemonSet → impersonate-victim-Node" attack documented in
/// Phase 4 of the 2026-04-25 security audit roadmap.
///
/// `host_machine_id == None` means the operator passed
/// `--skip-host-id-check`; the function falls back to the pre-Phase-4
/// behaviour of trusting `NODE_NAME` blindly.
async fn annotate_node(
    nodes: &Api<Node>,
    node_name: &str,
    m: &Match,
    host_machine_id: Option<&str>,
) -> Result<()> {
    if let Some(expected) = host_machine_id {
        let node = nodes
            .get(node_name)
            .await
            .with_context(|| format!("fetch Node/{node_name} for host-identity check"))?;
        if let Err(e) = compare_machine_ids(&node, node_name, expected) {
            error!(error = %e, "host-identity check failed — refusing to patch Node");
            bail!(e);
        }
        debug!(node = %node_name, "host-identity check passed");
    }

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
