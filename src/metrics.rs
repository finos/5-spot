// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Prometheus metrics
//!
//! Observability metrics for monitoring the controller's health, performance,
//! and operational state.  All metrics are registered with the default
//! Prometheus registry and are exposed on the `/metrics` endpoint.
//!
//! ## Registration strategy
//! Metrics are declared as `static LazyLock<…>` values so they are
//! initialised exactly once on first access.  If registration fails (e.g. a
//! duplicate name in tests), the fallback helpers create an *unregistered*
//! metric so the process continues rather than panicking.
//!
//! ## Available metrics
//! | Metric | Type | Description |
//! |---|---|---|
//! | `fivespot_reconciliations_total` | Counter | Reconciliation attempts by phase and result |
//! | `fivespot_reconciliation_duration_seconds` | Histogram | Reconciliation latency |
//! | `fivespot_machines_active` | Gauge | Currently active machines |
//! | `fivespot_machines_by_phase` | Gauge | Machine count per lifecycle phase |
//! | `fivespot_schedule_evaluations_total` | Counter | Schedule evaluations by outcome |
//! | `fivespot_kill_switch_activations_total` | Counter | Kill-switch activations |
//! | `fivespot_controller_info` | Gauge | Controller version and instance metadata |
//! | `fivespot_is_leader` | Gauge | Whether this instance currently holds the leader lease |
//! | `fivespot_errors_total` | Counter | Errors by type |
//! | `fivespot_node_drains_total` | Counter | Node drain attempts by outcome |
//! | `fivespot_pod_evictions_total` | Counter | Pod eviction attempts by outcome |
//! | `fivespot_emergency_drain_duration_seconds` | Histogram | Emergency-reclaim drain wall-clock by outcome |
//! | `fivespot_emergency_reclaims_total` | Counter | Emergency-reclaim events per SM (namespace, name) |
//! | `fivespot_rapid_re_reclaims_total` | Counter | RapidReReclaim warnings per SM (namespace, name) |
//! | `fivespot_kata_config_writes_total` | Counter | Kata drop-in host writes by the node agent |
//! | `fivespot_kata_config_deletes_total` | Counter | Kata drop-in host deletions (GitOps tear-down) |
//! | `fivespot_kata_config_drift_corrected_total` | Counter | Out-of-band edits rewritten without a service restart |
//! | `fivespot_kata_config_restarts_total` | Counter | Host k0s-service restarts issued via nsenter |
//! | `fivespot_kata_config_sync_errors_total` | Counter | Failed kata-config reconcile ticks |
//! | `fivespot_kata_config_last_sync_timestamp_seconds` | Gauge | Unix time of the last successful kata-config reconcile |

use std::sync::LazyLock;

use prometheus::{
    register_counter, register_counter_vec, register_gauge, register_gauge_vec,
    register_histogram_vec, Counter, CounterVec, Gauge, GaugeVec, HistogramVec, Opts,
};

// ============================================================================
// Fallback helpers
//
// If metric registration fails (e.g., duplicate name in tests), we log a
// warning and fall back to an *unregistered* metric so the process continues.
// The fallback constructors use hardcoded metric names that the Prometheus
// crate must accept (ASCII alphanumerics + underscores, non-empty, not
// starting with a digit). They have never failed in practice — but the
// contract is enforced by `prometheus`, not us, so we guard with `expect()`
// carrying a pointed diagnostic rather than `unreachable!()` which compiles
// to a panic with a misleading message. Either way a failure here is a
// programming bug (likely a rename that introduced an invalid character),
// not a runtime configuration issue.
// ============================================================================

/// Error message used by every fallback constructor — identifies the failing
/// metric so a crash log points straight at the offending hardcoded name.
const FALLBACK_METRIC_BUG_MSG: &str = "BUG: hardcoded metric name failed Prometheus validation; \
     this is a programming error, not a runtime issue — \
     see src/metrics.rs for the offending static";

