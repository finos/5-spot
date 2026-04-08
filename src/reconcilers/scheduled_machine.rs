// Reconciliation logic for ScheduledMachine resources

use std::sync::Arc;
use std::time::{Duration, Instant};

use kube::{runtime::controller::Action, Client, Resource, ResourceExt};
use tracing::{debug, error, info};

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

#[derive(Clone)]
pub struct Context {
    pub client: Client,
    pub instance_id: u32,
    pub instance_count: u32,
}

impl Context {
    #[must_use]
    pub fn new(client: Client, instance_id: u32, instance_count: u32) -> Self {
        Self {
            client,
            instance_id,
            instance_count,
        }
    }
}

// ============================================================================
// Error types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ReconcilerError {
    #[error("Kubernetes API error: {0}")]
    KubeError(#[from] kube::Error),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Schedule evaluation error: {0}")]
    ScheduleError(String),

    #[error("CAPI operation failed: {0}")]
    CapiError(String),

    #[error("File content resolution failed: {0}")]
    FileResolutionError(String),

    #[error("Reference validation failed: {0}")]
    ReferenceValidationError(String),

    #[error("Security validation failed: {0}")]
    ValidationError(String),

    #[error("Operation timed out: {0}")]
    TimeoutError(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// ============================================================================
// Main reconciliation logic
// ============================================================================

/// Main reconciliation entry point with finalizer handling
///
/// # Errors
/// Returns error if schedule evaluation, k8s API calls, or machine lifecycle operations fail
pub async fn reconcile_scheduled_machine(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let start_time = Instant::now();
    let namespace = resource.namespace().ok_or_else(|| {
        record_error("invalid_config");
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Get current phase for metrics (clone to avoid borrow issues)
    let current_phase = resource
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| PHASE_PENDING.to_string());

    info!(
        resource = %name,
        namespace = %namespace,
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
    let result = reconcile_inner(resource, ctx).await;
    record_reconciliation_result(&result, &current_phase, start_time.elapsed());
    result
}

/// Record reconciliation result metrics
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

/// Inner reconciliation logic
async fn reconcile_inner(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
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

/// Handle Pending phase - initial state, evaluate schedule
async fn handle_pending_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    // Guard clause: if schedule is disabled
    if !resource.spec.schedule.enabled {
        info!(resource = %name, namespace = %namespace, "Schedule disabled");
        update_phase(
            &ctx.client,
            &namespace,
            &name,
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
            &ctx.client,
            &namespace,
            &name,
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
            &ctx.client,
            &namespace,
            &name,
            PHASE_ERROR,
            Some("MachineCreationFailed"),
            Some(&format!("Failed to create CAPI Machine: {e}")),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS)));
    }

    // Machine created successfully - transition to Active
    update_phase(
        &ctx.client,
        &namespace,
        &name,
        PHASE_ACTIVE,
        Some(REASON_MACHINE_CREATED),
        Some("CAPI Machine created successfully"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle Active phase - machine is running
async fn handle_active_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    // Guard clause: schedule disabled
    if !resource.spec.schedule.enabled {
        info!(resource = %name, namespace = %namespace, "Schedule disabled - initiating shutdown");
        update_phase_with_grace_period(
            &ctx.client,
            &namespace,
            &name,
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
            &ctx.client,
            &namespace,
            &name,
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

/// Handle `ShuttingDown` phase - graceful machine shutdown
async fn handle_shutting_down_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
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
                &ctx.client,
                &namespace,
                &name,
                PHASE_ERROR,
                Some("MachineDeletionFailed"),
                Some(&format!("Failed to delete CAPI Machine: {e}")),
            )
            .await?;
            return Ok(Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS)));
        }

        // Machine removed successfully - transition to Inactive
        update_phase(
            &ctx.client,
            &namespace,
            &name,
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

/// Handle Inactive phase - machine removed, waiting for schedule
async fn handle_inactive_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
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
        &ctx.client,
        &namespace,
        &name,
        PHASE_PENDING,
        Some(REASON_SCHEDULE_ACTIVE),
        Some("Schedule became active - initiating machine creation"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle Disabled phase - schedule is disabled
async fn handle_disabled_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    _should_be_active: bool,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    // Guard clause: schedule enabled again
    if resource.spec.schedule.enabled {
        info!(resource = %name, namespace = %namespace, "Schedule re-enabled");
        update_phase(
            &ctx.client,
            &namespace,
            &name,
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

/// Handle Terminated phase - kill switch activated
#[allow(clippy::unused_async)]
async fn handle_terminated_phase(
    _resource: Arc<ScheduledMachine>,
    _ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    // Terminal state - no further action needed
    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Handle Error phase - attempt recovery
async fn handle_error_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    // Log error and retry from Pending
    error!(resource = %name, namespace = %namespace, "In Error phase - attempting recovery");

    update_phase(
        &ctx.client,
        &namespace,
        &name,
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
