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

/// Record a finalizer-cleanup timeout (force-remove path).
///
/// Operators should treat any non-zero rate as a signal that orphan CAPI
/// Machine / bootstrap / infrastructure resources may exist and need
/// manual reconciliation.
pub fn record_finalizer_cleanup_timeout() {
    FINALIZER_CLEANUP_TIMEOUTS_TOTAL.inc();
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