/// Create an *unregistered* `CounterVec` used as a no-op fallback when
/// `register_counter_vec!` fails (e.g. duplicate name in test processes).
fn fallback_counter_vec(name: &str, help: &str, labels: &[&str]) -> CounterVec {
    CounterVec::new(Opts::new(name, help), labels)
        .unwrap_or_else(|e| panic!("{FALLBACK_METRIC_BUG_MSG}: name={name:?} err={e}"))
}

/// Create an *unregistered* `Gauge` used as a no-op fallback when
/// `register_gauge!` fails.
fn fallback_gauge(name: &str, help: &str) -> Gauge {
    Gauge::new(name, help)
        .unwrap_or_else(|e| panic!("{FALLBACK_METRIC_BUG_MSG}: name={name:?} err={e}"))
}

/// Create an *unregistered* `Counter` (label-less) used as a no-op fallback
/// when `register_counter!` fails.
fn fallback_counter(name: &str, help: &str) -> Counter {
    Counter::new(name, help)
        .unwrap_or_else(|e| panic!("{FALLBACK_METRIC_BUG_MSG}: name={name:?} err={e}"))
}

/// Create an *unregistered* `GaugeVec` used as a no-op fallback when
/// `register_gauge_vec!` fails.
fn fallback_gauge_vec(name: &str, help: &str, labels: &[&str]) -> GaugeVec {
    GaugeVec::new(Opts::new(name, help), labels)
        .unwrap_or_else(|e| panic!("{FALLBACK_METRIC_BUG_MSG}: name={name:?} err={e}"))
}

/// Create an *unregistered* `HistogramVec` used as a no-op fallback when
/// `register_histogram_vec!` fails.
fn fallback_histogram_vec(
    name: &str,
    help: &str,
    labels: &[&str],
    buckets: Vec<f64>,
) -> HistogramVec {
    HistogramVec::new(
        prometheus::HistogramOpts::new(name, help).buckets(buckets),
        labels,
    )
    .unwrap_or_else(|e| panic!("{FALLBACK_METRIC_BUG_MSG}: name={name:?} err={e}"))
}

// ============================================================================
// Metrics
// ============================================================================

/// Total number of reconciliations performed
pub static RECONCILIATIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_reconciliations_total",
        "Total number of reconciliations performed",
        &["phase", "result"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_reconciliations_total: {e}");
        fallback_counter_vec(
            "fivespot_reconciliations_total",
            "Total number of reconciliations performed",
            &["phase", "result"],
        )
    })
});

/// Duration of reconciliation operations in seconds
pub static RECONCILIATION_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        "fivespot_reconciliation_duration_seconds",
        "Duration of reconciliation operations in seconds",
        &["phase"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_reconciliation_duration_seconds: {e}");
        fallback_histogram_vec(
            "fivespot_reconciliation_duration_seconds",
            "Duration of reconciliation operations in seconds",
            &["phase"],
            vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
        )
    })
});

/// Number of currently active machines (in Active phase)
pub static MACHINES_ACTIVE: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!(
        "fivespot_machines_active",
        "Number of machines currently in Active phase"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_machines_active: {e}");
        fallback_gauge(
            "fivespot_machines_active",
            "Number of machines currently in Active phase",
        )
    })
});

/// Number of scheduled machines by phase
pub static MACHINES_BY_PHASE: LazyLock<GaugeVec> = LazyLock::new(|| {
    register_gauge_vec!(
        "fivespot_machines_by_phase",
        "Number of ScheduledMachine resources by phase",
        &["phase"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_machines_by_phase: {e}");
        fallback_gauge_vec(
            "fivespot_machines_by_phase",
            "Number of ScheduledMachine resources by phase",
            &["phase"],
        )
    })
});

