// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
//! # `ScheduledMachine` reconciler
//!
//! Implements the Kubernetes controller reconciliation loop for
//! [`ScheduledMachine`] custom resources.
//!
//! ## Lifecycle state machine
//!
//! ```text
//! Pending ──► Active ──► ShuttingDown ──► Inactive ──► Pending (loop)
//!   │                                                       ▲
//!   └──► Disabled ─────────────────────────────────────────┘
//!
//! Any state ──► Terminated  (kill switch)
//! Any state ──► Error       (unrecoverable failure)
//! Error     ──► Pending     (automatic recovery attempt)
//! ```
//!
//! ## Entry points
//! - [`reconcile_scheduled_machine`] — called by the `kube-rs` controller loop
//! - [`error_policy`] — determines requeue interval after a reconciliation error
//!
//! ## Multi-instance distribution
//! When `instance_count > 1`, each resource is deterministically assigned to
//! one instance via consistent hashing on `namespace/name`.  Instances that
//! are not assigned to a resource skip it with `Action::await_change`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;

use kube::{
    runtime::{
        controller::Action,
        events::{Recorder, Reporter},
    },
    Client, Resource, ResourceExt,
};
use tracing::{debug, error, info, Instrument};

use crate::constants::{
    ERROR_REQUEUE_SECS, PHASE_ACTIVE, PHASE_DISABLED, PHASE_ERROR, PHASE_INACTIVE, PHASE_PENDING,
    PHASE_SHUTTING_DOWN, PHASE_TERMINATED, REASON_GRACE_PERIOD, REASON_MACHINE_CREATED,
    REASON_MACHINE_DELETED, REASON_SCHEDULE_ACTIVE, REASON_SCHEDULE_DISABLED,
    REASON_SCHEDULE_INACTIVE, TIMER_REQUEUE_SECS,
};
use crate::crd::ScheduledMachine;
use crate::metrics::{
    record_error, record_reconciliation_failure, record_reconciliation_success,
    record_schedule_evaluation, KILL_SWITCH_ACTIVATIONS_TOTAL,
};

// ============================================================================
// Context for reconciliation
// ============================================================================

/// Controller name used for event reporting
pub const CONTROLLER_NAME: &str = "5spot-controller";

/// Shared state passed to every reconciliation call.
///
/// `Context` is cheaply cloneable (all fields are `Arc`-backed or `Copy`) and
/// is wrapped in [`Arc`] by the controller framework before being handed to
/// [`reconcile_scheduled_machine`].
#[derive(Clone)]
pub struct Context {
    /// Kubernetes API client authenticated via the controller's service account.
    pub client: Client,
    /// Zero-based index of this operator instance (set via `OPERATOR_INSTANCE_ID`).
    pub instance_id: u32,
    /// Total number of running operator instances (set via `OPERATOR_INSTANCE_COUNT`).
    /// When `1`, every resource is processed by this instance.
    pub instance_count: u32,
    /// Kubernetes event recorder for publishing immutable audit-trail events.
    pub recorder: Recorder,
    /// Per-resource reconciliation error counts, keyed by `"namespace/name"`.
    ///
    /// Incremented by [`error_policy`](crate::reconcilers::error_policy) on each
    /// failure and cleared by `reconcile_guarded` on success, so back-off resets
    /// after the resource recovers.  Uses a `std::sync::Mutex` because
    /// `error_policy` is synchronous.
    pub retry_counts: Arc<Mutex<HashMap<String, u32>>>,
}

