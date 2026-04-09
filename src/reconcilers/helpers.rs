// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
//! # Reconciliation helper functions
//!
//! Pure utility functions used by the [`scheduled_machine`](super::scheduled_machine)
//! reconciler.  Separated here to keep the main reconciler focused on the
//! state-machine logic.
//!
//! ## Organisation
//! - **Resource distribution** — consistent hashing for multi-instance deployments
//! - **Schedule evaluation** — timezone-aware day/hour range and cron matching
//! - **Finalizer management** — add, check, and remove the 5-spot finalizer
//! - **Kill switch** — immediate machine removal path
//! - **Grace period** — elapsed-time check against the shutdown timeout
//! - **Duration parsing** — bounded `"5m"` / `"10s"` / `"1h"` string parser
//! - **Kubernetes event helpers** — phase-transition event construction
//! - **Status update helpers** — `patch_status` wrappers that also record events
//! - **Security validation** — label prefix rejection and API group allowlist
//! - **CAPI resource creation / deletion** — bootstrap, infra, and Machine lifecycle
//! - **Node draining** — cordon + pod eviction with timeout
//! - **Error policy** — controller requeue-on-error strategy

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use k8s_openapi::api::core::v1::ObjectReference;
use kube::{
    api::{Api, Patch, PatchParams},
    runtime::{
        controller::Action,
        events::{Event as KubeEvent, EventType},
    },
    Client, Resource, ResourceExt,
};
use serde_json::json;
use tracing::{debug, error, info, warn};

use super::{Context, ReconcilerError};
use crate::constants::{
    ALLOWED_BOOTSTRAP_API_GROUPS, ALLOWED_INFRASTRUCTURE_API_GROUPS, API_VERSION_FULL,
    CAPI_CLUSTER_NAME_LABEL, CAPI_GROUP, CAPI_MACHINE_API_VERSION, CAPI_MACHINE_API_VERSION_FULL,
    CAPI_RESOURCE_MACHINES, CONDITION_STATUS_TRUE, CONDITION_TYPE_READY, DEFAULT_INSTANCE_ID,
    ENV_OPERATOR_INSTANCE_ID, ERROR_REQUEUE_SECS, FINALIZER_CLEANUP_TIMEOUT_SECS,
    FINALIZER_SCHEDULED_MACHINE, MAX_BACKOFF_SECS, MAX_DURATION_SECS, MAX_RECONCILE_RETRIES,
    PHASE_ACTIVE, PHASE_ERROR, PHASE_INACTIVE, PHASE_SHUTTING_DOWN, PHASE_TERMINATED,
    POD_EVICTION_GRACE_PERIOD_SECS, REASON_GRACE_PERIOD, REASON_KILL_SWITCH,
    REASON_RECONCILE_SUCCEEDED, RESERVED_LABEL_PREFIXES, TIMER_REQUEUE_SECS,
};
use crate::crd::{Condition, ScheduledMachine, ScheduledMachineStatus};
use crate::metrics::{record_node_drain, record_pod_eviction};

// ============================================================================
// Resource processing and consistent hashing
// ============================================================================

/// Determine if this operator instance should process a specific resource
/// Uses consistent hashing to distribute resources across instances
pub fn should_process_resource(
    name: &str,
    namespace: &str,
    priority: u8,
    instance_count: u32,
) -> bool {
    if instance_count <= 1 {
        return true;
    }

    // Create consistent hash of resource identifier with priority influence
    let resource_id = format!("{namespace}/{name}");
    let priority_modifier = u64::from(priority) * 1000;

    // Simple hash function (in production, consider using a proper hash)
    let mut hash: u64 = 0;
    for byte in resource_id.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    hash = hash.wrapping_add(priority_modifier);

    #[allow(clippy::cast_possible_truncation)]
    let assigned_instance = (hash % u64::from(instance_count)) as u32;
    let current_instance = std::env::var(ENV_OPERATOR_INSTANCE_ID)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_INSTANCE_ID);

    debug!(
        resource = %resource_id,
        priority = priority,
        assigned_instance = assigned_instance,
        current_instance = current_instance,
        "Resource assignment check"
    );

    assigned_instance == current_instance
}

// ============================================================================
// Schedule evaluation
// ============================================================================

/// Evaluate if a machine should be active based on schedule
///
/// # Errors
/// Returns error if timezone is invalid or weekday/hour parsing fails
pub fn evaluate_schedule(
    schedule: &crate::crd::ScheduleSpec,
    check_time: Option<DateTime<Utc>>,
) -> Result<bool, ReconcilerError> {
    if !schedule.enabled {
        return Ok(false);
    }

    // If using cron expression, evaluate it
    if let Some(cron_expr) = &schedule.cron {
        // TODO: Implement cron expression evaluation
        // For now, return an error indicating cron is not yet supported
        return Err(ReconcilerError::ScheduleError(format!(
            "Cron expression evaluation not yet implemented: {cron_expr}"
        )));
    }

    let now = check_time.unwrap_or_else(Utc::now);

    // Parse timezone
    let tz: Tz = schedule.timezone.parse().map_err(|_| {
        ReconcilerError::ScheduleError(format!("Invalid timezone: {}", schedule.timezone))
    })?;

    let current_time = now.with_timezone(&tz);

    // Check weekday (Monday = 0, Sunday = 6)
    #[allow(clippy::cast_possible_truncation)]
    let current_weekday = current_time.weekday().num_days_from_monday() as u8;
    let allowed_weekdays = schedule
        .get_active_weekdays()
        .map_err(|e| ReconcilerError::ScheduleError(format!("Failed to parse weekdays: {e}")))?
        .ok_or_else(|| ReconcilerError::ScheduleError("No weekday schedule defined".to_string()))?;

    debug!(
        current_weekday = current_weekday,
        allowed_weekdays = ?allowed_weekdays,
        "Weekday check"
    );

    if !allowed_weekdays.contains(&current_weekday) {
        return Ok(false);
    }

    // Check hour
    #[allow(clippy::cast_possible_truncation)]
    let current_hour = current_time.hour() as u8;
    let allowed_hours = schedule
        .get_active_hours()
        .map_err(|e| ReconcilerError::ScheduleError(format!("Failed to parse hours: {e}")))?
        .ok_or_else(|| ReconcilerError::ScheduleError("No hour schedule defined".to_string()))?;

    debug!(
        current_hour = current_hour,
        allowed_hours = ?allowed_hours,
        "Hour check"
    );

    Ok(allowed_hours.contains(&current_hour))
}