/// Total number of schedule evaluations
pub static SCHEDULE_EVALUATIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_schedule_evaluations_total",
        "Total number of schedule evaluations",
        &["result"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_schedule_evaluations_total: {e}");
        fallback_counter_vec(
            "fivespot_schedule_evaluations_total",
            "Total number of schedule evaluations",
            &["result"],
        )
    })
});

/// Spot-schedule provider resolutions, labelled by namespace, provider kind,
/// and result (`active` | `inactive` | `unresolved`). Cardinality is bounded by
/// namespace × kind × 3 — deliberately **not** keyed by the (unbounded) provider
/// or SM name (ADR 0006 metrics note).
pub static SPOT_SCHEDULE_RESOLUTIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_spot_schedule_resolutions_total",
        "Total spot-schedule provider resolutions by namespace, kind, and result",
        &["namespace", "kind", "result"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_spot_schedule_resolutions_total: {e}");
        fallback_counter_vec(
            "fivespot_spot_schedule_resolutions_total",
            "Total spot-schedule provider resolutions by namespace, kind, and result",
            &["namespace", "kind", "result"],
        )
    })
});

/// Spot-schedule provider *unresolved* resolutions, labelled by namespace,
/// provider kind, and reason (a `REASON_SPOT_SCHEDULE_*` value). The signal an
/// operator alerts on when a provider-driven machine is holding last-state.
pub static SPOT_SCHEDULE_RESOLUTION_ERRORS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_spot_schedule_resolution_errors_total",
        "Total unresolved spot-schedule provider resolutions by namespace, kind, and reason",
        &["namespace", "kind", "reason"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_spot_schedule_resolution_errors_total: {e}");
        fallback_counter_vec(
            "fivespot_spot_schedule_resolution_errors_total",
            "Total unresolved spot-schedule provider resolutions by namespace, kind, and reason",
            &["namespace", "kind", "reason"],
        )
    })
});

/// Spot-schedule provider active⇄inactive transitions, labelled by namespace and
/// provider kind. A high rate is the flapping signal called out in the threat
/// model.
pub static SPOT_SCHEDULE_TRANSITIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_spot_schedule_transitions_total",
        "Total spot-schedule active/inactive transitions by namespace and kind",
        &["namespace", "kind"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_spot_schedule_transitions_total: {e}");
        fallback_counter_vec(
            "fivespot_spot_schedule_transitions_total",
            "Total spot-schedule active/inactive transitions by namespace and kind",
            &["namespace", "kind"],
        )
    })
});

/// Number of machines with kill switch activated
pub static KILL_SWITCH_ACTIVATIONS_TOTAL: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!(
        "fivespot_kill_switch_activations_total",
        "Total number of kill switch activations"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kill_switch_activations_total: {e}");
        fallback_gauge(
            "fivespot_kill_switch_activations_total",
            "Total number of kill switch activations",
        )
    })
});

/// Controller info gauge (always 1, used for labels)
pub static CONTROLLER_INFO: LazyLock<GaugeVec> = LazyLock::new(|| {
    register_gauge_vec!(
        "fivespot_controller_info",
        "Controller information",
        &["version", "instance_id"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_controller_info: {e}");
        fallback_gauge_vec(
            "fivespot_controller_info",
            "Controller information",
            &["version", "instance_id"],
        )
    })
});

/// Whether the controller is the leader (1 = leader, 0 = not leader)
pub static IS_LEADER: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!(
        "fivespot_is_leader",
        "Whether this controller instance is the leader (1 = leader, 0 = not leader)"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_is_leader: {e}");
        fallback_gauge(
            "fivespot_is_leader",
            "Whether this controller instance is the leader",
        )
    })
});

/// Number of errors by type
pub static ERRORS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_errors_total",
        "Total number of errors by type",
        &["error_type"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_errors_total: {e}");
        fallback_counter_vec(
            "fivespot_errors_total",
            "Total number of errors by type",
            &["error_type"],
        )
    })
});

