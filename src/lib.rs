// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # `five_spot` — library crate
//!
//! Public API surface for the 5-Spot `ScheduledMachine` controller.
//!
//! Modules:
//! - [`auto_vex_presence`] — presence-based auto-VEX generation (CI-only, roadmap Phase 2)
//! - [`auto_vex_reachability`] — symbol-import reachability auto-VEX (CI-only, roadmap Phase 3)
//! - [`constants`] — all named constants (timing, labels, phases, CAPI API strings)
//! - [`crd`] — `ScheduledMachine` CRD type definitions (source of truth for YAML generation)
//! - [`health`] — HTTP health and readiness server
//! - [`labels`] — standard Kubernetes label helpers
//! - [`metrics`] — Prometheus metric definitions and recording helpers
//! - [`reconcilers`] — reconciliation logic and controller context

pub mod auto_vex_presence;
pub mod auto_vex_reachability;
pub mod constants;
pub mod crd;
pub mod health;
pub mod labels;
pub mod metrics;
pub mod reclaim_agent;
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