impl Context {
    /// Create a new `Context`, initialising the event recorder from the client.
    ///
    /// The recorder uses `CONTROLLER_POD_NAME` from the environment as the
    /// reporting instance name (optional; falls back to `None`).
    #[must_use]
    pub fn new(client: Client, instance_id: u32, instance_count: u32) -> Self {
        let reporter = Reporter {
            controller: CONTROLLER_NAME.to_string(),
            instance: std::env::var("CONTROLLER_POD_NAME").ok(),
        };
        let recorder = Recorder::new(client.clone(), reporter);
        Self {
            client,
            instance_id,
            instance_count,
            recorder,
            retry_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// ============================================================================
// Error types
// ============================================================================

/// All errors that can be returned by the reconciliation path.
///
/// Variants map to specific Prometheus error label values recorded via
/// [`record_error`](crate::metrics::record_error) so each failure mode is
/// individually observable.
#[derive(Debug, thiserror::Error)]
pub enum ReconcilerError {
    /// A Kubernetes API call failed (network error, 5xx, auth, etc.).
    /// Automatically converted from [`kube::Error`].
    #[error("Kubernetes API error: {0}")]
    KubeError(#[from] kube::Error),

    /// A required Kubernetes resource does not exist.
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// The `ScheduledMachine` spec contains invalid or incomplete configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Schedule parsing or evaluation failed (invalid timezone, bad day/hour range, etc.).
    #[error("Schedule evaluation error: {0}")]
    ScheduleError(String),

    /// A Cluster API operation failed (resource creation, deletion, drain, etc.).
    #[error("CAPI operation failed: {0}")]
    CapiError(String),

    /// Bootstrap or infrastructure file content could not be resolved.
    #[error("File content resolution failed: {0}")]
    FileResolutionError(String),

    /// A cross-resource reference (bootstrap ref, infra ref) is invalid.
    #[error("Reference validation failed: {0}")]
    ReferenceValidationError(String),

    /// An input field failed a security validation check (reserved label prefix,
    /// disallowed API group, etc.).
    #[error("Security validation failed: {0}")]
    ValidationError(String),

    /// An async operation exceeded its configured deadline (e.g. finalizer cleanup).
    #[error("Operation timed out: {0}")]
    TimeoutError(String),

    /// Catch-all for unexpected errors from third-party libraries.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// ============================================================================
// Main reconciliation logic
// ============================================================================

/// Generates a short, unique correlation ID for a single reconciliation attempt.
///
/// The ID combines the last `-`-separated segment of the resource's Kubernetes
/// UID with a hex-encoded nanosecond timestamp.  Because the UID is stable per
/// resource and the timestamp has nanosecond resolution, the resulting ID is
/// unique in practice across all reconciliations of the same resource.
///
/// Format: `{uid_suffix}-{timestamp_ns_hex}` — e.g. `deadbeef0001-17f3e2a1b`.
///
/// Falls back to `unknown` as the UID prefix when `metadata.uid` is absent
/// (e.g. in tests that construct a bare resource without calling the API).
pub(crate) fn generate_reconcile_id(resource: &ScheduledMachine) -> String {
    let uid_suffix = resource
        .metadata
        .uid
        .as_deref()
        .and_then(|u| u.split('-').next_back())
        .unwrap_or("unknown");
    let ts_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{uid_suffix}-{ts_ns:x}")
}

/// Main reconciliation entry point with finalizer handling
///
/// Every reconciliation is wrapped in a `tracing` span carrying a unique
/// `reconcile_id` field.  In JSON log mode this field appears on every log
/// line emitted during the reconciliation, enabling full correlation in a
/// SIEM or log-aggregation platform (NIST AU-3 / SOX §404).
///
/// # Errors
/// Returns error if schedule evaluation, k8s API calls, or machine lifecycle operations fail
pub async fn reconcile_scheduled_machine(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        record_error("invalid_config");
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();
    let reconcile_id = generate_reconcile_id(&resource);

    let span = tracing::info_span!(
        "reconcile",
        resource = %name,
        namespace = %namespace,
        reconcile_id = %reconcile_id,
    );

    reconcile_guarded(resource, ctx, namespace, name, reconcile_id)
        .instrument(span)
        .await
}

/// Inner reconciliation body — separated so it can be fully wrapped in a tracing span.
///
/// # Errors
/// Returns error if schedule evaluation, k8s API calls, or machine lifecycle operations fail
async fn reconcile_guarded(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    namespace: String,
    name: String,
    reconcile_id: String,
) -> Result<Action, ReconcilerError> {
    let start_time = Instant::now();

    // Get current phase for metrics (clone to avoid borrow issues)
    let current_phase = resource
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| PHASE_PENDING.to_string());

    info!(
        resource = %name,
        namespace = %namespace,
        reconcile_id = %reconcile_id,
        priority = resource.spec.priority,
        "Starting reconciliation"
    );

    // Check if this instance should process this resource
    if !should_process_resource(
        &name,
        &namespace,
        resource.spec.priority,
        ctx.instance_count,
    ) {
        info!(
            resource = %name,
            namespace = %namespace,
            priority = resource.spec.priority,
            instance_id = ctx.instance_id,
            "Skipping resource - assigned to different instance"
        );
        return Ok(Action::await_change());
    }

    // Check for deletion
    if resource.meta().deletion_timestamp.is_some() {
        let result = handle_deletion(resource, ctx).await;
        record_reconciliation_result(&result, &current_phase, start_time.elapsed());
        return result;
    }

    // Add finalizer if not present
    if !has_finalizer(&resource) {
        let result = add_finalizer(resource, ctx).await;
        record_reconciliation_result(&result, &current_phase, start_time.elapsed());
        return result;
    }

    // Perform actual reconciliation
    let result = reconcile_inner(resource, ctx.clone()).await;
    record_reconciliation_result(&result, &current_phase, start_time.elapsed());

    // Reset the exponential back-off counter on success so the resource
    // starts from the base delay if it encounters a future error.
    if result.is_ok() {
        let key = format!("{namespace}/{name}");
        if let Ok(mut counts) = ctx.retry_counts.lock() {
            counts.remove(&key);
        }
    }

    result
}

/// Record Prometheus metrics for a completed reconciliation attempt.
///
/// On success, increments the success counter and records duration.
/// On failure, increments the failure counter, records duration, and
/// additionally increments a per-error-type counter so each failure mode
/// is individually observable.
fn record_reconciliation_result(
    result: &Result<Action, ReconcilerError>,
    phase: &str,
    duration: Duration,
) {
    let duration_secs = duration.as_secs_f64();
    match result {
        Ok(_) => record_reconciliation_success(phase, duration_secs),
        Err(e) => {
            record_reconciliation_failure(phase, duration_secs);
            // Record specific error types
            match e {
                ReconcilerError::KubeError(_) => record_error("kube_api"),
                ReconcilerError::NotFound(_) => record_error("not_found"),
                ReconcilerError::InvalidConfig(_) => record_error("invalid_config"),
                ReconcilerError::ScheduleError(_) => record_error("schedule"),
                ReconcilerError::CapiError(_) => record_error("capi"),
                ReconcilerError::FileResolutionError(_) => record_error("file_resolution"),
                ReconcilerError::ReferenceValidationError(_) => {
                    record_error("reference_validation");
                }
                ReconcilerError::ValidationError(_) => record_error("validation"),
                ReconcilerError::TimeoutError(_) => record_error("timeout"),
                ReconcilerError::Other(_) => record_error("other"),
            }
        }
    }
}

/// Core reconciliation logic executed after finalizer and deletion guards pass.
///
/// Evaluates the kill switch, the schedule, and the current phase, then
/// dispatches to the appropriate phase handler.  Each phase handler is
/// responsible for a single state transition and must return an [`Action`]
/// indicating when the controller should next wake up.
///
/// # Errors
/// Propagates any error returned by a phase handler or by schedule evaluation.
async fn reconcile_inner(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Get current status
    let current_phase = resource
        .status
        .as_ref()
        .map(|s| s.phase.clone())
        .unwrap_or_default();

    debug!(
        resource = %name,
        namespace = %namespace,
        phase = ?current_phase,
        "Current phase"
    );

    // Check kill switch first
    if resource.spec.kill_switch {
        info!(
            resource = %name,
            namespace = %namespace,
            "Kill switch activated - removing machine immediately"
        );
        KILL_SWITCH_ACTIVATIONS_TOTAL.inc();
        return handle_kill_switch(resource, ctx).await;
    }

    // Evaluate schedule
    let should_be_active = evaluate_schedule(&resource.spec.schedule, None)?;

    // Record schedule evaluation metric
    record_schedule_evaluation(should_be_active);

    debug!(
        resource = %name,
        namespace = %namespace,
        should_be_active = should_be_active,
        enabled = resource.spec.schedule.enabled,
        "Schedule evaluation"
    );

    // Handle state transitions based on current phase and schedule
    let current_phase_str = current_phase.as_deref().unwrap_or(PHASE_PENDING);

    match current_phase_str {
        PHASE_ACTIVE => handle_active_phase(resource, ctx, should_be_active).await,
        PHASE_SHUTTING_DOWN => handle_shutting_down_phase(resource, ctx).await,
        PHASE_INACTIVE => handle_inactive_phase(resource, ctx, should_be_active).await,
        PHASE_DISABLED => handle_disabled_phase(resource, ctx, should_be_active).await,
        PHASE_TERMINATED => handle_terminated_phase(resource, ctx).await,
        PHASE_ERROR => handle_error_phase(resource, ctx).await,
        // PHASE_PENDING and unknown phases handled by default
        _ => handle_pending_phase(resource, ctx, should_be_active).await,
    }
}

// ============================================================================
// Phase-specific handlers (CAPI-based)
// ============================================================================

/// Handle the `Pending` phase — initial state for new or recovering resources.
///
/// Transitions:
/// - Schedule disabled → `Disabled`
/// - Outside schedule window → `Inactive`
/// - Inside schedule window → creates CAPI resources, then `Active`
/// - CAPI creation failure → `Error` (retried after [`ERROR_REQUEUE_SECS`])
async fn handle_pending_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Guard clause: if schedule is disabled
    if !resource.spec.schedule.enabled {
        info!(resource = %name, namespace = %namespace, "Schedule disabled");
        update_phase(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_PENDING),
            PHASE_DISABLED,
            Some(REASON_SCHEDULE_DISABLED),
            Some("Schedule is disabled"),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Guard clause: if outside schedule
    if !should_be_active {
        info!(resource = %name, namespace = %namespace, "Outside schedule window");
        update_phase(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_PENDING),
            PHASE_INACTIVE,
            Some(REASON_SCHEDULE_INACTIVE),
            Some("Outside scheduled time window"),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Within schedule - proceed to create machine
    info!(resource = %name, namespace = %namespace, "Within schedule - creating machine");

    // Create CAPI resources (bootstrap, infrastructure, machine) from inline specs
    if let Err(e) = add_machine_to_cluster(&resource, &ctx.client, &namespace).await {
        error!(resource = %name, error = %e, "Failed to create CAPI Machine");
        update_phase(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_PENDING),
            PHASE_ERROR,
            Some("MachineCreationFailed"),
            Some(&format!("Failed to create CAPI Machine: {e}")),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS)));
    }

    // Machine created successfully - transition to Active
    update_phase(
        &ctx,
        &namespace,
        &name,
        Some(PHASE_PENDING),
        PHASE_ACTIVE,
        Some(REASON_MACHINE_CREATED),
        Some("CAPI Machine created successfully"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle the `Active` phase — machine is provisioned and part of the cluster.
///
/// Transitions:
/// - Schedule disabled → `ShuttingDown` (grace period starts)
/// - Outside schedule window → `ShuttingDown` (grace period starts)
/// - Still in schedule → no-op, requeue after [`TIMER_REQUEUE_SECS`]
async fn handle_active_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Guard clause: schedule disabled
    if !resource.spec.schedule.enabled {
        info!(resource = %name, namespace = %namespace, "Schedule disabled - initiating shutdown");
        update_phase_with_grace_period(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_ACTIVE),
            PHASE_SHUTTING_DOWN,
            Some(REASON_SCHEDULE_DISABLED),
            Some("Schedule disabled - starting graceful shutdown"),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Guard clause: outside schedule
    if !should_be_active {
        info!(resource = %name, namespace = %namespace, "Outside schedule - initiating shutdown");
        update_phase_with_grace_period(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_ACTIVE),
            PHASE_SHUTTING_DOWN,
            Some(REASON_GRACE_PERIOD),
            Some("Outside schedule - starting graceful shutdown"),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Happy path: machine active and in schedule
    debug!(resource = %name, namespace = %namespace, "Machine active and in schedule");
    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle the `ShuttingDown` phase — graceful shutdown with node drain.
///
/// On each reconciliation tick, checks whether the grace period
/// (configured by `spec.gracefulShutdownTimeout`) has elapsed.
///
/// If elapsed:
/// 1. Resolves the Kubernetes `Node` via the CAPI Machine's `nodeRef`
/// 2. Drains the node (cordon + pod eviction) up to `spec.nodeDrainTimeout`
/// 3. Deletes the CAPI `Machine` resource
/// 4. Transitions to `Inactive`
///
/// Drain failures are logged but do **not** block machine deletion, preventing
/// the controller from getting stuck if drain is unrecoverable.
///
/// If the grace period has not yet elapsed, the handler simply requeues.
async fn handle_shutting_down_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Check if grace period elapsed
    let grace_period_elapsed = check_grace_period_elapsed(&resource)?;

    if grace_period_elapsed {
        info!(resource = %name, namespace = %namespace, "Grace period elapsed - draining node and removing machine");

        // Step 1: Drain the node if it exists
        let machine_name = format!("{name}-machine");
        if let Some(node_name) =
            get_node_from_machine(&ctx.client, &namespace, &machine_name).await?
        {
            info!(
                resource = %name,
                namespace = %namespace,
                node = %node_name,
                "Node found - initiating drain"
            );

            // Parse drain timeout
            let drain_timeout = parse_duration(&resource.spec.node_drain_timeout)?;

            // Attempt to drain the node
            match drain_node_with_timeout(&ctx.client, &node_name, drain_timeout).await {
                Ok(()) => {
                    info!(
                        resource = %name,
                        node = %node_name,
                        "Node drained successfully"
                    );
                }
                Err(e) => {
                    error!(
                        resource = %name,
                        node = %node_name,
                        error = %e,
                        "Node drain failed - proceeding with machine deletion anyway"
                    );
                    // Continue with deletion even if drain fails
                    // This ensures we don't get stuck if drain has issues
                }
            }
        } else {
            debug!(
                resource = %name,
                namespace = %namespace,
                "No node found for machine - skipping drain"
            );
        }

        // Step 2: Delete CAPI Machine
        if let Err(e) = remove_machine_from_cluster(&resource, &ctx.client, &namespace).await {
            error!(resource = %name, error = %e, "Failed to delete CAPI Machine");
            update_phase(
                &ctx,
                &namespace,
                &name,
                Some(PHASE_SHUTTING_DOWN),
                PHASE_ERROR,
                Some("MachineDeletionFailed"),
                Some(&format!("Failed to delete CAPI Machine: {e}")),
            )
            .await?;
            return Ok(Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS)));
        }

        // Machine removed successfully - transition to Inactive
        update_phase(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_SHUTTING_DOWN),
            PHASE_INACTIVE,
            Some(REASON_MACHINE_DELETED),
            Some("Machine removed from cluster"),
        )
        .await?;

        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Still in grace period
    debug!(resource = %name, namespace = %namespace, "Grace period active");
    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle the `Inactive` phase — machine removed, waiting for the next active window.
///
/// Transitions:
/// - Schedule disabled → no-op, requeue
/// - Still outside schedule window → no-op, requeue
/// - Schedule window becomes active → `Pending` (triggers machine recreation)
async fn handle_inactive_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Guard clause: schedule disabled
    if !resource.spec.schedule.enabled {
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Guard clause: still outside schedule
    if !should_be_active {
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Schedule became active - transition to Pending to recreate machine
    info!(resource = %name, namespace = %namespace, "Schedule active - recreating machine");
    update_phase(
        &ctx,
        &namespace,
        &name,
        Some(PHASE_INACTIVE),
        PHASE_PENDING,
        Some(REASON_SCHEDULE_ACTIVE),
        Some("Schedule became active - initiating machine creation"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle the `Disabled` phase — `spec.schedule.enabled` is `false`.
///
/// Transitions:
/// - Schedule re-enabled → `Pending` (machine will be recreated if in window)
/// - Still disabled → no-op, requeue
///
/// Note: disabling the schedule does **not** remove an already-active machine.
/// The machine will only be removed once the `Active → ShuttingDown` transition
/// is triggered by the schedule being disabled while the machine is running.
async fn handle_disabled_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    _should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Guard clause: schedule enabled again
    if resource.spec.schedule.enabled {
        info!(resource = %name, namespace = %namespace, "Schedule re-enabled");
        update_phase(
            &ctx,
            &namespace,
            &name,
            Some(PHASE_DISABLED),
            PHASE_PENDING,
            Some(REASON_SCHEDULE_ACTIVE),
            Some("Schedule re-enabled"),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Still disabled
    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle the `Terminated` phase — terminal state after kill-switch activation.
///
/// This is a **terminal state**: no further transitions occur.  The resource
/// continues to exist in the cluster but the controller will not take any
/// action other than periodic requeueing.  The phase can only be cleared by
/// deleting the `ScheduledMachine` resource.
#[allow(clippy::unused_async)]
async fn handle_terminated_phase(
    _resource: Arc<ScheduledMachine>,
    _ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    // Terminal state - no further action needed
    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle the `Error` phase — attempt automatic recovery.
///
/// Resets the phase to `Pending`, which causes the controller to re-evaluate
/// the schedule and attempt to reconcile from a clean state.  Uses
/// [`ERROR_REQUEUE_SECS`] for the requeue interval to avoid tight retry loops.
async fn handle_error_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Log error and retry from Pending
    error!(resource = %name, namespace = %namespace, "In Error phase - attempting recovery");

    update_phase(
        &ctx,
        &namespace,
        &name,
        Some(PHASE_ERROR),
        PHASE_PENDING,
        Some("RetryingReconciliation"),
        Some("Attempting recovery from error"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS)))
}

// Re-export helper functions for use in this module
use super::helpers::{
    add_finalizer, add_machine_to_cluster, check_grace_period_elapsed, drain_node_with_timeout,
    evaluate_schedule, get_node_from_machine, handle_deletion, handle_kill_switch, has_finalizer,
    parse_duration, remove_machine_from_cluster, should_process_resource, update_phase,
    update_phase_with_grace_period,
};

#[cfg(test)]
#[path = "scheduled_machine_tests.rs"]
mod tests;