/// Node drain operations
pub static NODE_DRAINS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_node_drains_total",
        "Total number of node drain operations",
        &["result"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_node_drains_total: {e}");
        fallback_counter_vec(
            "fivespot_node_drains_total",
            "Total number of node drain operations",
            &["result"],
        )
    })
});

/// Duration of emergency-reclaim drain operations in seconds.
///
/// Histogram observed once per `EmergencyRemove` flow, labelled by
/// outcome (`success` / `timeout` / `error`). Buckets are sized for the
/// `EMERGENCY_DRAIN_TIMEOUT_SECS = 60` ceiling — anything past 60 s is a
/// flow-killing timeout that the controller force-deletes through anyway.
///
/// Operator SLO: P95 of `outcome="success"` should sit well below 30 s on
/// a healthy fleet. A growing P95 signals workloads with bad
/// `terminationGracePeriodSeconds` defaults or PodDisruptionBudgets that
/// are clipping the drain. A non-zero `outcome="timeout"` rate means the
/// 60 s ceiling is biting — investigate the workloads on the affected
/// nodes via the `fivespot_pod_evictions_total{result="failure"}` metric
/// alongside.
pub static EMERGENCY_DRAIN_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        "fivespot_emergency_drain_duration_seconds",
        "Wall-clock duration of emergency-reclaim node drains, by outcome",
        &["outcome"],
        vec![0.5, 1.0, 2.5, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_emergency_drain_duration_seconds: {e}");
        fallback_histogram_vec(
            "fivespot_emergency_drain_duration_seconds",
            "Wall-clock duration of emergency-reclaim node drains, by outcome",
            &["outcome"],
            vec![0.5, 1.0, 2.5, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0],
        )
    })
});

/// Total emergency-reclaim events recorded per ScheduledMachine,
/// labelled by namespace + name. Used by the loop-protection logic to
/// detect rapid re-fires (a user re-enabling a SM whose conflicting
/// process is still running) — alert when the rate per SM exceeds the
/// `RAPID_RE_RECLAIM_THRESHOLD` constant within a 10-minute window.
pub static EMERGENCY_RECLAIMS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_emergency_reclaims_total",
        "Total emergency-reclaim events fired per ScheduledMachine",
        &["namespace", "name"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_emergency_reclaims_total: {e}");
        fallback_counter_vec(
            "fivespot_emergency_reclaims_total",
            "Total emergency-reclaim events fired per ScheduledMachine",
            &["namespace", "name"],
        )
    })
});

/// Total rapid re-reclaim warnings emitted, labelled by namespace +
/// name. Operator-actionable signal — every increment corresponds to a
/// `RapidReReclaim` Warning Event on the SM whose user has re-enabled
/// the schedule without first stopping the conflicting process.
pub static RAPID_RE_RECLAIMS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_rapid_re_reclaims_total",
        "Total RapidReReclaim warnings emitted per ScheduledMachine",
        &["namespace", "name"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_rapid_re_reclaims_total: {e}");
        fallback_counter_vec(
            "fivespot_rapid_re_reclaims_total",
            "Total RapidReReclaim warnings emitted per ScheduledMachine",
            &["namespace", "name"],
        )
    })
});

/// Pod evictions during node drain
pub static POD_EVICTIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_pod_evictions_total",
        "Total number of pod evictions during node drain",
        &["result"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_pod_evictions_total: {e}");
        fallback_counter_vec(
            "fivespot_pod_evictions_total",
            "Total number of pod evictions during node drain",
            &["result"],
        )
    })
});