// ============================================================================
// ============================================================================
// Finalizer management
// ============================================================================

/// Return `true` if the resource already carries the 5-spot finalizer.
///
/// Used as a guard in [`reconcile_scheduled_machine`] to avoid adding the
/// finalizer a second time when the resource has already been processed.
pub fn has_finalizer(resource: &ScheduledMachine) -> bool {
    resource
        .meta()
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&FINALIZER_SCHEDULED_MACHINE.to_string()))
}

/// Add the 5-spot finalizer to a `ScheduledMachine` resource.
///
/// The finalizer prevents Kubernetes from deleting the resource until the
/// controller has successfully run cleanup logic (see [`handle_deletion`]).
/// After patching the finalizer, the function requeues immediately
/// (`Duration::from_secs(0)`) so the main reconcile loop can proceed to the
/// `Pending` phase in the same reconciliation cycle.
///
/// # Errors
/// Returns [`ReconcilerError::InvalidConfig`] if the resource has no namespace,
/// or a kube API error if the merge-patch call fails.
pub async fn add_finalizer(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    info!(
        resource = %name,
        namespace = %namespace,
        "Adding finalizer"
    );

    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), &namespace);

    let mut finalizers = resource.meta().finalizers.clone().unwrap_or_default();
    finalizers.push(FINALIZER_SCHEDULED_MACHINE.to_string());

    let patch = json!({
        "metadata": {
            "finalizers": finalizers
        }
    });

    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(Action::requeue(Duration::from_secs(0)))
}

/// Run finalizer cleanup when a `ScheduledMachine` is being deleted.
///
/// If the resource is currently in the `Active` or `ShuttingDown` phase the
/// corresponding CAPI Machine (and its child resources) are removed from the
/// cluster first.  The removal is wrapped in a hard
/// [`FINALIZER_CLEANUP_TIMEOUT_SECS`] timeout so a hung API call cannot
/// block namespace deletion or cluster upgrades indefinitely.
///
/// Once cleanup succeeds (or is skipped for non-running phases) the
/// finalizer string is removed from `metadata.finalizers`.  After this patch
/// Kubernetes considers the resource fully deleted.
///
/// # Errors
/// - [`ReconcilerError::InvalidConfig`] — resource has no namespace
/// - [`ReconcilerError::TimeoutError`] — machine removal exceeded the cleanup timeout
/// - [`ReconcilerError::CapiError`] — CAPI Machine delete call failed
/// - kube API error — finalizer patch call failed
pub async fn handle_deletion(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    info!(
        resource = %name,
        namespace = %namespace,
        "Handling deletion"
    );

    // Wrap machine removal in a hard timeout so a hung removal cannot block
    // namespace deletion or cluster upgrades indefinitely.
    let cleanup_timeout = Duration::from_secs(FINALIZER_CLEANUP_TIMEOUT_SECS);
    let current_phase = resource.status.as_ref().and_then(|s| s.phase.as_deref());

    if let Some(phase) = current_phase {
        if matches!(phase, PHASE_ACTIVE | PHASE_SHUTTING_DOWN) {
            info!(
                resource = %name,
                namespace = %namespace,
                timeout_secs = FINALIZER_CLEANUP_TIMEOUT_SECS,
                "Removing machine from cluster before deletion"
            );

            tokio::time::timeout(
                cleanup_timeout,
                remove_machine_from_cluster(&resource, &ctx.client, &namespace),
            )
            .await
            .map_err(|_| {
                ReconcilerError::TimeoutError(format!(
                    "Finalizer cleanup timed out after {FINALIZER_CLEANUP_TIMEOUT_SECS}s for {name}"
                ))
            })??;
        }
    }

    // Remove finalizer
    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), &namespace);

    let mut finalizers = resource.meta().finalizers.clone().unwrap_or_default();
    finalizers.retain(|f| f != FINALIZER_SCHEDULED_MACHINE);

    let patch = json!({
        "metadata": {
            "finalizers": finalizers
        }
    });

    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    info!(
        resource = %name,
        namespace = %namespace,
        "Finalizer removed, resource will be deleted"
    );

    Ok(Action::await_change())
}

// ============================================================================
// Kill switch handling
// ============================================================================

