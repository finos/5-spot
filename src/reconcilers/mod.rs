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

pub mod capacity_decision;
pub mod capacity_path;
pub mod child_client;
pub mod child_watch;
pub mod dynamic_ref_watch;
mod helpers;
pub mod scheduled_capacity;
pub mod scheduled_machine;
pub mod spot_schedule;

// Re-export main types and functions
pub use capacity_decision::{decide, CapacityDecision, CapacityDecisionInput};
pub use capacity_path::{
    build_merge_patch, read_i64_at, validate_drained_path, validate_write_path, CapacityPathError,
};
pub use child_client::{
    CacheKey, ChildClientCache, ChildWatchHook, ResolvedClient, DEFAULT_KUBECONFIG_SECRET_KEY,
};
pub use child_watch::ChildNodeWatchManager;
pub use dynamic_ref_watch::{
    capacity_schedule_key_for, capacity_target_key_for, provider_key_for, CapacityRefWatchManager,
    DynamicRefWatchManager, KeyExtractor, ProviderKey, ReverseIndex, SpotScheduleWatchManager,
    WATCH_LABEL_CAPACITY_TARGET, WATCH_LABEL_SPOT_SCHEDULE,
};
#[allow(deprecated)] // re-export of legacy node_to_scheduled_machines for one release
pub use helpers::{
    build_clear_reclaim_patch, build_kata_config_label_patch,
    build_kata_config_ref_annotation_patch, compose_should_be_active, error_policy,
    machine_to_scheduled_machine, node_reclaim_request, node_to_scheduled_machines,
    node_to_scheduled_machines_via_machine, parse_duration, reconcile_node_taints,
    should_process_resource, validate_cluster_name, validate_kill_if_commands,
    validate_schedule_ref, NodeTaintReconcileOutcome, ReclaimRequest, ReconcileNodeTaintsInput,
};
pub use scheduled_capacity::{CapacityContext, CapacityError};
pub use scheduled_machine::{reconcile_scheduled_machine, Context, ReconcilerError};
pub use spot_schedule::{resolve_spot_schedule, verdict_from_status, SpotScheduleVerdict};