/// Finalizer cleanup timeouts during deletion handling.
///
/// Incremented every time `handle_deletion` exceeds
/// [`crate::constants::FINALIZER_CLEANUP_TIMEOUT_SECS`] while removing a
/// machine from its cluster. A non-zero value typically indicates a
/// misconfigured Pod Disruption Budget on a workload that the controller
/// is trying to evict — the controller force-removes the finalizer to
/// unblock namespace deletion, but this metric tells operators an
/// orphaned CAPI Machine + bootstrap/infrastructure resources may need
/// manual cleanup.
///
/// Alert when the rate is non-zero. See
/// `docs/src/operations/troubleshooting.md` for the orphan-cleanup runbook.
pub static FINALIZER_CLEANUP_TIMEOUTS_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter!(
        "fivespot_finalizer_cleanup_timeouts_total",
        "Total number of finalizer cleanup timeouts (force-removed; possible orphan resources)"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_finalizer_cleanup_timeouts_total: {e}");
        fallback_counter(
            "fivespot_finalizer_cleanup_timeouts_total",
            "Total number of finalizer cleanup timeouts (force-removed; possible orphan resources)",
        )
    })
});

/// Total number of child-cluster kubeconfig resolutions, labelled by
/// outcome. Label values:
///
/// - `management` — neither an explicit ref nor an auto-discovered
///   Secret was present; the controller used the management client.
/// - `child_explicit` — built a child client from
///   `spec.kubeconfigSecretRef`.
/// - `child_auto` — built a child client from the auto-discovered
///   `<clusterName>-kubeconfig` Secret.
/// - `cache_hit` — same `resourceVersion`, returned the cached client.
/// - `rebuild` — `resourceVersion` changed, rebuilt the child client.
/// - `error` — resolution failed (see
///   `fivespot_child_kubeconfig_errors_total` for the breakdown).
pub static CHILD_KUBECONFIG_RESOLUTIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_child_kubeconfig_resolutions_total",
        "Total number of child-cluster kubeconfig resolutions by outcome",
        &["result"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_child_kubeconfig_resolutions_total: {e}");
        fallback_counter_vec(
            "fivespot_child_kubeconfig_resolutions_total",
            "Total number of child-cluster kubeconfig resolutions by outcome",
            &["result"],
        )
    })
});

/// Total number of child-cluster kubeconfig resolution errors, labelled
/// by reason:
/// - `secret_missing_key` — Secret exists but lacks `data[key]`
/// - `invalid_yaml`       — kubeconfig YAML parse failed
/// - `unreachable`        — explicit ref 404 / Secret GET network error
/// - `non_404_kube_error` — non-404 kube::Error during auto-discovery GET
///
/// Operators should alert on a non-trivial rate of any reason: every
/// error here means at least one `ScheduledMachine` is silently failing
/// its Node/Pod operations on the workload cluster.
pub static CHILD_KUBECONFIG_ERRORS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        "fivespot_child_kubeconfig_errors_total",
        "Total number of child-cluster kubeconfig resolution errors by reason",
        &["reason"]
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_child_kubeconfig_errors_total: {e}");
        fallback_counter_vec(
            "fivespot_child_kubeconfig_errors_total",
            "Total number of child-cluster kubeconfig resolution errors by reason",
            &["reason"],
        )
    })
});

/// Kata drop-in host writes performed by the node agent (new-config rollouts
/// **and** drift corrections — slice the latter out via
/// `fivespot_kata_config_drift_corrected_total`).
pub static KATA_CONFIG_WRITES_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter!(
        "fivespot_kata_config_writes_total",
        "Total kata drop-in files written to the host by the node agent"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kata_config_writes_total: {e}");
        fallback_counter(
            "fivespot_kata_config_writes_total",
            "Total kata drop-in files written to the host by the node agent",
        )
    })
});

/// Kata drop-in host deletions (source object/key/annotation cleared —
/// GitOps: absent in source ⇒ absent on host).
pub static KATA_CONFIG_DELETES_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter!(
        "fivespot_kata_config_deletes_total",
        "Total kata drop-in files removed from the host by the node agent"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kata_config_deletes_total: {e}");
        fallback_counter(
            "fivespot_kata_config_deletes_total",
            "Total kata drop-in files removed from the host by the node agent",
        )
    })
});

