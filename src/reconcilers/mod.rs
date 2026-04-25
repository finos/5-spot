// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Reconcilers
//!
//! This module contains all reconciliation logic for 5-Spot custom resources.
//!
//! ## Sub-modules
//! - `scheduled_machine` — top-level reconciliation entry point, phase-state
//!   machine, [`Context`], and [`ReconcilerError`]
//! - `helpers` — pure helper functions for schedule evaluation, CAPI resource
//!   creation/deletion, node draining, status patching, and security validation
//!
//! ## Re-exports
//! The most commonly used symbols are re-exported at this level so callers only
//! need `use crate::reconcilers::{…}`.

mod helpers;
pub mod scheduled_machine;

// Re-export main types and functions
pub use helpers::{
    error_policy, evaluate_schedule, machine_to_scheduled_machine, node_to_scheduled_machines,
    parse_duration, reconcile_node_taints, should_process_resource, validate_cluster_name,
    validate_kill_if_commands, NodeTaintReconcileOutcome, ReconcileNodeTaintsInput,
};
pub use scheduled_machine::{reconcile_scheduled_machine, Context, ReconcilerError};
