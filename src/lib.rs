//! # five_spot — library crate
//!
//! Public API surface for the 5-Spot ScheduledMachine controller.
//!
//! Modules:
//! - [`constants`] — all named constants (timing, labels, phases, CAPI API strings)
//! - [`crd`] — `ScheduledMachine` CRD type definitions (source of truth for YAML generation)
//! - [`health`] — HTTP health and readiness server
//! - [`labels`] — standard Kubernetes label helpers
//! - [`metrics`] — Prometheus metric definitions and recording helpers
//! - [`reconcilers`] — reconciliation logic and controller context

pub mod constants;
pub mod crd;
pub mod health;
pub mod labels;
pub mod metrics;
pub mod reconcilers;

// Re-export main types
pub use crd::ScheduledMachine;
pub use health::HealthState;
pub use metrics::{
    init_controller_info, record_error, record_node_drain, record_pod_eviction,
    record_reconciliation_failure, record_reconciliation_success, record_schedule_evaluation,
    set_leader_status, set_machines_by_phase,
};
pub use reconcilers::{Context, ReconcilerError};