/// Writes that restored content already applied (and restarted for) — i.e.
/// corrections of out-of-band edits. These rewrite the file but do **not**
/// bounce the host k0s service (ADR 0003 restart-loop guard). A sustained
/// non-zero rate means something on the node keeps editing the drop-in.
pub static KATA_CONFIG_DRIFT_CORRECTED_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter!(
        "fivespot_kata_config_drift_corrected_total",
        "Total out-of-band kata drop-in edits corrected without a service restart"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kata_config_drift_corrected_total: {e}");
        fallback_counter(
            "fivespot_kata_config_drift_corrected_total",
            "Total out-of-band kata drop-in edits corrected without a service restart",
        )
    })
});

/// Host k0s-service restarts issued via `nsenter` (ADR 0003). Expect exactly
/// one per distinct config change per node; a higher rate signals a
/// restart loop (e.g. two writers fighting over the same destPath).
pub static KATA_CONFIG_RESTARTS_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter!(
        "fivespot_kata_config_restarts_total",
        "Total host k0s-service restarts issued by the kata-config agent"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kata_config_restarts_total: {e}");
        fallback_counter(
            "fivespot_kata_config_restarts_total",
            "Total host k0s-service restarts issued by the kata-config agent",
        )
    })
});

/// Failed kata-config reconcile ticks (API fetch, host I/O, annotation PATCH,
/// or restart errors). The agent retries next tick; alert on a sustained rate.
pub static KATA_CONFIG_SYNC_ERRORS_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter!(
        "fivespot_kata_config_sync_errors_total",
        "Total failed kata-config reconcile ticks"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kata_config_sync_errors_total: {e}");
        fallback_counter(
            "fivespot_kata_config_sync_errors_total",
            "Total failed kata-config reconcile ticks",
        )
    })
});

/// Unix timestamp of the last successful kata-config reconcile tick
/// (regardless of whether it wrote, deleted, or found everything in sync).
/// Alert when `time() - this` exceeds a few poll intervals — the agent is
/// wedged or cannot reach the API.
pub static KATA_CONFIG_LAST_SYNC_TIMESTAMP_SECONDS: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!(
        "fivespot_kata_config_last_sync_timestamp_seconds",
        "Unix time of the last successful kata-config reconcile tick"
    )
    .unwrap_or_else(|e| {
        eprintln!("WARN: Failed to register fivespot_kata_config_last_sync_timestamp_seconds: {e}");
        fallback_gauge(
            "fivespot_kata_config_last_sync_timestamp_seconds",
            "Unix time of the last successful kata-config reconcile tick",
        )
    })
});

/// Record a successful reconciliation
pub fn record_reconciliation_success(phase: &str, duration_secs: f64) {
    RECONCILIATIONS_TOTAL
        .with_label_values(&[phase, "success"])
        .inc();
    RECONCILIATION_DURATION_SECONDS
        .with_label_values(&[phase])
        .observe(duration_secs);
}

/// Record a failed reconciliation
pub fn record_reconciliation_failure(phase: &str, duration_secs: f64) {
    RECONCILIATIONS_TOTAL
        .with_label_values(&[phase, "failure"])
        .inc();
    RECONCILIATION_DURATION_SECONDS
        .with_label_values(&[phase])
        .observe(duration_secs);
}

/// Record a schedule evaluation result
pub fn record_schedule_evaluation(is_active: bool) {
    let result = if is_active { "active" } else { "inactive" };
    SCHEDULE_EVALUATIONS_TOTAL
        .with_label_values(&[result])
        .inc();
}

/// Record a spot-schedule resolution outcome. `result` is `active`, `inactive`,
/// or `unresolved`.
pub fn record_spot_schedule_resolution(namespace: &str, kind: &str, result: &str) {
    SPOT_SCHEDULE_RESOLUTIONS_TOTAL
        .with_label_values(&[namespace, kind, result])
        .inc();
}