/// Execute an emergency kill-switch that immediately removes the machine from
/// its cluster and transitions it to the `Terminated` phase.
///
/// The kill switch is an operator-level escape hatch for situations where the
/// normal graceful shutdown period must be bypassed (e.g., cost runaway,
/// security incident).  Unlike the ordinary shutdown path, no grace period is
/// observed — removal happens synchronously in the reconcile loop.
///
/// The machine is only removed when its current phase is `Active` or
/// `ShuttingDown`.  Resources already in `Inactive` or `Terminated` are left
/// untouched; the function simply records the `Terminated` phase to ensure
/// the status is up to date.
///
/// # Errors
/// - [`ReconcilerError::InvalidConfig`] — resource has no namespace
/// - [`ReconcilerError::CapiError`] — CAPI Machine delete call failed
/// - kube API error — status patch call failed
pub async fn handle_kill_switch(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    let current_phase = resource.status.as_ref().and_then(|s| s.phase.as_deref());

    // Only remove if not already inactive or terminated
    if let Some(phase) = current_phase {
        if phase != PHASE_INACTIVE
            && phase != PHASE_TERMINATED
            && matches!(phase, PHASE_ACTIVE | PHASE_SHUTTING_DOWN)
        {
            info!(
                resource = %name,
                namespace = %namespace,
                "Kill switch active - removing machine immediately"
            );

            remove_machine_from_cluster(&resource, &ctx.client, &namespace).await?;
        }
    }

    let from_phase = resource.status.as_ref().and_then(|s| s.phase.as_deref());
    update_phase(
        &ctx,
        &namespace,
        &name,
        from_phase,
        PHASE_TERMINATED,
        Some(REASON_KILL_SWITCH),
        Some("Machine terminated due to kill switch"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

// ============================================================================
// Grace period management
// ============================================================================

/// Return `true` when the graceful shutdown timeout has been exceeded.
///
/// The grace period is tracked via the `last_transition_time` field of the
/// status condition whose `reason` equals [`REASON_GRACE_PERIOD`].  That
/// timestamp is written by [`update_phase_with_grace_period`] when the
/// machine first enters the `ShuttingDown` phase.
///
/// If no such condition is found (e.g., the resource was transitioned to
/// `ShuttingDown` by an older controller version that did not record the
/// condition), the function conservatively returns `true` so the drain
/// proceeds without getting stuck.
///
/// # Errors
/// - [`ReconcilerError::InvalidConfig`] — resource has no `.status` sub-resource
///   or the recorded timestamp is not valid RFC-3339
pub fn check_grace_period_elapsed(resource: &ScheduledMachine) -> Result<bool, ReconcilerError> {
    let status = resource
        .status
        .as_ref()
        .ok_or_else(|| ReconcilerError::InvalidConfig("Resource has no status".to_string()))?;

    // Check for grace period start time in conditions
    let grace_start_str = status
        .conditions
        .iter()
        .find(|c| c.reason == REASON_GRACE_PERIOD)
        .map(|c| c.last_transition_time.as_str());

    if let Some(start_time_str) = grace_start_str {
        // Parse RFC3339 timestamp
        let start_time = DateTime::parse_from_rfc3339(start_time_str)
            .map_err(|e| ReconcilerError::InvalidConfig(format!("Invalid timestamp: {e}")))?
            .with_timezone(&Utc);

        let timeout = parse_duration(&resource.spec.graceful_shutdown_timeout)?;
        let now = Utc::now();
        let elapsed = now.signed_duration_since(start_time);

        debug!(
            grace_start = %start_time,
            elapsed_secs = elapsed.num_seconds(),
            timeout_secs = timeout.as_secs(),
            "Grace period check"
        );

        #[allow(clippy::cast_possible_wrap)]
        Ok(elapsed.num_seconds() >= timeout.as_secs() as i64)
    } else {
        // No grace period started yet
        Ok(true)
    }
}

/// Parse duration string (e.g., "5m", "10s", "1h")
///
/// # Errors
/// Returns error on empty input, invalid format, integer overflow, or values exceeding 24 hours.
pub fn parse_duration(duration_str: &str) -> Result<Duration, ReconcilerError> {
    let duration_str = duration_str.trim();

    if duration_str.is_empty() {
        return Err(ReconcilerError::InvalidConfig(
            "Empty duration string".to_string(),
        ));
    }

    let (value_str, unit) = duration_str.split_at(duration_str.len() - 1);
    let value: u64 = value_str.parse().map_err(|_| {
        ReconcilerError::InvalidConfig(format!("Invalid duration value: {duration_str}"))
    })?;

    let multiplier: u64 = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => {
            return Err(ReconcilerError::InvalidConfig(format!(
                "Invalid duration unit: '{unit}'. Use 's', 'm', or 'h'"
            )))
        }
    };

    let seconds = value.checked_mul(multiplier).ok_or_else(|| {
        ReconcilerError::InvalidConfig(format!(
            "Duration overflow: '{duration_str}' exceeds representable range"
        ))
    })?;

    if seconds > MAX_DURATION_SECS {
        return Err(ReconcilerError::InvalidConfig(format!(
            "Duration {seconds}s exceeds maximum of {MAX_DURATION_SECS}s (24h)"
        )));
    }

    Ok(Duration::from_secs(seconds))
}

// ============================================================================
// Kubernetes Event helpers
// ============================================================================

/// Build a Kubernetes Event for a phase transition.
///
/// Returns `EventType::Warning` for `Error` and `Terminated` phases (actionable
/// operator alert); `EventType::Normal` for all other transitions.
pub fn build_phase_transition_event(
    from_phase: Option<&str>,
    to_phase: &str,
    reason: &str,
    message: &str,
) -> KubeEvent {
    let event_type = if to_phase == PHASE_ERROR || to_phase == PHASE_TERMINATED {
        EventType::Warning
    } else {
        EventType::Normal
    };
    KubeEvent {
        type_: event_type,
        reason: reason.to_string(),
        note: Some(format!(
            "{} -> {}: {}",
            from_phase.unwrap_or("Unknown"),
            to_phase,
            message
        )),
        action: format!("PhaseTransitionTo{to_phase}"),
        secondary: None,
    }
}

// ============================================================================
// Status update helpers
// ============================================================================

/// Update phase and status condition, recording an immutable Kubernetes Event
/// for audit trail (SOX §404, NIST AU-2/AU-3).
///
/// The `from_phase` parameter captures the previous phase for before/after logging.
/// Event recording is best-effort — a failure to publish the event is logged as a
/// warning but does not abort the phase transition.
pub async fn update_phase(
    ctx: &Context,
    namespace: &str,
    name: &str,
    from_phase: Option<&str>,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
) -> Result<(), ReconcilerError> {
    let resolved_reason = reason.unwrap_or(REASON_RECONCILE_SUCCEEDED);
    let resolved_message = message.unwrap_or("Phase transition completed");

    info!(
        resource = %name,
        namespace = %namespace,
        from = from_phase.unwrap_or("Unknown"),
        to = %phase,
        reason = %resolved_reason,
        "Phase transition"
    );

    // Record an immutable Kubernetes Event for the audit trail
    let object_ref = ObjectReference {
        api_version: Some(crate::constants::API_VERSION_FULL.to_string()),
        kind: Some(crate::constants::KIND_SCHEDULED_MACHINE.to_string()),
        name: Some(name.to_string()),
        namespace: Some(namespace.to_string()),
        ..Default::default()
    };
    let event = build_phase_transition_event(from_phase, phase, resolved_reason, resolved_message);
    if let Err(e) = ctx.recorder.publish(&event, &object_ref).await {
        warn!(
            resource = %name,
            namespace = %namespace,
            error = %e,
            "Failed to record phase transition event (audit trail incomplete)"
        );
    }

    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), namespace);

    let condition = Condition::new(
        CONDITION_TYPE_READY,
        CONDITION_STATUS_TRUE,
        resolved_reason,
        resolved_message,
    );

    let status = ScheduledMachineStatus {
        phase: Some(phase.to_string()),
        message: Some(resolved_message.to_string()),
        conditions: vec![condition],
        ..Default::default()
    };

    let patch = json!({
        "status": status
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(())
}

/// Patch the status with the current time as `lastScheduledTime`, recording
/// an immutable audit event.
///
/// Used when the machine transitions to `Active` after being provisioned on
/// schedule.  The `lastScheduledTime` field lets operators audit when the
/// last scheduled window began.
///
/// # Errors
/// Same as [`update_phase`].
#[allow(dead_code)] // TODO: Use this when machine creation is implemented
pub async fn update_phase_with_last_schedule(
    ctx: &Context,
    namespace: &str,
    name: &str,
    from_phase: Option<&str>,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
) -> Result<(), ReconcilerError> {
    let resolved_reason = reason.unwrap_or(REASON_RECONCILE_SUCCEEDED);
    let resolved_message = message.unwrap_or("Phase transition completed");

    info!(
        resource = %name,
        namespace = %namespace,
        from = from_phase.unwrap_or("Unknown"),
        to = %phase,
        reason = %resolved_reason,
        "Phase transition"
    );

    let object_ref = ObjectReference {
        api_version: Some(crate::constants::API_VERSION_FULL.to_string()),
        kind: Some(crate::constants::KIND_SCHEDULED_MACHINE.to_string()),
        name: Some(name.to_string()),
        namespace: Some(namespace.to_string()),
        ..Default::default()
    };
    let event = build_phase_transition_event(from_phase, phase, resolved_reason, resolved_message);
    if let Err(e) = ctx.recorder.publish(&event, &object_ref).await {
        warn!(resource = %name, error = %e, "Failed to record phase transition event");
    }

    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), namespace);

    let condition = Condition::new(
        CONDITION_TYPE_READY,
        CONDITION_STATUS_TRUE,
        resolved_reason,
        resolved_message,
    );

    let status = ScheduledMachineStatus {
        phase: Some(phase.to_string()),
        message: Some(resolved_message.to_string()),
        conditions: vec![condition],
        last_scheduled_time: Some(Utc::now().to_rfc3339()),
        ..Default::default()
    };

    let patch = json!({
        "status": status
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(())
}

/// Patch the status to `ShuttingDown` and stamp the current time into the
/// `last_transition_time` of the grace-period condition.
///
/// This timestamp is later read by [`check_grace_period_elapsed`] to decide
/// when the drain window has closed.  Recording it here — rather than
/// computing elapsed time from an external clock — makes the grace period
/// robust to controller restarts.
///
/// # Errors
/// Same as [`update_phase`].
pub async fn update_phase_with_grace_period(
    ctx: &Context,
    namespace: &str,
    name: &str,
    from_phase: Option<&str>,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
) -> Result<(), ReconcilerError> {
    let resolved_reason = reason.unwrap_or(REASON_GRACE_PERIOD);
    let resolved_message = message.unwrap_or("Grace period started");

    info!(
        resource = %name,
        namespace = %namespace,
        from = from_phase.unwrap_or("Unknown"),
        to = %phase,
        reason = %resolved_reason,
        "Phase transition"
    );

    let object_ref = ObjectReference {
        api_version: Some(crate::constants::API_VERSION_FULL.to_string()),
        kind: Some(crate::constants::KIND_SCHEDULED_MACHINE.to_string()),
        name: Some(name.to_string()),
        namespace: Some(namespace.to_string()),
        ..Default::default()
    };
    let event = build_phase_transition_event(from_phase, phase, resolved_reason, resolved_message);
    if let Err(e) = ctx.recorder.publish(&event, &object_ref).await {
        warn!(resource = %name, error = %e, "Failed to record phase transition event");
    }

    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), namespace);

    let condition = Condition::new(
        CONDITION_TYPE_READY,
        CONDITION_STATUS_TRUE,
        resolved_reason,
        resolved_message,
    );

    let status = ScheduledMachineStatus {
        phase: Some(phase.to_string()),
        message: Some(resolved_message.to_string()),
        conditions: vec![condition],
        ..Default::default()
    };

    let patch = json!({
        "status": status
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(())
}

// ============================================================================
// Security validation helpers
// ============================================================================

/// Reject label/annotation maps that contain reserved key prefixes.
///
/// Users must not be able to override system labels such as
/// `cluster.x-k8s.io/cluster-name` or `kubernetes.io/*` via `machineTemplate`.
pub fn validate_labels(
    labels: &BTreeMap<String, String>,
    field: &str,
) -> Result<(), ReconcilerError> {
    for key in labels.keys() {
        for prefix in RESERVED_LABEL_PREFIXES {
            if key.starts_with(prefix) {
                return Err(ReconcilerError::ValidationError(format!(
                    "{field} key '{key}' uses reserved prefix '{prefix}'"
                )));
            }
        }
    }
    Ok(())
}

/// Validate that an apiVersion string belongs to an allowed API group.
///
/// Core Kubernetes API versions (no `/`) are always rejected for CAPI resources.
pub fn validate_api_group(
    api_version: &str,
    allowed_groups: &[&str],
    resource_type: &str,
) -> Result<(), ReconcilerError> {
    let group = api_version.rfind('/').map(|idx| &api_version[..idx]).ok_or_else(|| {
        ReconcilerError::ValidationError(format!(
            "{resource_type} apiVersion '{api_version}' must use a namespaced API group (e.g. 'bootstrap.cluster.x-k8s.io/v1beta1')"
        ))
    })?;

    if !allowed_groups.contains(&group) {
        return Err(ReconcilerError::ValidationError(format!(
            "{resource_type} API group '{group}' is not allowed. Permitted groups: {allowed_groups:?}"
        )));
    }
    Ok(())
}

// ============================================================================
// CAPI Resource Creation
// ============================================================================

/// Derive the Kubernetes name for the bootstrap config resource.
///
/// The name is `<scheduled-machine-name>-bootstrap`, which makes the
/// child resource easy to identify in `kubectl get` output and ties its
/// lifecycle to the parent `ScheduledMachine` via name-based correlation
/// (in addition to `ownerReferences`).
fn bootstrap_resource_name(scheduled_machine_name: &str) -> String {
    format!("{scheduled_machine_name}-bootstrap")
}

/// Derive the Kubernetes name for the infrastructure resource.
///
/// The name is `<scheduled-machine-name>-infra`.  See [`bootstrap_resource_name`]
/// for the naming rationale.
fn infrastructure_resource_name(scheduled_machine_name: &str) -> String {
    format!("{scheduled_machine_name}-infra")
}

/// Derive the Kubernetes name for the CAPI `Machine` resource.
///
/// The name is `<scheduled-machine-name>-machine`.  See [`bootstrap_resource_name`]
/// for the naming rationale.
fn machine_resource_name(scheduled_machine_name: &str) -> String {
    format!("{scheduled_machine_name}-machine")
}

/// Add machine to cluster by creating bootstrap, infrastructure, and Machine resources
///
/// This function:
/// 1. Creates the bootstrap resource from bootstrapSpec
/// 2. Creates the infrastructure resource from infrastructureSpec
/// 3. Creates the CAPI Machine referencing both
#[allow(clippy::too_many_lines)]
pub async fn add_machine_to_cluster(
    resource: &ScheduledMachine,
    client: &Client,
    namespace: &str,
) -> Result<(), ReconcilerError> {
    let name = resource.name_any();
    let cluster_name = &resource.spec.cluster_name;

    let bootstrap_name = bootstrap_resource_name(&name);
    let infra_name = infrastructure_resource_name(&name);
    let machine_name = machine_resource_name(&name);

    info!(
        resource = %name,
        namespace = %namespace,
        cluster = %cluster_name,
        bootstrap = %bootstrap_name,
        infrastructure = %infra_name,
        machine = %machine_name,
        "Creating CAPI resources from inline specs"
    );

    // Validate API groups before creating any resources
    validate_api_group(
        &resource.spec.bootstrap_spec.api_version,
        ALLOWED_BOOTSTRAP_API_GROUPS,
        "bootstrap",
    )?;
    validate_api_group(
        &resource.spec.infrastructure_spec.api_version,
        ALLOWED_INFRASTRUCTURE_API_GROUPS,
        "infrastructure",
    )?;

    // Validate user-supplied labels and annotations do not use reserved prefixes
    if let Some(template) = &resource.spec.machine_template {
        validate_labels(&template.labels, "machineTemplate.labels")?;
        validate_labels(&template.annotations, "machineTemplate.annotations")?;
    }

    let owner_ref = json!({
        "apiVersion": API_VERSION_FULL,
        "kind": "ScheduledMachine",
        "name": name,
        "uid": resource.metadata.uid.as_ref().ok_or_else(|| {
            ReconcilerError::InvalidConfig("Resource UID not set".to_string())
        })?,
        "controller": true,
        "blockOwnerDeletion": true,
    });

    // Bootstrap and infrastructure resources are always created in the same namespace
    // as the ScheduledMachine — cross-namespace resource creation is not permitted.
    let bootstrap_ns = namespace;
    let infra_ns = namespace;

    // 1. Create bootstrap resource
    let bootstrap_spec = &resource.spec.bootstrap_spec;
    let bootstrap_obj = json!({
        "apiVersion": bootstrap_spec.api_version,
        "kind": bootstrap_spec.kind,
        "metadata": {
            "name": bootstrap_name,
            "namespace": bootstrap_ns,
            "ownerReferences": [owner_ref],
        },
        "spec": bootstrap_spec.spec,
    });

    create_dynamic_resource(
        client,
        bootstrap_ns,
        &bootstrap_spec.api_version,
        &bootstrap_spec.kind,
        bootstrap_obj,
    )
    .await
    .map_err(|e| ReconcilerError::CapiError(format!("Failed to create bootstrap resource: {e}")))?;

    info!(bootstrap = %bootstrap_name, "Bootstrap resource created");

    // 2. Create infrastructure resource
    let infra_spec = &resource.spec.infrastructure_spec;
    let infra_obj = json!({
        "apiVersion": infra_spec.api_version,
        "kind": infra_spec.kind,
        "metadata": {
            "name": infra_name,
            "namespace": infra_ns,
            "ownerReferences": [owner_ref],
        },
        "spec": infra_spec.spec,
    });

    create_dynamic_resource(
        client,
        infra_ns,
        &infra_spec.api_version,
        &infra_spec.kind,
        infra_obj,
    )
    .await
    .map_err(|e| {
        ReconcilerError::CapiError(format!("Failed to create infrastructure resource: {e}"))
    })?;

    info!(infrastructure = %infra_name, "Infrastructure resource created");

    // 3. Create CAPI Machine referencing both
    let mut machine_labels = std::collections::BTreeMap::new();
    machine_labels.insert(CAPI_CLUSTER_NAME_LABEL.to_string(), cluster_name.clone());
    machine_labels.insert("5spot.io/scheduled-machine".to_string(), name.clone());

    // Merge in user-provided labels
    if let Some(template) = &resource.spec.machine_template {
        for (k, v) in &template.labels {
            machine_labels.insert(k.clone(), v.clone());
        }
    }

    let mut machine_annotations = std::collections::BTreeMap::new();
    // Merge in user-provided annotations
    if let Some(template) = &resource.spec.machine_template {
        for (k, v) in &template.annotations {
            machine_annotations.insert(k.clone(), v.clone());
        }
    }

    let machine_obj = json!({
        "apiVersion": CAPI_MACHINE_API_VERSION_FULL,
        "kind": "Machine",
        "metadata": {
            "name": machine_name,
            "namespace": namespace,
            "labels": machine_labels,
            "annotations": machine_annotations,
            "ownerReferences": [owner_ref],
        },
        "spec": {
            "clusterName": cluster_name,
            "bootstrap": {
                "configRef": {
                    "apiVersion": bootstrap_spec.api_version,
                    "kind": bootstrap_spec.kind,
                    "name": bootstrap_name,
                    "namespace": bootstrap_ns,
                }
            },
            "infrastructureRef": {
                "apiVersion": infra_spec.api_version,
                "kind": infra_spec.kind,
                "name": infra_name,
                "namespace": infra_ns,
            },
        }
    });

    create_dynamic_resource(
        client,
        namespace,
        CAPI_MACHINE_API_VERSION_FULL,
        "Machine",
        machine_obj,
    )
    .await
    .map_err(|e| ReconcilerError::CapiError(format!("Failed to create Machine: {e}")))?;

    info!(
        resource = %name,
        machine_name = %machine_name,
        "CAPI Machine created successfully"
    );

    Ok(())
}

/// Post a generic Kubernetes resource via the dynamic API client.
///
/// Converts `api_version` and `kind` into a [`kube::api::ApiResource`] and
/// issues a `POST` to the namespaced resource endpoint.  The function is used
/// to create CAPI bootstrap, infrastructure, and `Machine` objects whose types
/// are not statically known at compile time.
///
/// # Errors
/// Returns `kube::Error` if the JSON serialisation or the API call fails.
async fn create_dynamic_resource(
    client: &Client,
    namespace: &str,
    api_version: &str,
    kind: &str,
    obj: serde_json::Value,
) -> Result<(), kube::Error> {
    let (group, version) = parse_api_version(api_version);
    let plural = format!("{}s", kind.to_lowercase());

    let ar = kube::api::ApiResource::from_gvk_with_plural(
        &kube::api::GroupVersionKind::gvk(&group, &version, kind),
        &plural,
    );

    let api: Api<kube::core::DynamicObject> = Api::namespaced_with(client.clone(), namespace, &ar);

    let dyn_obj: kube::core::DynamicObject =
        serde_json::from_value(obj).map_err(kube::Error::SerdeError)?;

    api.create(&kube::api::PostParams::default(), &dyn_obj)
        .await?;
    Ok(())
}

/// Split a Kubernetes `apiVersion` string into `(group, version)`.
///
/// `"bootstrap.cluster.x-k8s.io/v1beta1"` → `("bootstrap.cluster.x-k8s.io", "v1beta1")`
///
/// Core API versions that contain no `/` (e.g., `"v1"`) return an empty
/// group string: `("", "v1")`.  In practice these are always rejected before
/// this function is called by [`validate_api_group`], so the empty-group
/// branch exists only for completeness.
fn parse_api_version(api_version: &str) -> (String, String) {
    if let Some(idx) = api_version.rfind('/') {
        (
            api_version[..idx].to_string(),
            api_version[idx + 1..].to_string(),
        )
    } else {
        // Core API (e.g., "v1")
        (String::new(), api_version.to_string())
    }
}

/// Delete the CAPI `Machine` resource that represents this node in the cluster.
///
/// Deletion is initiated by issuing a `DELETE` to the `Machine` resource.
/// CAPI's own machine controller then handles the provider-specific teardown
/// (deprovision, drain, etc.) asynchronously.  A 404 response is treated as
/// success because it means the machine was already removed.
///
/// # Errors
/// - [`ReconcilerError::CapiError`] — API call failed with a non-404 status
pub async fn remove_machine_from_cluster(
    resource: &ScheduledMachine,
    client: &Client,
    namespace: &str,
) -> Result<(), ReconcilerError> {
    let name = resource.name_any();
    let cluster_name = &resource.spec.cluster_name;
    let machine_name = format!("{name}-machine");

    info!(
        resource = %name,
        namespace = %namespace,
        cluster = %cluster_name,
        machine_name = %machine_name,
        "Deleting CAPI Machine resource"
    );

    // Get the Machine API
    let ar = kube::api::ApiResource::from_gvk_with_plural(
        &kube::api::GroupVersionKind::gvk(CAPI_GROUP, CAPI_MACHINE_API_VERSION, "Machine"),
        CAPI_RESOURCE_MACHINES,
    );
    let machines: Api<kube::core::DynamicObject> =
        Api::namespaced_with(client.clone(), namespace, &ar);

    // Delete the Machine resource if it exists
    match machines
        .delete(&machine_name, &kube::api::DeleteParams::default())
        .await
    {
        Ok(_) => {
            info!(
                machine_name = %machine_name,
                "CAPI Machine deletion initiated"
            );
            Ok(())
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            // Machine already deleted or doesn't exist
            debug!(
                machine_name = %machine_name,
                "CAPI Machine already deleted or does not exist"
            );
            Ok(())
        }
        Err(e) => Err(ReconcilerError::CapiError(format!(
            "Failed to delete Machine {machine_name}: {e}"
        ))),
    }
}

// ============================================================================
// Node Draining
// ============================================================================

/// Get the Kubernetes Node associated with a CAPI Machine
///
/// # Errors
/// Returns error if Machine not found, has no nodeRef, or Node lookup fails
pub async fn get_node_from_machine(
    client: &Client,
    namespace: &str,
    machine_name: &str,
) -> Result<Option<String>, ReconcilerError> {
    let ar = kube::api::ApiResource::from_gvk_with_plural(
        &kube::api::GroupVersionKind::gvk(CAPI_GROUP, CAPI_MACHINE_API_VERSION, "Machine"),
        CAPI_RESOURCE_MACHINES,
    );
    let machines: Api<kube::core::DynamicObject> =
        Api::namespaced_with(client.clone(), namespace, &ar);

    match machines.get(machine_name).await {
        Ok(machine) => {
            // Extract nodeRef from Machine status
            if let Some(status) = machine.data.get("status") {
                if let Some(node_ref) = status.get("nodeRef") {
                    if let Some(node_name) = node_ref.get("name") {
                        if let Some(name_str) = node_name.as_str() {
                            debug!(
                                machine = %machine_name,
                                node = %name_str,
                                "Found Node reference in Machine status"
                            );
                            return Ok(Some(name_str.to_string()));
                        }
                    }
                }
            }
            debug!(
                machine = %machine_name,
                "Machine has no nodeRef in status yet"
            );
            Ok(None)
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(machine = %machine_name, "Machine not found");
            Ok(None)
        }
        Err(e) => Err(ReconcilerError::CapiError(format!(
            "Failed to get Machine {machine_name}: {e}"
        ))),
    }
}

/// Cordon a Kubernetes Node (mark as unschedulable)
///
/// # Errors
/// Returns error if Node not found or update fails
async fn cordon_node(client: &Client, node_name: &str) -> Result<(), ReconcilerError> {
    use k8s_openapi::api::core::v1::Node;

    let nodes: Api<Node> = Api::all(client.clone());

    info!(node = %node_name, "Cordoning node");

    let patch = json!({
        "spec": {
            "unschedulable": true
        }
    });

    nodes
        .patch(node_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .map_err(|e| {
            ReconcilerError::CapiError(format!("Failed to cordon node {node_name}: {e}"))
        })?;

    info!(node = %node_name, "Node cordoned successfully");
    Ok(())
}

/// Return `true` if a pod should be evicted as part of a node drain.
///
/// Pods are skipped when:
/// - Their phase is `Succeeded` or `Failed` — they are already done.
/// - They are owned by a `DaemonSet` — `DaemonSet` pods are automatically
///   re-scheduled by the `DaemonSet` controller and should not be evicted
///   manually; doing so would cause needless churn.
pub fn should_evict_pod(pod: &k8s_openapi::api::core::v1::Pod) -> bool {
    // Skip pods that are already terminating or completed
    if let Some(status) = &pod.status {
        if let Some(phase) = &status.phase {
            if phase == "Succeeded" || phase == "Failed" {
                return false;
            }
        }
    }
    // Skip DaemonSet pods (they will be recreated on other nodes)
    if let Some(owner_refs) = &pod.metadata.owner_references {
        if owner_refs.iter().any(|owner| owner.kind == "DaemonSet") {
            return false;
        }
    }
    true
}

/// Delete a single pod with a graceful termination period.
///
/// Uses [`POD_EVICTION_GRACE_PERIOD_SECS`] as the `gracePeriodSeconds` so
/// the pod's `preStop` hooks and SIGTERM handlers have time to run.
///
/// # Errors
/// Returns [`ReconcilerError::CapiError`] if eviction fails for any reason,
/// including PDB-blocked evictions (HTTP 429). Only 404 (pod already gone)
/// is treated as a non-error condition. The caller is responsible for
/// deciding whether to retry or abort the drain.
async fn evict_pod(
    client: &Client,
    pod_name: &str,
    pod_namespace: &str,
    node_name: &str,
) -> Result<(), ReconcilerError> {
    use k8s_openapi::api::core::v1::Pod;

    let pods_ns: Api<Pod> = Api::namespaced(client.clone(), pod_namespace);
    let delete_params = kube::api::DeleteParams {
        grace_period_seconds: Some(u32::try_from(POD_EVICTION_GRACE_PERIOD_SECS).unwrap_or(30)),
        ..Default::default()
    };

    match pods_ns.delete(pod_name, &delete_params).await {
        Ok(_) => {
            debug!(pod = %pod_name, namespace = %pod_namespace, "Pod eviction initiated");
            record_pod_eviction(true);
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(pod = %pod_name, namespace = %pod_namespace, "Pod already deleted");
            record_pod_eviction(true);
        }
        Err(kube::Error::Api(e)) if e.code == 429 => {
            warn!(pod = %pod_name, namespace = %pod_namespace, "Pod eviction blocked by PDB (HTTP 429)");
            record_pod_eviction(false);
            return Err(ReconcilerError::CapiError(format!(
                "Pod {pod_name} eviction blocked by PodDisruptionBudget (HTTP 429) on node {node_name}"
            )));
        }
        Err(e) => {
            error!(pod = %pod_name, namespace = %pod_namespace, error = %e, "Failed to evict pod");
            record_pod_eviction(false);
            return Err(ReconcilerError::CapiError(format!(
                "Failed to evict pod {pod_name} from node {node_name}: {e}"
            )));
        }
    }
    Ok(())
}

/// Drain a Kubernetes Node by evicting all pods with timeout
///
/// # Errors
/// Returns error if cordoning fails, pod listing fails, or eviction fails
pub async fn drain_node_with_timeout(
    client: &Client,
    node_name: &str,
    timeout: Duration,
) -> Result<(), ReconcilerError> {
    use k8s_openapi::api::core::v1::Pod;

    info!(node = %node_name, timeout_secs = timeout.as_secs(), "Starting node drain");

    cordon_node(client, node_name).await?;

    let pods: Api<Pod> = Api::all(client.clone());
    let list_params =
        kube::api::ListParams::default().fields(&format!("spec.nodeName={node_name}"));

    let pod_list = pods.list(&list_params).await.map_err(|e| {
        ReconcilerError::CapiError(format!("Failed to list pods on node {node_name}: {e}"))
    })?;

    let pods_to_evict: Vec<_> = pod_list
        .items
        .iter()
        .filter(|p| should_evict_pod(p))
        .collect();

    if pods_to_evict.is_empty() {
        info!(node = %node_name, "No pods to evict on node");
        return Ok(());
    }

    info!(node = %node_name, pod_count = pods_to_evict.len(), "Found pods to evict");

    let start_time = std::time::Instant::now();
    for pod in pods_to_evict {
        if start_time.elapsed() >= timeout {
            record_node_drain(false);
            return Err(ReconcilerError::CapiError(format!(
                "Node drain timeout exceeded for {node_name}"
            )));
        }

        let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
        let pod_namespace = pod.metadata.namespace.as_deref().unwrap_or("default");

        debug!(node = %node_name, pod = %pod_name, namespace = %pod_namespace, "Evicting pod");
        evict_pod(client, pod_name, pod_namespace, node_name).await?;
    }

    info!(node = %node_name, elapsed_secs = start_time.elapsed().as_secs(), "Node drain completed");
    record_node_drain(true);
    Ok(())
}

// ============================================================================
// Error policy for controller
// ============================================================================

/// Computes bounded exponential back-off in seconds for a given retry count.
///
/// The delay starts at [`ERROR_REQUEUE_SECS`] and doubles with each successive
/// retry, capped at [`MAX_BACKOFF_SECS`].  Once `retry_count` reaches or exceeds
/// [`MAX_RECONCILE_RETRIES`] the function always returns [`MAX_BACKOFF_SECS`].
///
/// | retry | delay     |
/// |-------|-----------|
/// | 0     | 30 s      |
/// | 1     | 60 s      |
/// | 2     | 120 s     |
/// | 3     | 240 s     |
/// | 4+    | 300 s cap |
pub(crate) fn compute_backoff_secs(retry_count: u32) -> u64 {
    if retry_count >= MAX_RECONCILE_RETRIES {
        return MAX_BACKOFF_SECS;
    }
    // `1u64 << retry_count` doubles the interval each retry; min(63) prevents
    // overflow for large (but pre-capped) retry counts.
    let backoff = ERROR_REQUEUE_SECS.saturating_mul(1u64 << retry_count.min(63));
    backoff.min(MAX_BACKOFF_SECS)
}

/// Controller error policy — log the error and requeue with exponential back-off.
///
/// Called by the `kube-rs` [`Controller`](kube::runtime::Controller) runtime
/// whenever [`reconcile_scheduled_machine`](super::scheduled_machine::reconcile_scheduled_machine)
/// returns an `Err`.  Per-resource retry counts are tracked in [`Context::retry_counts`]:
/// the first failure requeues after [`ERROR_REQUEUE_SECS`]; each subsequent
/// failure doubles the delay up to [`MAX_BACKOFF_SECS`] (Basel III HA resilience
/// / NIST SI-2 flaw remediation).
///
/// The retry count is cleared when reconciliation succeeds (see
/// `reconcile_guarded` in `scheduled_machine.rs`).
pub fn error_policy(
    resource: Arc<ScheduledMachine>,
    err: &ReconcilerError,
    ctx: Arc<Context>,
) -> Action {
    let key = format!(
        "{}/{}",
        resource.namespace().unwrap_or_default(),
        resource.name_any()
    );
    let retry_count = {
        let mut counts = ctx.retry_counts.lock().unwrap_or_else(|p| p.into_inner());
        let count = counts.entry(key).or_insert(0);
        *count = count.saturating_add(1);
        *count
    };
    let backoff = compute_backoff_secs(retry_count);
    error!(
        error = %err,
        retry_count,
        backoff_secs = backoff,
        "Reconciliation error — requeuing with exponential back-off"
    );
    Action::requeue(Duration::from_secs(backoff))
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod helpers_tests;