/// Record an unresolved spot-schedule resolution, by `reason`
/// (a `REASON_SPOT_SCHEDULE_*` value).
pub fn record_spot_schedule_resolution_error(namespace: &str, kind: &str, reason: &str) {
    SPOT_SCHEDULE_RESOLUTION_ERRORS_TOTAL
        .with_label_values(&[namespace, kind, reason])
        .inc();
}

/// Record a spot-schedule active⇄inactive transition.
pub fn record_spot_schedule_transition(namespace: &str, kind: &str) {
    SPOT_SCHEDULE_TRANSITIONS_TOTAL
        .with_label_values(&[namespace, kind])
        .inc();
}

/// Update the count of machines in a specific phase
pub fn set_machines_by_phase(phase: &str, count: f64) {
    MACHINES_BY_PHASE.with_label_values(&[phase]).set(count);
}

/// Record an error by type
pub fn record_error(error_type: &str) {
    ERRORS_TOTAL.with_label_values(&[error_type]).inc();
}

/// Record a node drain result
pub fn record_node_drain(success: bool) {
    let result = if success { "success" } else { "failure" };
    NODE_DRAINS_TOTAL.with_label_values(&[result]).inc();
}

/// Record a pod eviction result
pub fn record_pod_eviction(success: bool) {
    let result = if success { "success" } else { "failure" };
    POD_EVICTIONS_TOTAL.with_label_values(&[result]).inc();
}

/// Outcome label for [`record_emergency_drain`]. Stable strings —
/// changing them is a Prometheus-dashboard breaking change.
#[derive(Clone, Copy, Debug)]
pub enum EmergencyDrainOutcome {
    /// Drain returned cleanly within the timeout.
    Success,
    /// Drain hit `EMERGENCY_DRAIN_TIMEOUT_SECS` and was force-completed
    /// without all pods evicting.
    Timeout,
    /// Drain returned an error other than timeout (apiserver 5xx, etc.).
    Error,
}

impl EmergencyDrainOutcome {
    fn as_str(self) -> &'static str {
        match self {
            EmergencyDrainOutcome::Success => "success",
            EmergencyDrainOutcome::Timeout => "timeout",
            EmergencyDrainOutcome::Error => "error",
        }
    }
}

/// Observe one emergency-drain duration sample. Called once per
/// `EmergencyRemove` flow regardless of outcome — operators slice by
/// the `outcome` label to compute success-only P95 vs timeout rate
/// separately.
pub fn record_emergency_drain(duration_secs: f64, outcome: EmergencyDrainOutcome) {
    EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&[outcome.as_str()])
        .observe(duration_secs);
}

/// Increment the per-SM emergency-reclaim counter. Called once per
/// successful entry into the `EmergencyRemove` flow (after the
/// idempotent recovery handler short-circuits, so a controller restart
/// mid-flow does not double-count).
pub fn record_emergency_reclaim(namespace: &str, name: &str) {
    EMERGENCY_RECLAIMS_TOTAL
        .with_label_values(&[namespace, name])
        .inc();
}

/// Increment the per-SM rapid-re-reclaim warning counter. Called when
/// the loop-protection logic detects ≥`RAPID_RE_RECLAIM_THRESHOLD`
/// reclaims for the same SM within `RAPID_RE_RECLAIM_WINDOW_SECS` and
/// emits a `RapidReReclaim` Warning Event.
pub fn record_rapid_re_reclaim(namespace: &str, name: &str) {
    RAPID_RE_RECLAIMS_TOTAL
        .with_label_values(&[namespace, name])
        .inc();
}

/// Record a finalizer-cleanup timeout (force-remove path).
///
/// Operators should treat any non-zero rate as a signal that orphan CAPI
/// Machine / bootstrap / infrastructure resources may exist and need
/// manual reconciliation.
/// Record one child-cluster kubeconfig resolution. `result` must be one
/// of the documented outcomes on
/// [`CHILD_KUBECONFIG_RESOLUTIONS_TOTAL`].
pub fn record_child_kubeconfig_resolution(result: &str) {
    CHILD_KUBECONFIG_RESOLUTIONS_TOTAL
        .with_label_values(&[result])
        .inc();
}

/// Record one child-cluster kubeconfig resolution error. `reason` must
/// be one of the documented categories on
/// [`CHILD_KUBECONFIG_ERRORS_TOTAL`].
pub fn record_child_kubeconfig_error(reason: &str) {
    CHILD_KUBECONFIG_ERRORS_TOTAL
        .with_label_values(&[reason])
        .inc();
}

pub fn record_finalizer_cleanup_timeout() {
    FINALIZER_CLEANUP_TIMEOUTS_TOTAL.inc();
}

/// Stamp [`KATA_CONFIG_LAST_SYNC_TIMESTAMP_SECONDS`] with the current wall
/// clock. A pre-epoch system clock (the only failure mode) degrades to not
/// stamping rather than panicking.
fn stamp_kata_config_last_sync() {
    if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        KATA_CONFIG_LAST_SYNC_TIMESTAMP_SECONDS.set(now.as_secs_f64());
    }
}

/// Record one kata drop-in host write. `drift_corrected` is `true` when the
/// write restored content already applied (see
/// `kata_config_agent::is_drift_correction`) — it then also increments
/// [`KATA_CONFIG_DRIFT_CORRECTED_TOTAL`].
pub fn record_kata_config_write(drift_corrected: bool) {
    KATA_CONFIG_WRITES_TOTAL.inc();
    if drift_corrected {
        KATA_CONFIG_DRIFT_CORRECTED_TOTAL.inc();
    }
    stamp_kata_config_last_sync();
}

/// Record one kata drop-in host deletion (GitOps tear-down).
pub fn record_kata_config_delete() {
    KATA_CONFIG_DELETES_TOTAL.inc();
    stamp_kata_config_last_sync();
}

/// Record an in-sync kata-config tick — only refreshes the last-sync gauge.
pub fn record_kata_config_sync_unchanged() {
    stamp_kata_config_last_sync();
}

/// Record one host k0s-service restart issued by the kata-config agent.
pub fn record_kata_config_restart() {
    KATA_CONFIG_RESTARTS_TOTAL.inc();
}

/// Record one failed kata-config reconcile tick.
pub fn record_kata_config_sync_error() {
    KATA_CONFIG_SYNC_ERRORS_TOTAL.inc();
}

/// Serve the default Prometheus registry on `/metrics` at `0.0.0.0:<port>`.
/// Shared by the controller and the node agents — runs until the process
/// exits, so callers `tokio::spawn` it.
pub async fn serve_metrics(port: u16) {
    use prometheus::{Encoder, TextEncoder};
    use warp::Filter;

    tracing::info!(port = port, "Starting metrics server");

    let metrics = warp::path("metrics").map(|| {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = vec![];

        match encoder.encode(&metric_families, &mut buffer) {
            Ok(()) => warp::reply::with_status(
                String::from_utf8_lossy(&buffer).to_string(),
                warp::http::StatusCode::OK,
            ),
            Err(e) => {
                tracing::error!(error = %e, "Failed to encode metrics");
                warp::reply::with_status(
                    format!("# Error encoding metrics: {e}\n"),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
            }
        }
    });

    warp::serve(metrics.boxed()).run(([0, 0, 0, 0], port)).await;
}

/// Initialize controller info metric
pub fn init_controller_info(version: &str, instance_id: u32) {
    CONTROLLER_INFO
        .with_label_values(&[version, &instance_id.to_string()])
        .set(1.0);
}

/// Set leader status
pub fn set_leader_status(is_leader: bool) {
    IS_LEADER.set(if is_leader { 1.0 } else { 0.0 });
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
