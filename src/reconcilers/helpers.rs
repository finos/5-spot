// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Reconciliation helper functions
//!
//! Pure utility functions used by the [`scheduled_machine`](super::scheduled_machine)
//! reconciler.  Separated here to keep the main reconciler focused on the
//! state-machine logic.
//!
//! ## Organisation
//! - **Resource distribution** — consistent hashing for multi-instance deployments
//! - **Schedule evaluation** — timezone-aware day/hour range matching
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
    FINALIZER_SCHEDULED_MACHINE, MAX_BACKOFF_SECS, MAX_CLUSTER_NAME_LEN, MAX_DURATION_SECS,
    MAX_KILL_IF_COMMANDS_COUNT, MAX_KILL_IF_COMMAND_LEN, MAX_RECONCILE_RETRIES, PHASE_ACTIVE,
    PHASE_ERROR, PHASE_INACTIVE, PHASE_SHUTTING_DOWN, PHASE_TERMINATED,
    POD_EVICTION_GRACE_PERIOD_SECS, REASON_GRACE_PERIOD, REASON_KILL_SWITCH,
    REASON_RECONCILE_SUCCEEDED, RESERVED_LABEL_PREFIXES, TIMER_REQUEUE_SECS,
};
use crate::crd::{Condition, NodeRef, ScheduledMachine, ScheduledMachineStatus};
use crate::metrics::{
    record_emergency_drain, record_emergency_reclaim, record_node_drain, record_pod_eviction,
    EmergencyDrainOutcome,
};

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

/// Outcome of a timed cleanup attempt — produced by
/// [`run_cleanup_with_timeout`] and consumed by [`handle_deletion`] to
/// classify what happened to a finalizer-cleanup future.
///
/// The variants are deliberately *not* an `enum E { Ok, Err(_) }`: the
/// caller must distinguish "cleanup itself returned an error" (retry)
/// from "cleanup did not complete in time" (force-remove the finalizer
/// to unblock namespace deletion). Folding the timeout into a generic
/// error was the source of the original bug.
#[derive(Debug)]
pub enum CleanupOutcome {
    /// The cleanup future completed within the timeout with `Ok(())`.
    Completed,
    /// The cleanup future completed within the timeout with `Err(_)`.
    /// Caller should propagate the error and retry on the next reconcile.
    Failed(ReconcilerError),
    /// The cleanup future did NOT complete within the timeout. Caller
    /// should force-remove the finalizer to prevent stalling namespace
    /// deletion and surface a Warning event so operators verify orphans.
    TimedOut,
}

/// Wrap a cleanup future in a hard timeout and classify the outcome.
///
/// Pulled out as a standalone async function so the three branches can be
/// unit-tested in microseconds (`tokio::test(start_paused = true)`)
/// without mocking the kube API.
pub async fn run_cleanup_with_timeout<F>(timeout_duration: Duration, cleanup: F) -> CleanupOutcome
where
    F: std::future::Future<Output = Result<(), ReconcilerError>>,
{
    match tokio::time::timeout(timeout_duration, cleanup).await {
        Ok(Ok(())) => CleanupOutcome::Completed,
        Ok(Err(e)) => CleanupOutcome::Failed(e),
        Err(_) => CleanupOutcome::TimedOut,
    }
}

/// Build the `Warning` Kubernetes Event surfaced on a `ScheduledMachine`
/// whose finalizer was force-removed because cleanup exceeded
/// [`FINALIZER_CLEANUP_TIMEOUT_SECS`].
///
/// The note is the only operator-facing signal in `kubectl describe`, so
/// it MUST direct the operator to the orphan-cleanup runbook — the
/// controller has accepted that CAPI Machine + bootstrap/infra resources
/// may now be unowned and require manual verification.
#[must_use]
pub fn build_finalizer_timeout_event(timeout_secs: u64) -> KubeEvent {
    KubeEvent {
        type_: EventType::Warning,
        reason: "FinalizerCleanupTimedOut".to_string(),
        note: Some(format!(
            "Finalizer cleanup exceeded {timeout_secs}s timeout. The finalizer was \
             force-removed to unblock namespace deletion. Operators MUST manually verify \
             that the CAPI Machine and its bootstrap/infrastructure resources have been \
             cleaned up — orphan resources are possible. See the troubleshooting docs \
             ('Orphan resources after finalizer timeout') for the runbook."
        )),
        action: "FinalizerCleanupTimedOut".to_string(),
        secondary: None,
    }
}

/// Run finalizer cleanup when a `ScheduledMachine` is being deleted.
///
/// If the resource is currently in the `Active` or `ShuttingDown` phase the
/// corresponding CAPI Machine (and its child resources) are removed from the
/// cluster first. The removal is wrapped in a hard
/// [`FINALIZER_CLEANUP_TIMEOUT_SECS`] timeout so a hung API call cannot
/// block namespace deletion or cluster upgrades indefinitely.
///
/// On timeout the finalizer is **force-removed** (the reverse of the
/// original behaviour, which propagated the error and stalled deletion
/// indefinitely). The trade-off: a misbehaving Pod Disruption Budget on
/// a tenant workload can no longer block namespace deletion, but the
/// CAPI Machine and bootstrap/infrastructure resources may be left as
/// orphans. A `FinalizerCleanupTimedOut` Warning event is published on
/// the SM and `fivespot_finalizer_cleanup_timeouts_total` is incremented
/// so operators can detect and reconcile orphans.
///
/// On a non-timeout cleanup error (e.g., real CAPI delete failure) the
/// finalizer is **kept** and the error is propagated so the controller
/// retries on the next reconcile.
///
/// Once cleanup succeeds (or is skipped for non-running phases, or is
/// force-removed on timeout) the finalizer string is removed from
/// `metadata.finalizers`. After this patch Kubernetes considers the
/// resource fully deleted.
///
/// # Errors
/// - [`ReconcilerError::InvalidConfig`] — resource has no namespace
/// - [`ReconcilerError::CapiError`] — CAPI Machine delete call failed
///   (returned without removing the finalizer; reconciler retries)
/// - kube API error — finalizer patch call failed
///
/// Note: [`ReconcilerError::TimeoutError`] is no longer returned from this
/// function — timeouts now drive the force-remove path rather than
/// propagating up.
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

            let outcome = run_cleanup_with_timeout(
                cleanup_timeout,
                remove_machine_from_cluster(&resource, &ctx.client, &namespace),
            )
            .await;

            match outcome {
                CleanupOutcome::Completed => {
                    debug!(
                        resource = %name,
                        namespace = %namespace,
                        "Cleanup completed within timeout"
                    );
                }
                CleanupOutcome::Failed(e) => {
                    // Real cleanup error (e.g. CAPI delete failed): keep the
                    // finalizer, propagate the error, and let the reconciler
                    // retry with exponential back-off.
                    return Err(e);
                }
                CleanupOutcome::TimedOut => {
                    // Always increment the metric + emit Warning event so
                    // operators see timeouts regardless of which mode is on.
                    crate::metrics::record_finalizer_cleanup_timeout();
                    let object_ref = ObjectReference {
                        api_version: Some(API_VERSION_FULL.to_string()),
                        kind: Some(crate::constants::KIND_SCHEDULED_MACHINE.to_string()),
                        name: Some(name.clone()),
                        namespace: Some(namespace.clone()),
                        ..Default::default()
                    };
                    let event = build_finalizer_timeout_event(FINALIZER_CLEANUP_TIMEOUT_SECS);
                    publish_best_effort(&ctx, &object_ref, event, "finalizer-timeout").await;

                    if ctx.force_finalizer_on_timeout {
                        // Force-remove path (default): log and fall through
                        // to finalizer removal so namespace deletion can
                        // proceed.
                        warn!(
                            resource = %name,
                            namespace = %namespace,
                            timeout_secs = FINALIZER_CLEANUP_TIMEOUT_SECS,
                            "Finalizer cleanup timed out; force-removing finalizer to unblock \
                             namespace deletion. Operators MUST verify CAPI Machine + \
                             bootstrap/infrastructure resources have been cleaned up — \
                             orphans are possible."
                        );
                    } else {
                        // Strict-cleanup mode: keep the finalizer and
                        // propagate the timeout so the reconciler retries.
                        // Operators must have an external sweep to garbage-
                        // collect stuck SMs in this mode.
                        error!(
                            resource = %name,
                            namespace = %namespace,
                            timeout_secs = FINALIZER_CLEANUP_TIMEOUT_SECS,
                            "Finalizer cleanup timed out; strict-cleanup mode enabled, \
                             keeping finalizer and retrying. SM deletion is BLOCKED until \
                             cleanup succeeds."
                        );
                        return Err(ReconcilerError::TimeoutError(format!(
                            "Finalizer cleanup timed out after {FINALIZER_CLEANUP_TIMEOUT_SECS}s for {name}"
                        )));
                    }
                }
            }
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
        false, // in_schedule: kill switch overrides schedule
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
        let elapsed_secs = elapsed.num_seconds();

        debug!(
            grace_start = %start_time,
            elapsed_secs,
            timeout_secs = timeout.as_secs(),
            "Grace period check"
        );

        // Clock-skew guard: if the recorded start timestamp is *ahead* of the
        // current wall clock (NTP step, VM freeze-thaw, or a manual clock
        // adjustment), `elapsed_secs` is negative. The prior
        // `elapsed.num_seconds() >= timeout.as_secs() as i64` expression
        // silently returned false in that case and could bypass the
        // graceful-drain window entirely. Treat any negative elapsed as
        // "timeout reached" so the reconciler forces progress rather than
        // stalling on a misbehaving clock.
        if elapsed_secs < 0 {
            warn!(
                grace_start = %start_time,
                now_utc = %now,
                elapsed_secs,
                "Negative elapsed time detected (clock adjustment?); treating grace period as elapsed"
            );
            return Ok(true);
        }

        // Safe cast: u64 timeout converted to i64 cannot wrap because
        // MAX_DURATION_SECS (24h) fits in i64, and elapsed_secs is now
        // known non-negative.
        #[allow(clippy::cast_possible_wrap)]
        Ok(elapsed_secs >= timeout.as_secs() as i64)
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

    // Reject non-ASCII early. split_at(len - 1) below indexes by bytes; a
    // multi-byte UTF-8 code point at the tail panics. Legal units are s/m/h
    // (all ASCII) so any non-ASCII byte is already an invalid input.
    if !duration_str.is_ascii() {
        return Err(ReconcilerError::InvalidConfig(format!(
            "Invalid duration (non-ASCII): {duration_str:?}"
        )));
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
#[allow(clippy::too_many_arguments)]
pub async fn update_phase(
    ctx: &Context,
    namespace: &str,
    name: &str,
    from_phase: Option<&str>,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
    in_schedule: bool,
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
        in_schedule,
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
#[allow(clippy::too_many_arguments)]
pub async fn update_phase_with_last_schedule(
    ctx: &Context,
    namespace: &str,
    name: &str,
    from_phase: Option<&str>,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
    in_schedule: bool,
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
        in_schedule,
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
#[allow(clippy::too_many_arguments)]
pub async fn update_phase_with_grace_period(
    ctx: &Context,
    namespace: &str,
    name: &str,
    from_phase: Option<&str>,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
    in_schedule: bool,
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
        in_schedule,
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

/// Reject `spec.clusterName` values that exceed the effective CAPI cap or
/// contain characters unsafe for log / label / metric emission.
///
/// CAPI itself inherits the Kubernetes 253-char DNS-1123 subdomain cap on
/// `Cluster.metadata.name`, but the name flows downstream into DNS labels
/// (63 chars) and the `cluster.x-k8s.io/cluster-name` label value. 63 is
/// therefore the *effective* upper bound — see [`MAX_CLUSTER_NAME_LEN`].
///
/// The character check rejects control chars, whitespace, and non-ASCII so
/// a malicious CR cannot forge log records via embedded newlines or bloat
/// Prometheus label cardinality with exotic Unicode.
///
/// # Errors
/// [`ReconcilerError::ValidationError`] if the name is empty, too long, or
/// contains a disallowed character.
pub fn validate_cluster_name(cluster_name: &str) -> Result<(), ReconcilerError> {
    if cluster_name.is_empty() {
        return Err(ReconcilerError::ValidationError(
            "spec.clusterName must not be empty".to_string(),
        ));
    }
    if cluster_name.len() > MAX_CLUSTER_NAME_LEN {
        return Err(ReconcilerError::ValidationError(format!(
            "spec.clusterName length {} exceeds maximum of {MAX_CLUSTER_NAME_LEN} characters",
            cluster_name.len()
        )));
    }
    // Allow ASCII letters, digits, '-', '.', '_' — the intersection of
    // DNS-1123 subdomain and Kubernetes label-value charsets. Anything
    // outside that (including embedded NUL, newline, or multi-byte UTF-8)
    // is rejected with a single uniform error.
    if !cluster_name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
    {
        return Err(ReconcilerError::ValidationError(format!(
            "spec.clusterName contains invalid character(s); allowed: ASCII alphanumerics, '-', '.', '_' (got: {cluster_name:?})"
        )));
    }
    Ok(())
}

/// Reject `spec.killIfCommands` lists that exceed the count cap or contain
/// an over-length / empty pattern.
///
/// The reclaim agent evaluates each pattern against every PID in `/proc`;
/// without a cap, a single CR can pin an entire node's agent CPU or balloon
/// the per-node `ConfigMap` projection. Empty patterns would match every
/// process and are almost certainly a mistake rather than an intentional
/// kill-all rule — reject them loudly.
///
/// `None` and `Some(&[])` are both accepted: the controller treats them
/// identically ("no reclaim agent on this node").
///
/// # Errors
/// [`ReconcilerError::ValidationError`] on any violation, naming the
/// offending index so operators can fix the CR quickly.
pub fn validate_kill_if_commands(commands: Option<&[String]>) -> Result<(), ReconcilerError> {
    let Some(cmds) = commands else {
        return Ok(());
    };
    if cmds.len() > MAX_KILL_IF_COMMANDS_COUNT {
        return Err(ReconcilerError::ValidationError(format!(
            "spec.killIfCommands has {} entries, exceeds maximum of {MAX_KILL_IF_COMMANDS_COUNT}",
            cmds.len()
        )));
    }
    for (idx, cmd) in cmds.iter().enumerate() {
        if cmd.is_empty() {
            return Err(ReconcilerError::ValidationError(format!(
                "spec.killIfCommands[{idx}] must not be empty"
            )));
        }
        if cmd.len() > MAX_KILL_IF_COMMAND_LEN {
            return Err(ReconcilerError::ValidationError(format!(
                "spec.killIfCommands[{idx}] length {} exceeds maximum of {MAX_KILL_IF_COMMAND_LEN} characters",
                cmd.len()
            )));
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

/// Add machine to cluster by creating bootstrap, infrastructure, and Machine resources
///
/// This function:
/// 1. Creates the bootstrap resource from bootstrapSpec
/// 2. Creates the infrastructure resource from infrastructureSpec
/// 3. Creates the CAPI Machine referencing both
///
/// # Invariants
///
/// `namespace` MUST equal `resource.namespace()`. Every child resource
/// (bootstrap, infrastructure, Machine) carries an `ownerReference` back
/// to the `ScheduledMachine`. Kubernetes' garbage collector treats
/// owner-cascade as a same-namespace relationship: a child in
/// namespace `B` cannot be garbage-collected by an owner in namespace
/// `A` — the API server silently ignores the ownerRef. Mismatching
/// namespaces would therefore produce orphan resources on cascading
/// delete without any error surfaced at creation time.
///
/// The `debug_assert_eq!` below makes the violation loud in test and
/// dev builds. Today every caller passes `resource.namespace()` so the
/// invariant holds; the assertion is a regression guard for future
/// refactors that might move bootstrap / infra creation to a different
/// namespace without updating the ownerRef contract. Filed as Phase 6b
/// of the 2026-04-25 security audit roadmap.
#[allow(clippy::too_many_lines)]
pub async fn add_machine_to_cluster(
    resource: &ScheduledMachine,
    client: &Client,
    namespace: &str,
) -> Result<(), ReconcilerError> {
    debug_assert_eq!(
        resource.namespace().as_deref(),
        Some(namespace),
        "ownerRef contract: bootstrap/infra/Machine MUST be created in the SM's own \
         namespace; cross-namespace ownerReferences are silently ignored by the GC"
    );

    let name = resource.name_any();
    let cluster_name = &resource.spec.cluster_name;

    info!(
        resource = %name,
        namespace = %namespace,
        cluster = %cluster_name,
        "Creating CAPI resources from inline specs"
    );

    // Extract required fields from embedded resources
    let bootstrap_api_version = resource.spec.bootstrap_spec.api_version().ok_or_else(|| {
        ReconcilerError::InvalidConfig(
            "bootstrapSpec missing required field 'apiVersion'".to_string(),
        )
    })?;
    let bootstrap_kind = resource.spec.bootstrap_spec.kind().ok_or_else(|| {
        ReconcilerError::InvalidConfig("bootstrapSpec missing required field 'kind'".to_string())
    })?;
    let bootstrap_spec_inner = resource
        .spec
        .bootstrap_spec
        .spec()
        .cloned()
        .unwrap_or_default();

    let infra_api_version = resource
        .spec
        .infrastructure_spec
        .api_version()
        .ok_or_else(|| {
            ReconcilerError::InvalidConfig(
                "infrastructureSpec missing required field 'apiVersion'".to_string(),
            )
        })?;
    let infra_kind = resource.spec.infrastructure_spec.kind().ok_or_else(|| {
        ReconcilerError::InvalidConfig(
            "infrastructureSpec missing required field 'kind'".to_string(),
        )
    })?;
    let infra_spec_inner = resource
        .spec
        .infrastructure_spec
        .spec()
        .cloned()
        .unwrap_or_default();

    // Validate cluster name and killIfCommands bounds before touching CAPI.
    // `cluster_name` flows into every derived resource's label set and into
    // log/metric emission; `killIfCommands` into the per-node reclaim-agent
    // `ConfigMap`. Rejecting oversize input here keeps both failure modes
    // out of the hot path. Admission policy does the same check at
    // CREATE/UPDATE — this runtime guard is defence-in-depth for clusters
    // that have not enabled ValidatingAdmissionPolicy.
    validate_cluster_name(cluster_name)?;
    validate_kill_if_commands(resource.spec.kill_if_commands.as_deref())?;

    // Validate API groups before creating any resources
    validate_api_group(
        bootstrap_api_version,
        ALLOWED_BOOTSTRAP_API_GROUPS,
        "bootstrap",
    )?;
    validate_api_group(
        infra_api_version,
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
    // NOTE: No ownerReferences here - the bootstrap controller (e.g., k0smotron) needs to
    // process this resource. We use labels for tracking instead, and the CAPI Machine's
    // bootstrap.configRef provides the logical relationship.
    let bootstrap_obj = json!({
        "apiVersion": bootstrap_api_version,
        "kind": bootstrap_kind,
        "metadata": {
            "name": name,
            "namespace": bootstrap_ns,
            "labels": {
                "5spot.finos.org/scheduled-machine": name,
                CAPI_CLUSTER_NAME_LABEL: cluster_name,
            },
        },
        "spec": bootstrap_spec_inner,
    });

    create_dynamic_resource(
        client,
        bootstrap_ns,
        bootstrap_api_version,
        bootstrap_kind,
        bootstrap_obj,
    )
    .await
    .map_err(|e| ReconcilerError::CapiError(format!("Failed to create bootstrap resource: {e}")))?;

    info!(kind = %bootstrap_kind, "Bootstrap resource created");

    // 2. Create infrastructure resource
    // NOTE: No ownerReferences here - the infrastructure controller (e.g., CAPM3, CAPA) needs to
    // process this resource. We use labels for tracking instead, and the CAPI Machine's
    // infrastructureRef provides the logical relationship.
    let infra_obj = json!({
        "apiVersion": infra_api_version,
        "kind": infra_kind,
        "metadata": {
            "name": name,
            "namespace": infra_ns,
            "labels": {
                "5spot.finos.org/scheduled-machine": name,
                CAPI_CLUSTER_NAME_LABEL: cluster_name,
            },
        },
        "spec": infra_spec_inner,
    });

    create_dynamic_resource(client, infra_ns, infra_api_version, infra_kind, infra_obj)
        .await
        .map_err(|e| {
            ReconcilerError::CapiError(format!("Failed to create infrastructure resource: {e}"))
        })?;

    info!(kind = %infra_kind, "Infrastructure resource created");

    // 3. Create CAPI Machine referencing both
    let mut machine_labels = std::collections::BTreeMap::new();
    machine_labels.insert(CAPI_CLUSTER_NAME_LABEL.to_string(), cluster_name.clone());
    machine_labels.insert(
        "5spot.finos.org/scheduled-machine".to_string(),
        name.clone(),
    );

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
            "name": name,
            "namespace": namespace,
            "labels": machine_labels,
            "annotations": machine_annotations,
            "ownerReferences": [owner_ref],
        },
        "spec": {
            "clusterName": cluster_name,
            "bootstrap": {
                "configRef": {
                    "apiVersion": bootstrap_api_version,
                    "kind": bootstrap_kind,
                    "name": name,
                    "namespace": bootstrap_ns,
                }
            },
            "infrastructureRef": {
                "apiVersion": infra_api_version,
                "kind": infra_kind,
                "name": name,
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

    info!(resource = %name, "CAPI Machine created successfully");

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

/// Delete a Kubernetes resource via the dynamic API client.
///
/// A 404 response is treated as success (resource already deleted).
///
/// # Errors
/// Returns `ReconcilerError::CapiError` if the API call fails with a non-404 status.
async fn delete_dynamic_resource(
    client: &Client,
    namespace: &str,
    api_version: &str,
    kind: &str,
    name: &str,
) -> Result<(), ReconcilerError> {
    let (group, version) = parse_api_version(api_version);
    let plural = format!("{}s", kind.to_lowercase());

    let ar = kube::api::ApiResource::from_gvk_with_plural(
        &kube::api::GroupVersionKind::gvk(&group, &version, kind),
        &plural,
    );

    let api: Api<kube::core::DynamicObject> = Api::namespaced_with(client.clone(), namespace, &ar);

    match api.delete(name, &kube::api::DeleteParams::default()).await {
        Ok(_) => {
            info!(kind = %kind, name = %name, "Resource deletion initiated");
            Ok(())
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(kind = %kind, name = %name, "Resource already deleted or does not exist");
            Ok(())
        }
        Err(e) => Err(ReconcilerError::CapiError(format!(
            "Failed to delete {kind} {name}: {e}"
        ))),
    }
}

/// Delete the CAPI `Machine` resource that represents this node in the cluster.
///
/// Deletion is initiated by issuing a `DELETE` to the `Machine` resource.
/// CAPI's own machine controller then handles the provider-specific teardown
/// (deprovision, drain, etc.) asynchronously.  A 404 response is treated as
/// success because it means the machine was already removed.
///
/// Also deletes the associated bootstrap and infrastructure resources since they
/// no longer have ownerReferences (to allow their respective controllers to process them).
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

    info!(
        resource = %name,
        namespace = %namespace,
        cluster = %cluster_name,
        "Deleting CAPI resources"
    );

    // 1. Delete the Machine resource first (this triggers CAPI cleanup)
    let ar = kube::api::ApiResource::from_gvk_with_plural(
        &kube::api::GroupVersionKind::gvk(CAPI_GROUP, CAPI_MACHINE_API_VERSION, "Machine"),
        CAPI_RESOURCE_MACHINES,
    );
    let machines: Api<kube::core::DynamicObject> =
        Api::namespaced_with(client.clone(), namespace, &ar);

    match machines
        .delete(&name, &kube::api::DeleteParams::default())
        .await
    {
        Ok(_) => {
            info!(resource = %name, "CAPI Machine deletion initiated");
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(resource = %name, "CAPI Machine already deleted or does not exist");
        }
        Err(e) => {
            return Err(ReconcilerError::CapiError(format!(
                "Failed to delete Machine {name}: {e}"
            )));
        }
    }

    // 2. Delete bootstrap resource (no ownerReference, must delete explicitly)
    let bootstrap_api_version = resource.spec.bootstrap_spec.api_version();
    let bootstrap_kind = resource.spec.bootstrap_spec.kind();
    if let (Some(api_version), Some(kind)) = (bootstrap_api_version, bootstrap_kind) {
        delete_dynamic_resource(client, namespace, api_version, kind, &name).await?;
    }

    // 3. Delete infrastructure resource (no ownerReference, must delete explicitly)
    let infra_api_version = resource.spec.infrastructure_spec.api_version();
    let infra_kind = resource.spec.infrastructure_spec.kind();
    if let (Some(api_version), Some(kind)) = (infra_api_version, infra_kind) {
        delete_dynamic_resource(client, namespace, api_version, kind, &name).await?;
    }

    Ok(())
}

// ============================================================================
// CAPI Machine status extraction
// ============================================================================

/// Pull `providerID` and the full `NodeRef` out of a CAPI Machine `DynamicObject`.
///
/// Pure: no I/O, safe to unit-test. Treats partial data as "not yet populated":
/// a `nodeRef` missing `apiVersion`, `kind`, or `name` yields `None` for the
/// ref rather than a half-filled struct.
///
/// # Returns
/// `(providerID, nodeRef)` — either or both may be `None` while CAPI is still
/// reconciling the underlying Machine.
#[must_use]
pub fn extract_machine_refs(
    machine: &kube::core::DynamicObject,
) -> (Option<String>, Option<NodeRef>) {
    let provider_id = machine
        .data
        .get("spec")
        .and_then(|s| s.get("providerID"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    let node_ref = machine
        .data
        .get("status")
        .and_then(|s| s.get("nodeRef"))
        .and_then(|nr| {
            let api_version = nr.get("apiVersion").and_then(serde_json::Value::as_str)?;
            let kind = nr.get("kind").and_then(serde_json::Value::as_str)?;
            let name = nr.get("name").and_then(serde_json::Value::as_str)?;
            let uid = nr
                .get("uid")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            Some(NodeRef {
                api_version: api_version.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                uid,
            })
        });

    (provider_id, node_ref)
}

/// Fetch the CAPI Machine for a `ScheduledMachine` by conventional name.
///
/// Returns `Ok(None)` on 404 so callers can treat a missing Machine as
/// "not yet created" rather than an error.
///
/// # Errors
/// Returns `ReconcilerError::CapiError` on any non-404 Kubernetes API failure.
pub async fn fetch_capi_machine(
    client: &Client,
    namespace: &str,
    machine_name: &str,
) -> Result<Option<kube::core::DynamicObject>, ReconcilerError> {
    let ar = kube::api::ApiResource::from_gvk_with_plural(
        &kube::api::GroupVersionKind::gvk(CAPI_GROUP, CAPI_MACHINE_API_VERSION, "Machine"),
        CAPI_RESOURCE_MACHINES,
    );
    let machines: Api<kube::core::DynamicObject> =
        Api::namespaced_with(client.clone(), namespace, &ar);
    match machines.get(machine_name).await {
        Ok(m) => Ok(Some(m)),
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(machine = %machine_name, "Machine not found");
            Ok(None)
        }
        Err(e) => Err(ReconcilerError::CapiError(format!(
            "Failed to get Machine {machine_name}: {e}"
        ))),
    }
}

/// Patch `providerID` and/or `nodeRef` onto `ScheduledMachine.status`.
///
/// No-op when both arguments are `None`. Uses a Merge patch so other status
/// fields (phase, conditions, timestamps) are preserved. Fields are only
/// written when `Some` — `None` values do NOT clear the existing value.
///
/// # Errors
/// Returns `ReconcilerError` if the status subresource patch fails.
pub async fn patch_machine_refs_status(
    client: &Client,
    namespace: &str,
    name: &str,
    provider_id: Option<&str>,
    node_ref: Option<&NodeRef>,
) -> Result<(), ReconcilerError> {
    let mut status_fields = serde_json::Map::new();
    if let Some(pid) = provider_id {
        status_fields.insert("providerID".to_string(), json!(pid));
    }
    if let Some(nref) = node_ref {
        status_fields.insert("nodeRef".to_string(), json!(nref));
    }
    if status_fields.is_empty() {
        return Ok(());
    }

    let patch = json!({ "status": serde_json::Value::Object(status_fields) });
    let api: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);
    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    debug!(
        resource = %name,
        namespace = %namespace,
        has_provider_id = provider_id.is_some(),
        has_node_ref = node_ref.is_some(),
        "Patched Machine refs into ScheduledMachine status"
    );
    Ok(())
}

// ============================================================================
// Node Draining
// ============================================================================

/// Get the Kubernetes Node name associated with a CAPI Machine, resolving via
/// `status.nodeRef.name`.
///
/// Thin wrapper around [`fetch_capi_machine`] + [`extract_machine_refs`] kept
/// for callers that only need the drain target name.
///
/// # Errors
/// Returns `ReconcilerError::CapiError` on non-404 API failures.
pub async fn get_node_from_machine(
    client: &Client,
    namespace: &str,
    machine_name: &str,
) -> Result<Option<String>, ReconcilerError> {
    let Some(machine) = fetch_capi_machine(client, namespace, machine_name).await? else {
        return Ok(None);
    };
    let (_, node_ref) = extract_machine_refs(&machine);
    let Some(nref) = node_ref else {
        debug!(
            machine = %machine_name,
            "Machine has no nodeRef in status yet"
        );
        return Ok(None);
    };
    debug!(
        machine = %machine_name,
        node = %nref.name,
        "Found Node reference in Machine status"
    );
    Ok(Some(nref.name))
}

// ============================================================================
// Secondary-watch mappers (label/name → ObjectRef<ScheduledMachine>)
// ============================================================================

/// Map a CAPI Machine event to the owning `ScheduledMachine`.
///
/// Uses the `5spot.eribourg.dev/scheduled-machine` label that the reconciler
/// already stamps on every Machine it creates. Returns an empty vec when the
/// label is missing, empty, whitespace-only, or when the Machine has no
/// namespace — any of those would produce an ill-formed reconcile request.
///
/// Returns `Vec` (rather than `Option`) so it composes directly with the
/// `kube::runtime::Controller::watches()` mapper contract
/// (`IntoIterator<Item = ObjectRef<K>>`).
#[must_use]
pub fn machine_to_scheduled_machine(
    machine: &kube::core::DynamicObject,
) -> Vec<kube::runtime::reflector::ObjectRef<ScheduledMachine>> {
    let Some(labels) = machine.metadata.labels.as_ref() else {
        return Vec::new();
    };
    let Some(raw_name) = labels.get(crate::labels::LABEL_SCHEDULED_MACHINE) else {
        return Vec::new();
    };
    let name = raw_name.trim();
    if name.is_empty() {
        return Vec::new();
    }
    let Some(namespace) = machine.metadata.namespace.as_deref() else {
        return Vec::new();
    };
    vec![kube::runtime::reflector::ObjectRef::<ScheduledMachine>::new(name).within(namespace)]
}

/// Map a `Node` event to all `ScheduledMachine`s whose
/// `status.nodeRef.name == node.metadata.name`.
///
/// **DEPRECATED — replaced by [`node_to_scheduled_machines_via_machine`].**
/// This function trusts the SM's own `status.nodeRef.name`, which is
/// writable by anyone with `patch scheduledmachines/status` on the
/// resource. A tenant could write a fast-changing node name into
/// status to amplify how many SM reconciliations they trigger from
/// unrelated Node events (CPU DoS amplifier — see Phase 3 of the
/// 2026-04-25 security audit roadmap).
///
/// The replacement routes Node → Machine (canonical
/// `Machine.status.nodeRef.name` written by CAPI) → SM (via the
/// controller-stamped `LABEL_SCHEDULED_MACHINE` label on the Machine),
/// closing the spoof window.
///
/// Kept exported with `#[deprecated]` for one release so external
/// callers (none expected) get a compile-time pointer at the
/// replacement.
///
/// Runs `O(N)` over the supplied SM iterator. Fine at small scale (tens to
/// hundreds of SMs); if the cluster ever hosts thousands, swap in a reverse
/// index keyed by the last-observed `nodeRef.name`.
#[must_use]
#[deprecated(
    since = "0.1.2",
    note = "Trusts tenant-writable SM.status.nodeRef.name; use \
            node_to_scheduled_machines_via_machine for canonical-Machine routing."
)]
pub fn node_to_scheduled_machines<'a, I>(
    node: &k8s_openapi::api::core::v1::Node,
    scheduled_machines: I,
) -> Vec<kube::runtime::reflector::ObjectRef<ScheduledMachine>>
where
    I: IntoIterator<Item = &'a ScheduledMachine>,
{
    let Some(node_name) = node.metadata.name.as_deref() else {
        return Vec::new();
    };
    if node_name.is_empty() {
        return Vec::new();
    }
    scheduled_machines
        .into_iter()
        .filter_map(|sm| {
            let nref = sm.status.as_ref()?.node_ref.as_ref()?;
            if nref.name.is_empty() || nref.name != node_name {
                return None;
            }
            let name = sm.metadata.name.as_deref()?;
            let namespace = sm.metadata.namespace.as_deref()?;
            Some(
                kube::runtime::reflector::ObjectRef::<ScheduledMachine>::new(name)
                    .within(namespace),
            )
        })
        .collect()
}

/// Map a `Node` event to all `ScheduledMachine`s whose **owning CAPI Machine**
/// has `status.nodeRef.name == node.metadata.name`.
///
/// This is the canonical-Machine replacement for
/// [`node_to_scheduled_machines`]. Routing via the CAPI Machine instead of the
/// SM's own status closes the watch-amplification vector where a tenant with
/// `patch scheduledmachines/status` could spoof `SM.status.nodeRef.name` to
/// pin controller CPU on unrelated Node updates: only the CAPI controller
/// writes `Machine.status.nodeRef`, and only the 5-Spot controller stamps
/// the [`crate::labels::LABEL_SCHEDULED_MACHINE`] label that walks back to
/// the SM. Both legs are write-controlled by trusted controllers, not by
/// the tenant.
///
/// Algorithm (per Machine in the supplied iterator):
/// 1. Read `metadata.namespace` — required to build a namespaced
///    `ObjectRef`. Skip Machines without one.
/// 2. Read `status.nodeRef.name` from the Machine's dynamic `data` field
///    (`DynamicObject` because CAPI types are not statically modelled in
///    this codebase). Skip Machines whose `nodeRef` is absent or doesn't
///    match the node's name.
/// 3. Read `metadata.labels[LABEL_SCHEDULED_MACHINE]` to identify the
///    owning SM. Skip if the label is missing, empty, or whitespace-only
///    (mirrors the same defensive trim+empty check in
///    [`machine_to_scheduled_machine`]).
/// 4. Emit one [`kube::runtime::reflector::ObjectRef`] per match.
///
/// Returns an empty `Vec` for the degenerate case where the Node itself
/// has no `metadata.name` — there is nothing meaningful to match against.
///
/// Runs `O(N)` over the supplied Machine iterator. With one Machine per
/// SM and tens-to-hundreds of SMs per cluster this is comfortably cheap;
/// at thousands of SMs, build a reverse index keyed by node name.
///
/// # Threat model context
/// Filed as Phase 3 of the 2026-04-25 security audit roadmap (severity:
/// Low — CPU amplification, not a drain pivot). The reconciler always
/// reads canonical `Machine.status.nodeRef` via `get_node_from_machine`
/// before draining, so the spoof never reached the drain target — but
/// it still amplified per-SM CPU cost. This routing change makes the
/// amplification impossible regardless of how the reconciler evolves.
#[must_use]
pub fn node_to_scheduled_machines_via_machine<'a, M>(
    node: &k8s_openapi::api::core::v1::Node,
    machines: M,
) -> Vec<kube::runtime::reflector::ObjectRef<ScheduledMachine>>
where
    M: IntoIterator<Item = &'a kube::core::DynamicObject>,
{
    let Some(node_name) = node.metadata.name.as_deref() else {
        return Vec::new();
    };
    if node_name.is_empty() {
        return Vec::new();
    }

    machines
        .into_iter()
        .filter_map(|machine| {
            // Canonical nodeRef from Machine.status — written only by the
            // CAPI controller, not by the tenant.
            let machine_node_name = machine
                .data
                .get("status")?
                .get("nodeRef")?
                .get("name")?
                .as_str()?;
            if machine_node_name.is_empty() || machine_node_name != node_name {
                return None;
            }

            // Walk to the owning SM via the controller-stamped label —
            // same primitive used by machine_to_scheduled_machine.
            let labels = machine.metadata.labels.as_ref()?;
            let raw_sm_name = labels.get(crate::labels::LABEL_SCHEDULED_MACHINE)?;
            let sm_name = raw_sm_name.trim();
            if sm_name.is_empty() {
                return None;
            }

            let sm_namespace = machine.metadata.namespace.as_deref()?;

            Some(
                kube::runtime::reflector::ObjectRef::<ScheduledMachine>::new(sm_name)
                    .within(sm_namespace),
            )
        })
        .collect()
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
#[allow(clippy::needless_pass_by_value)] // kube-rs Controller API requires Arc by value
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
    // Explicit poison handling: if another reconciler thread panicked while
    // holding `retry_counts`, the map may be in an inconsistent state. Rather
    // than silently recovering the inner map and continuing with partially-
    // modified retry counters (which would corrupt the exponential-backoff
    // schedule), log the incident and requeue after the standard error
    // interval — a subsequent reconcile will proceed from a fresh lock.
    let retry_count = match ctx.retry_counts.lock() {
        Ok(mut counts) => {
            let count = counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
            *count
        }
        Err(poisoned) => {
            error!(
                resource_key = %key,
                error = %poisoned,
                original_error = %err,
                "retry_counts mutex poisoned; aborting this error-policy pass and requeuing after ERROR_REQUEUE_SECS"
            );
            return Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS));
        }
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

// ============================================================================
// Emergency reclaim — node annotation detection + cleanup
// ============================================================================
//
// The node-side `5spot-reclaim-agent` writes three annotations on its own
// `Node` to signal the controller (contract defined in `constants.rs`:
// `RECLAIM_REQUESTED_ANNOTATION` and siblings). These helpers cover the
// two primitives the controller needs:
//
// 1. [`node_reclaim_request`] — parse a Node into a typed
//    [`ReclaimRequest`] when the trigger annotation is set to the literal
//    `"true"`. Any other state (missing, empty, wrong value) yields
//    `None`, mirroring the strict check in the agent and guarding
//    against partial-write foot-guns.
//
// 2. [`build_clear_reclaim_patch`] — the merge-patch body run as the
//    last step of `Phase::EmergencyRemove` to wipe all three
//    annotations, so a node that rejoins the cluster later does not
//    immediately re-fire the trigger on stale metadata.

/// Typed view of the reclaim annotations observed on a `Node`. `reason`
/// and `requested_at` are `Option` so a missing value does not veto the
/// trigger — the boolean [`RECLAIM_REQUESTED_ANNOTATION`] is the
/// contract; reason/timestamp are audit metadata.
///
/// [`RECLAIM_REQUESTED_ANNOTATION`]: crate::constants::RECLAIM_REQUESTED_ANNOTATION
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReclaimRequest {
    /// Value of [`crate::constants::RECLAIM_REASON_ANNOTATION`], if set.
    pub reason: Option<String>,
    /// Value of [`crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION`], if set.
    pub requested_at: Option<String>,
}

/// Parse a Node's annotations into a [`ReclaimRequest`]. Returns `None`
/// unless the trigger annotation is present and set to the literal
/// [`crate::constants::RECLAIM_REQUESTED_VALUE`] — any other value
/// (including absent, empty, or `"false"`) is explicitly treated as
/// not-requested to avoid partial-write foot-guns.
#[must_use]
pub fn node_reclaim_request(node: &k8s_openapi::api::core::v1::Node) -> Option<ReclaimRequest> {
    let annotations = node.metadata.annotations.as_ref()?;
    let triggered = annotations
        .get(crate::constants::RECLAIM_REQUESTED_ANNOTATION)
        .map(String::as_str)
        == Some(crate::constants::RECLAIM_REQUESTED_VALUE);
    if !triggered {
        return None;
    }
    Some(ReclaimRequest {
        reason: annotations
            .get(crate::constants::RECLAIM_REASON_ANNOTATION)
            .cloned(),
        requested_at: annotations
            .get(crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION)
            .cloned(),
    })
}

/// Build the merge-patch body that clears all three reclaim annotations
/// from a Node. Run as the final step of the emergency-reclaim path so
/// that a node re-joining the cluster later does not immediately
/// re-fire on stale annotations. Merge-patch semantics: a `null` value
/// deletes the key.
#[must_use]
pub fn build_clear_reclaim_patch() -> serde_json::Value {
    json!({
        "metadata": {
            "annotations": {
                crate::constants::RECLAIM_REQUESTED_ANNOTATION: serde_json::Value::Null,
                crate::constants::RECLAIM_REASON_ANNOTATION: serde_json::Value::Null,
                crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION: serde_json::Value::Null,
            }
        }
    })
}

/// Build the merge-patch body that disables the owning `ScheduledMachine`'s
/// schedule by setting `spec.schedule.enabled = false`. Run as part of the
/// emergency-reclaim handler **before** the annotation-clear step, so that
/// if the controller crashes after disabling the schedule but before
/// clearing the annotations, the next reconcile still sees the annotations
/// and retries the handler from the top (idempotent).
///
/// Rationale: without this flip, the next schedule window silently re-adds
/// the node to the cluster, the agent sees the user's still-running JVM,
/// and the eject→re-add→re-eject loop repeats every schedule boundary
/// forever. Setting `enabled=false` makes the user's explicit re-enable
/// the signal to return the node to service — see
/// `docs/roadmaps/5spot-emergency-reclaim-by-process-match.md` Phase 3 and
/// Open Question 6.
///
/// Merge-patch shape is deliberately narrow: only `spec.schedule.enabled`
/// is addressed. Siblings under `spec.schedule` (`daysOfWeek`,
/// `hoursOfDay`, `timezone`) and siblings under `spec` (`killSwitch`,
/// `killIfCommands`, `bootstrapSpec`, `infrastructureSpec`, ...) are
/// untouched.
#[must_use]
pub fn build_disable_schedule_patch() -> serde_json::Value {
    json!({
        "spec": {
            "schedule": {
                "enabled": false,
            }
        }
    })
}

// ============================================================================
// Emergency reclaim — event + condition-message builders (Phase 3 dispatch)
// ============================================================================

/// Build the `Warning`-type Kubernetes Event published on the
/// `ScheduledMachine` at the start of the emergency reclaim path.
///
/// Mirrors the audit shape of [`build_phase_transition_event`] but uses a
/// dedicated `action` string so event filters can target it explicitly:
/// `kubectl get events --field-selector reason=EmergencyReclaim`.
///
/// The note embeds the agent-supplied `reason` verbatim (e.g.
/// `"process-match: java"`) so an operator reading `kubectl describe`
/// sees the root cause without cross-referencing the node object.
#[must_use]
pub fn build_emergency_reclaim_event(request: &ReclaimRequest) -> KubeEvent {
    let reason_str = request
        .reason
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("reclaim trigger on Node");
    let note = match request.requested_at.as_deref() {
        Some(ts) if !ts.trim().is_empty() => {
            format!("Emergency reclaim requested ({ts}): {reason_str}")
        }
        _ => format!("Emergency reclaim requested: {reason_str}"),
    };
    KubeEvent {
        type_: EventType::Warning,
        reason: crate::constants::REASON_EMERGENCY_RECLAIM.to_string(),
        note: Some(note),
        action: "EmergencyReclaim".to_string(),
        secondary: None,
    }
}

/// Build the `Warning`-type Kubernetes Event published on the
/// `ScheduledMachine` immediately after the controller has flipped
/// `spec.schedule.enabled = false` as part of emergency reclaim.
///
/// Emitted separately from [`build_emergency_reclaim_event`] so the
/// `kubectl describe` timeline shows both the trigger and the
/// schedule-disable as distinct entries — operators need to notice that
/// the node will NOT return at the next schedule window without their
/// action.
#[must_use]
pub fn build_emergency_disable_schedule_event() -> KubeEvent {
    KubeEvent {
        type_: EventType::Warning,
        reason: crate::constants::REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE.to_string(),
        note: Some(
            "spec.schedule.enabled flipped to false to break the \
             eject→re-add→re-eject loop. Re-enable is a user action."
                .to_string(),
        ),
        action: "EmergencyReclaimDisableSchedule".to_string(),
        secondary: None,
    }
}

/// Build the `Warning`-type Kubernetes Event published on the
/// `ScheduledMachine` when the controller observes ≥
/// [`crate::constants::RAPID_RE_RECLAIM_THRESHOLD`] reclaim events for
/// the same SM within the configured window. The event tells the
/// operator their re-enable likely failed because the conflicting
/// process is still running — they should stop it before re-enabling
/// again, otherwise the loop will continue.
#[must_use]
pub fn build_rapid_re_reclaim_event(name: &str) -> KubeEvent {
    KubeEvent {
        type_: EventType::Warning,
        reason: crate::constants::REASON_RAPID_RE_RECLAIM.to_string(),
        note: Some(format!(
            "ScheduledMachine {name} has been emergency-reclaimed {} or more times within \
             {} seconds. The conflicting process is likely still running on the node — \
             stop it before re-enabling spec.schedule.enabled, or the agent will fire again \
             on the next reconcile.",
            crate::constants::RAPID_RE_RECLAIM_THRESHOLD,
            crate::constants::RAPID_RE_RECLAIM_WINDOW_SECS,
        )),
        action: "RapidReReclaim".to_string(),
        secondary: None,
    }
}

/// Format the human-readable message for the status condition recorded
/// when the `ScheduledMachine` enters `Phase::EmergencyRemove`.
///
/// Always includes the node name (the minimum floor); embeds the
/// agent-supplied reason and timestamp when present. Pure for unit-test
/// coverage — no I/O, no clock reads.
#[must_use]
pub fn emergency_reclaim_message(node_name: &str, request: &ReclaimRequest) -> String {
    let reason_part = request
        .reason
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|r| format!(": {r}"))
        .unwrap_or_default();
    let time_part = request
        .requested_at
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|t| format!(" at {t}"))
        .unwrap_or_default();
    format!("Emergency reclaim on node {node_name}{time_part}{reason_part}")
}

// ============================================================================
// Emergency reclaim — dispatch handler (Phase 3)
// ============================================================================

/// Execute the full node-driven emergency reclaim on a `ScheduledMachine`.
///
/// Invoked from [`scheduled_machine::reconcile_inner`](super::scheduled_machine)
/// once the controller has observed
/// [`RECLAIM_REQUESTED_ANNOTATION`](crate::constants::RECLAIM_REQUESTED_ANNOTATION)
/// on the Node backing this resource.
///
/// # Ordering contract
/// Executes the seven steps documented in the Phase-3 roadmap in order:
/// 1. Emit the `EmergencyReclaim` Warning event on the `ScheduledMachine`.
/// 2. Transition phase to [`PHASE_EMERGENCY_REMOVE`].
/// 3. Drain the Node with the short
///    [`EMERGENCY_DRAIN_TIMEOUT_SECS`] — failures are **logged**, not
///    fatal, so a misbehaving PDB cannot stall the reclaim.
/// 4. Delete the CAPI `Machine` (removes node from cluster).
/// 5. PATCH `spec.schedule.enabled = false` via
///    [`build_disable_schedule_patch`]; emit
///    `EmergencyReclaimDisabledSchedule` event.
/// 6. Clear the three reclaim annotations on the Node via
///    [`build_clear_reclaim_patch`] — best-effort; failure here only means
///    the next reconcile replays from step 1 idempotently.
/// 7. Transition phase to [`PHASE_DISABLED`].
///
/// # Idempotence
/// Each step is safe to re-run. If the controller crashes after step 4
/// but before step 5, the next reconcile still observes the annotation
/// and replays — the `schedule.enabled=false` PATCH is idempotent (no
/// diff if already false). We deliberately disable the schedule *before*
/// clearing annotations so that a crash between steps 5 and 6 leaves
/// the annotation in place and the next reconcile retries from the top.
///
/// # Errors
/// - [`ReconcilerError::InvalidConfig`] — resource has no namespace
/// - [`ReconcilerError::KubeError`] — status PATCH or spec PATCH failed
///   (those errors **do** abort the flow so the caller back-offs and
///   retries, because the `enabled=false` flip is the loop-breaker and
///   cannot be silently skipped)
/// - [`ReconcilerError::CapiError`] — CAPI Machine deletion failed
pub async fn handle_emergency_remove(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
    node_name: &str,
    request: &ReclaimRequest,
) -> Result<Action, ReconcilerError> {
    use crate::constants::{
        EMERGENCY_DRAIN_TIMEOUT_SECS, PHASE_DISABLED, PHASE_EMERGENCY_REMOVE,
        REASON_EMERGENCY_RECLAIM, REASON_SCHEDULE_DISABLED,
    };

    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();
    let from_phase = resource.status.as_ref().and_then(|s| s.phase.as_deref());

    info!(
        resource = %name,
        namespace = %namespace,
        node = %node_name,
        reason = request.reason.as_deref().unwrap_or("(none)"),
        "Emergency reclaim path engaged"
    );

    let sm_object_ref = ObjectReference {
        api_version: Some(crate::constants::API_VERSION_FULL.to_string()),
        kind: Some(crate::constants::KIND_SCHEDULED_MACHINE.to_string()),
        name: Some(name.clone()),
        namespace: Some(namespace.clone()),
        ..Default::default()
    };

    // Step 1: emit EmergencyReclaim Warning event on the SM.
    publish_best_effort(
        &ctx,
        &sm_object_ref,
        build_emergency_reclaim_event(request),
        "EmergencyReclaim",
    )
    .await;

    // Step 2: transition phase to EmergencyRemove.
    let condition_message = emergency_reclaim_message(node_name, request);
    update_phase(
        &ctx,
        &namespace,
        &name,
        from_phase,
        PHASE_EMERGENCY_REMOVE,
        Some(REASON_EMERGENCY_RECLAIM),
        Some(&condition_message),
        false,
    )
    .await?;

    // Per-SM emergency-reclaim counter. Bumped once per *flow* (the
    // idempotent recovery handler short-circuits before this point on
    // restart) so a controller crash mid-flow does not double-count.
    record_emergency_reclaim(&namespace, &name);

    // Loop-protection: append this reclaim's timestamp to the per-SM
    // deque in Context, prune entries outside the window, and if the
    // post-append count crosses the threshold, emit a RapidReReclaim
    // Warning event + bump the metric. The tracker is in-memory only
    // (see crate::loop_protection module rustdoc for rationale).
    let breach = {
        let key = format!("{namespace}/{name}");
        let now_ts = chrono::Utc::now();
        let mut lock = ctx.recent_reclaims.lock().expect("recent_reclaims mutex");
        let deque = lock.entry(key).or_default();
        crate::loop_protection::record_emergency_reclaim_event(deque, now_ts)
    };
    if breach {
        warn!(
            resource = %name,
            namespace = %namespace,
            "Rapid-re-reclaim threshold crossed — emitting RapidReReclaim Warning Event"
        );
        crate::metrics::record_rapid_re_reclaim(&namespace, &name);
        publish_best_effort(
            &ctx,
            &sm_object_ref,
            build_rapid_re_reclaim_event(&name),
            "RapidReReclaim",
        )
        .await;
    }

    // Step 3: drain with short emergency timeout. Best-effort — we do
    // NOT block Machine deletion on a failed drain, because the agent
    // has already decided the node must leave. The stopwatch is
    // observed regardless of outcome so operators can compute success
    // P95 vs timeout-rate side by side.
    let drain_start = std::time::Instant::now();
    let drain_result = drain_node_with_timeout(
        &ctx.client,
        node_name,
        Duration::from_secs(EMERGENCY_DRAIN_TIMEOUT_SECS),
    )
    .await;
    let drain_outcome = match &drain_result {
        Ok(()) => EmergencyDrainOutcome::Success,
        Err(e) if e.to_string().contains("timeout") => EmergencyDrainOutcome::Timeout,
        Err(_) => EmergencyDrainOutcome::Error,
    };
    record_emergency_drain(drain_start.elapsed().as_secs_f64(), drain_outcome);
    if let Err(e) = drain_result {
        warn!(
            resource = %name,
            node = %node_name,
            error = %e,
            "Emergency drain failed or timed out — proceeding with Machine deletion"
        );
    }

    // Step 4: delete the CAPI Machine. This one we DO propagate — if we
    // cannot delete the Machine, the node remains in the cluster and
    // the loop-breaker (step 5) would be premature.
    remove_machine_from_cluster(&resource, &ctx.client, &namespace).await?;

    // Step 5: disable the schedule — the load-bearing step that breaks
    // the eject→re-add→re-eject loop.
    patch_disable_schedule(&ctx.client, &namespace, &name).await?;
    publish_best_effort(
        &ctx,
        &sm_object_ref,
        build_emergency_disable_schedule_event(),
        "EmergencyReclaimDisabledSchedule",
    )
    .await;

    // Step 6: clear reclaim annotations. Best-effort — failure only
    // triggers an idempotent replay on the next reconcile.
    clear_reclaim_annotations_best_effort(&ctx.client, node_name).await;

    // Step 7: finalise state machine transition to Disabled.
    update_phase(
        &ctx,
        &namespace,
        &name,
        Some(PHASE_EMERGENCY_REMOVE),
        PHASE_DISABLED,
        Some(REASON_SCHEDULE_DISABLED),
        Some("Schedule disabled by emergency reclaim; re-enable is a user action"),
        false,
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Publish a Kubernetes Event, logging at `warn` on failure. Used for
/// best-effort audit events where a missing event must not abort the
/// reconcile.
async fn publish_best_effort(
    ctx: &Context,
    object_ref: &ObjectReference,
    event: KubeEvent,
    tag: &str,
) {
    if let Err(e) = ctx.recorder.publish(&event, object_ref).await {
        warn!(error = %e, tag = %tag, "Failed to record Kubernetes Event (audit trail incomplete)");
    }
}

/// Merge-patch `spec.schedule.enabled = false` onto the named
/// `ScheduledMachine`. Propagates any error so the caller back-offs —
/// this is the loop-breaker and cannot silently no-op.
async fn patch_disable_schedule(
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<(), ReconcilerError> {
    let sms: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);
    let disable_patch = build_disable_schedule_patch();
    sms.patch(name, &PatchParams::default(), &Patch::Merge(&disable_patch))
        .await
        .map_err(|e| {
            error!(
                resource = %name,
                namespace = %namespace,
                error = %e,
                "Failed to PATCH spec.schedule.enabled=false — reclaim loop not broken; retrying"
            );
            ReconcilerError::KubeError(e)
        })?;
    Ok(())
}

/// Best-effort clear of the three reclaim annotations on the node.
/// Failures are logged but swallowed — the next reconcile replays
/// idempotently if the annotations are still present.
async fn clear_reclaim_annotations_best_effort(client: &Client, node_name: &str) {
    let nodes: Api<k8s_openapi::api::core::v1::Node> = Api::all(client.clone());
    let clear_patch = build_clear_reclaim_patch();
    if let Err(e) = nodes
        .patch(
            node_name,
            &PatchParams::default(),
            &Patch::Merge(&clear_patch),
        )
        .await
    {
        warn!(
            node = %node_name,
            error = %e,
            "Failed to clear reclaim annotations on Node — next reconcile will replay idempotently"
        );
    }
}

// ============================================================================
// Emergency reclaim — controller-side agent provisioning (Phase 2.5 remainder)
// ============================================================================
//
// When a `ScheduledMachine`'s `spec.killIfCommands` is non-empty, the
// controller must mirror the user's declared intent into two cluster
// objects on the child cluster:
//
// 1. A label `RECLAIM_AGENT_LABEL=RECLAIM_AGENT_LABEL_ENABLED` on each
//    backing Node so the opt-in DaemonSet's `nodeSelector` matches and
//    the agent pod lands.
// 2. A per-node `ConfigMap` named `reclaim-agent-<node-name>` in
//    `RECLAIM_AGENT_NAMESPACE` carrying a single `reclaim.toml` key
//    whose body is the rendered TOML shape the agent parses at startup.
//
// Clearing `killIfCommands` back to empty strips the label (which
// evicts the DaemonSet pod) and deletes the ConfigMap. The pure
// builders below are unit-tested; the async orchestrator underneath
// wires them to the kube API.

/// Render the `spec.killIfCommands` list into the TOML shape consumed
/// by `reclaim_agent::parse_config`. The output is guaranteed to be
/// valid input for that parser — that round-trip is pinned by unit
/// tests so a rename or format-drift here surfaces immediately.
///
/// The argv-substring list is intentionally not exposed on the CRD
/// (see roadmap Phase 2.5) so this renderer always emits an empty
/// `match_argv_substrings`. The poll interval is fixed to the agent's
/// default so a spec change cannot tune the loop.
#[must_use]
pub fn render_reclaim_toml(commands: &[String]) -> String {
    use toml::Value;
    let cmds: Vec<Value> = commands.iter().map(|c| Value::String(c.clone())).collect();
    let mut table = toml::map::Map::new();
    table.insert("match_commands".to_string(), Value::Array(cmds));
    table.insert(
        "match_argv_substrings".to_string(),
        Value::Array(Vec::new()),
    );
    table.insert(
        "poll_interval_ms".to_string(),
        Value::Integer(crate::reclaim_agent::DEFAULT_POLL_INTERVAL_MS as i64),
    );
    let body = toml::to_string(&Value::Table(table))
        .expect("fixed-shape TOML value must always serialize");
    format!(
        "# Auto-generated by 5-spot controller from spec.killIfCommands\n\
         # DO NOT EDIT — changes will be overwritten on next reconcile.\n\
         {body}"
    )
}

/// Return the `ConfigMap` name for the reclaim agent on a given node:
/// `reclaim-agent-<node-name>`. Node names are DNS-1123 by kubelet
/// contract so no sanitisation is applied — the projected name must be
/// guessable from the node identity for debugging and for a future
/// agent-side runtime ConfigMap fetch.
#[must_use]
pub fn per_node_configmap_name(node_name: &str) -> String {
    format!(
        "{prefix}{node_name}",
        prefix = crate::constants::RECLAIM_AGENT_CONFIGMAP_PREFIX,
    )
}

/// Build the per-node reclaim-agent `ConfigMap` containing a single
/// `reclaim.toml` data key rendered from the commands list. Operator
/// discovery labels are stamped so
/// `kubectl get cm -n 5spot-system -l app.kubernetes.io/component=reclaim-agent`
/// lists every projection at a glance.
#[must_use]
pub fn build_reclaim_agent_configmap(
    node_name: &str,
    commands: &[String],
) -> k8s_openapi::api::core::v1::ConfigMap {
    use k8s_openapi::api::core::v1::ConfigMap;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), "5spot".to_string());
    labels.insert(
        "app.kubernetes.io/component".to_string(),
        "reclaim-agent".to_string(),
    );
    let mut data = BTreeMap::new();
    data.insert(
        crate::constants::RECLAIM_CONFIG_DATA_KEY.to_string(),
        render_reclaim_toml(commands),
    );
    ConfigMap {
        metadata: ObjectMeta {
            name: Some(per_node_configmap_name(node_name)),
            namespace: Some(crate::constants::RECLAIM_AGENT_NAMESPACE.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    }
}

/// Build the merge-patch body that sets (when `enable == true`) or
/// clears (when `enable == false`) the reclaim-agent opt-in label on a
/// `Node`. Clearing uses JSON `null` so merge-patch deletes the key —
/// an empty string would leave the label set to `""` and the DaemonSet
/// nodeSelector would still not match, but the label would linger as
/// noise on `kubectl describe node`.
#[must_use]
pub fn build_reclaim_agent_label_patch(enable: bool) -> serde_json::Value {
    let value = if enable {
        serde_json::Value::String(crate::constants::RECLAIM_AGENT_LABEL_ENABLED.to_string())
    } else {
        serde_json::Value::Null
    };
    json!({
        "metadata": {
            "labels": {
                crate::constants::RECLAIM_AGENT_LABEL: value,
            }
        }
    })
}

/// Drive the reclaim-agent provisioning for a single Node based on the
/// `killIfCommands` list:
///
/// - Non-empty → stamp the opt-in label on the Node and apply the
///   per-node `ConfigMap` (idempotent server-side apply).
/// - Empty → clear the opt-in label and delete the per-node `ConfigMap`
///   (idempotent — 404 is benign).
///
/// Best-effort with respect to the reconcile loop: individual failures
/// are logged and returned as [`ReconcilerError::KubeError`] only on a
/// genuine API failure; 404 on the tear-down path is treated as
/// success so a re-run after a partial prior tear-down completes
/// cleanly.
///
/// # Errors
/// Returns [`ReconcilerError::KubeError`] when a Node PATCH or
/// ConfigMap apply/delete fails with a non-404 error.
pub async fn reconcile_reclaim_agent_provision(
    client: &Client,
    node_name: &str,
    commands: &[String],
) -> Result<(), ReconcilerError> {
    use k8s_openapi::api::core::v1::{ConfigMap, Node};
    const FIELD_MANAGER: &str = "5spot-controller-reclaim-agent";

    let label_patch = build_reclaim_agent_label_patch(!commands.is_empty());
    let nodes: Api<Node> = Api::all(client.clone());
    nodes
        .patch(
            node_name,
            &PatchParams::default(),
            &Patch::Merge(&label_patch),
        )
        .await
        .map_err(|e| {
            error!(
                node = %node_name,
                enable = !commands.is_empty(),
                error = %e,
                "Failed to patch reclaim-agent label on Node"
            );
            ReconcilerError::KubeError(e)
        })?;

    let cms: Api<ConfigMap> =
        Api::namespaced(client.clone(), crate::constants::RECLAIM_AGENT_NAMESPACE);
    let cm_name = per_node_configmap_name(node_name);

    if commands.is_empty() {
        match cms.delete(&cm_name, &Default::default()).await {
            Ok(_) => {
                debug!(
                    node = %node_name,
                    configmap = %cm_name,
                    "Deleted per-node reclaim-agent ConfigMap"
                );
            }
            Err(kube::Error::Api(e)) if e.code == 404 => {
                debug!(
                    node = %node_name,
                    configmap = %cm_name,
                    "ConfigMap already absent — tear-down idempotent"
                );
            }
            Err(e) => {
                error!(
                    node = %node_name,
                    configmap = %cm_name,
                    error = %e,
                    "Failed to delete per-node reclaim-agent ConfigMap"
                );
                return Err(ReconcilerError::KubeError(e));
            }
        }
        return Ok(());
    }

    let cm = build_reclaim_agent_configmap(node_name, commands);
    let apply_params = PatchParams::apply(FIELD_MANAGER).force();
    cms.patch(&cm_name, &apply_params, &Patch::Apply(&cm))
        .await
        .map_err(|e| {
            error!(
                node = %node_name,
                configmap = %cm_name,
                error = %e,
                "Failed to apply per-node reclaim-agent ConfigMap"
            );
            ReconcilerError::KubeError(e)
        })?;
    debug!(
        node = %node_name,
        configmap = %cm_name,
        commands = ?commands,
        "Applied per-node reclaim-agent ConfigMap"
    );
    Ok(())
}

// ============================================================================
// Node taint diff + apply (Phase 3 of user-defined-node-taints roadmap)
// ============================================================================

/// Plan computed by [`diff_node_taints`] describing exactly how a Node's
/// `spec.taints` list needs to change to reach the desired state while
/// respecting admin-owned taints.
///
/// The plan is a pure data structure — it's the inputs that [`apply_node_taints`]
/// then turns into a single `PATCH /api/v1/nodes/<name>` round-trip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeTaintPlan {
    /// Taints in `desired` that are not present on the current Node.
    pub to_add: Vec<crate::crd::NodeTaint>,

    /// Taints on the current Node (that we previously applied) whose value has
    /// drifted from `desired`. Only taints we own — admin-added collisions go
    /// into `conflicts` instead.
    pub to_update: Vec<crate::crd::NodeTaint>,

    /// Taints we previously applied that are absent from `desired`. Bounded by
    /// `previously_applied ∩ current` and further restricted to cases where
    /// the current Node value still matches what we applied — if the admin
    /// mutated it, we surface a conflict instead of silently overwriting.
    pub to_remove: Vec<crate::crd::NodeTaint>,

    /// Desired taints already present on the Node with matching value.
    /// Populated to give the caller a precise "no-op" check via [`Self::is_noop`].
    pub unchanged: Vec<crate::crd::NodeTaint>,

    /// Entries from `desired` (or `previously_applied`) that collide with an
    /// admin-added taint on `(key, effect)`. The controller will NOT overwrite
    /// these — surface a `TaintOwnershipConflict` condition instead.
    pub conflicts: Vec<crate::crd::NodeTaint>,
}

impl NodeTaintPlan {
    /// `true` when the plan has zero mutating entries — the caller can skip
    /// the PATCH round-trip entirely.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.to_add.is_empty() && self.to_update.is_empty() && self.to_remove.is_empty()
    }
}

/// Compute the taint-reconciliation plan without performing any IO.
///
/// Identity is the tuple `(key, effect)` — value is mutable. See
/// [`NodeTaintPlan`] for the exact semantics of each output bucket.
///
/// A current taint whose `effect` string doesn't match any [`crate::crd::TaintEffect`]
/// variant (because an admin applied it with an unknown effect, or a future
/// Kubernetes version added a new one) is treated as opaque admin state and
/// left alone.
#[must_use]
pub fn diff_node_taints(
    current: &[k8s_openapi::api::core::v1::Taint],
    desired: &[crate::crd::NodeTaint],
    previously_applied: &[crate::crd::NodeTaint],
) -> NodeTaintPlan {
    use crate::crd::{NodeTaint, TaintEffect};

    fn effect_from_str(s: &str) -> Option<TaintEffect> {
        match s {
            "NoSchedule" => Some(TaintEffect::NoSchedule),
            "PreferNoSchedule" => Some(TaintEffect::PreferNoSchedule),
            "NoExecute" => Some(TaintEffect::NoExecute),
            _ => None,
        }
    }

    fn find_current<'a>(
        current: &'a [k8s_openapi::api::core::v1::Taint],
        key: &str,
        effect: &TaintEffect,
    ) -> Option<&'a k8s_openapi::api::core::v1::Taint> {
        current
            .iter()
            .find(|c| c.key == key && effect_from_str(&c.effect).as_ref() == Some(effect))
    }

    fn find_applied<'a>(
        previously_applied: &'a [NodeTaint],
        key: &str,
        effect: &TaintEffect,
    ) -> Option<&'a NodeTaint> {
        previously_applied
            .iter()
            .find(|p| p.key == key && &p.effect == effect)
    }

    let mut plan = NodeTaintPlan::default();

    for want in desired {
        match find_current(current, &want.key, &want.effect) {
            None => plan.to_add.push(want.clone()),
            Some(cur) => {
                let cur_value = cur.value.as_deref();
                let want_value = want.value.as_deref();
                if cur_value == want_value {
                    plan.unchanged.push(want.clone());
                    continue;
                }
                let we_own_current = find_applied(previously_applied, &want.key, &want.effect)
                    .is_some_and(|p| p.value.as_deref() == cur_value);
                if we_own_current {
                    plan.to_update.push(want.clone());
                } else {
                    plan.conflicts.push(want.clone());
                }
            }
        }
    }

    for applied in previously_applied {
        let still_desired = desired
            .iter()
            .any(|d| d.key == applied.key && d.effect == applied.effect);
        if still_desired {
            continue;
        }
        let Some(cur) = find_current(current, &applied.key, &applied.effect) else {
            continue;
        };
        if cur.value.as_deref() == applied.value.as_deref() {
            plan.to_remove.push(applied.clone());
        } else {
            plan.conflicts.push(applied.clone());
        }
    }

    plan
}

/// Apply a [`NodeTaintPlan`] to the named Node via a single server-side apply
/// PATCH, and write the `5spot.finos.org/applied-taints` ownership annotation.
///
/// Reads the Node's current `spec.taints` first, rebuilds the target list by
/// (a) dropping entries in `plan.to_remove`, (b) overriding entries with the
/// new value for `plan.to_update`, and (c) appending `plan.to_add`, then
/// PATCHes the whole list under the
/// [`NODE_TAINT_FIELD_MANAGER`](crate::constants::NODE_TAINT_FIELD_MANAGER)
/// field-manager so our ownership is explicit in `managedFields`.
///
/// # Errors
/// Returns [`ReconcilerError::KubeError`] on any API failure. A no-op plan
/// returns `Ok(())` without touching the network.
pub async fn apply_node_taints(
    client: &Client,
    node_name: &str,
    plan: &NodeTaintPlan,
) -> Result<(), ReconcilerError> {
    use crate::constants::{APPLIED_TAINTS_ANNOTATION, NODE_TAINT_FIELD_MANAGER};
    use crate::crd::TaintEffect;
    use k8s_openapi::api::core::v1::{Node, Taint};

    if plan.is_noop() {
        debug!(node = %node_name, "Node taint plan is a no-op; skipping PATCH");
        return Ok(());
    }

    let nodes: Api<Node> = Api::all(client.clone());
    let current_node = nodes.get(node_name).await.map_err(|e| {
        error!(node = %node_name, error = %e, "Failed to GET Node for taint apply");
        ReconcilerError::KubeError(e)
    })?;

    let current_taints = current_node
        .spec
        .as_ref()
        .and_then(|s| s.taints.clone())
        .unwrap_or_default();

    fn effect_str(e: &TaintEffect) -> &'static str {
        match e {
            TaintEffect::NoSchedule => "NoSchedule",
            TaintEffect::PreferNoSchedule => "PreferNoSchedule",
            TaintEffect::NoExecute => "NoExecute",
        }
    }

    fn matches(t: &Taint, key: &str, effect: &TaintEffect) -> bool {
        t.key == key && t.effect == effect_str(effect)
    }

    let mut target: Vec<Taint> = current_taints
        .into_iter()
        .filter(|t| !plan.to_remove.iter().any(|r| matches(t, &r.key, &r.effect)))
        .map(|t| {
            if let Some(update) = plan
                .to_update
                .iter()
                .find(|u| matches(&t, &u.key, &u.effect))
            {
                Taint {
                    key: update.key.clone(),
                    value: update.value.clone(),
                    effect: effect_str(&update.effect).to_string(),
                    time_added: None,
                }
            } else {
                t
            }
        })
        .collect();

    for want in &plan.to_add {
        target.push(Taint {
            key: want.key.clone(),
            value: want.value.clone(),
            effect: effect_str(&want.effect).to_string(),
            time_added: None,
        });
    }

    let owned: Vec<serde_json::Value> = plan
        .to_add
        .iter()
        .chain(plan.to_update.iter())
        .chain(plan.unchanged.iter())
        .map(|t| serde_json::json!({"key": t.key, "effect": effect_str(&t.effect)}))
        .collect();
    let annotation_value = serde_json::to_string(&owned).unwrap_or_else(|_| "[]".to_string());

    let patch = json!({
        "metadata": {
            "annotations": {
                APPLIED_TAINTS_ANNOTATION: annotation_value,
            }
        },
        "spec": {
            "taints": target,
        }
    });

    let params = PatchParams::apply(NODE_TAINT_FIELD_MANAGER).force();
    nodes
        .patch(node_name, &params, &Patch::Apply(&patch))
        .await
        .map_err(|e| {
            error!(node = %node_name, error = %e, "Failed to PATCH Node taints");
            ReconcilerError::KubeError(e)
        })?;

    info!(
        node = %node_name,
        added = plan.to_add.len(),
        updated = plan.to_update.len(),
        removed = plan.to_remove.len(),
        "Applied Node taints"
    );
    Ok(())
}

// ============================================================================
// Node taint orchestration (Phase 4 of user-defined-node-taints roadmap)
// ============================================================================

/// Input bundle for [`reconcile_node_taints`]. Grouped into a struct so the
/// function signature stays short and clippy-happy.
#[derive(Clone, Copy)]
pub struct ReconcileNodeTaintsInput<'a> {
    /// Name of the Node this `ScheduledMachine` is bound to (via `status.nodeRef`).
    pub node_name: &'a str,
    /// Desired taints from `ScheduledMachineSpec.node_taints`.
    pub desired: &'a [crate::crd::NodeTaint],
    /// Taints the controller has previously applied (from `status.appliedNodeTaints`).
    pub previously_applied: &'a [crate::crd::NodeTaint],
}

/// Result of one `reconcile_node_taints` call. The caller turns this into a
/// `NodeTainted` condition update (see `CONDITION_TYPE_NODE_TAINTED` reasons).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeTaintReconcileOutcome {
    /// Node API returned 404 — CAPI has a nodeRef but the Node object isn't
    /// materialised yet. Caller should surface `NodeTainted=Unknown/NoNodeYet`.
    NoNodeYet,
    /// Node exists but `Ready != True` — caller should surface
    /// `NodeTainted=False/NodeNotReady` and rely on the Node watch to re-enqueue.
    NodeNotReady,
    /// The node taint plan ran to completion (possibly no-op). `applied` is
    /// the new `status.appliedNodeTaints` value the caller should persist.
    Applied { applied: Vec<crate::crd::NodeTaint> },
    /// One or more `(key, effect)` pairs collide with admin-owned taints.
    /// Caller should surface `NodeTainted=False/TaintOwnershipConflict` and
    /// stop retrying until spec changes.
    Conflict {
        conflicts: Vec<crate::crd::NodeTaint>,
    },
}

/// Orchestrate the taint reconcile for a single Node: GET the Node, verify
/// `Ready`, compute the plan, and (if non-no-op) PATCH.
///
/// This is the function `handle_active_phase` calls once `status.nodeRef` is
/// populated. It is deliberately non-async-recursive and returns a structured
/// outcome rather than a naked `Result` — callers need to both surface a
/// condition *and* know whether to requeue short (transient) or long (stable).
///
/// # Errors
/// Returns [`ReconcilerError::KubeError`] only for non-404 GET failures and
/// for PATCH failures. A 404 on the Node is mapped to
/// [`NodeTaintReconcileOutcome::NoNodeYet`]; not-Ready is
/// [`NodeTaintReconcileOutcome::NodeNotReady`]; admin conflicts go into
/// [`NodeTaintReconcileOutcome::Conflict`]. None of those are errors.
pub async fn reconcile_node_taints(
    client: &Client,
    input: ReconcileNodeTaintsInput<'_>,
) -> Result<NodeTaintReconcileOutcome, ReconcilerError> {
    use k8s_openapi::api::core::v1::Node;

    let nodes: Api<Node> = Api::all(client.clone());
    let node = match nodes.get(input.node_name).await {
        Ok(n) => n,
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(node = %input.node_name, "Node not materialised yet — NoNodeYet");
            return Ok(NodeTaintReconcileOutcome::NoNodeYet);
        }
        Err(e) => {
            error!(node = %input.node_name, error = %e, "Failed to GET Node for taint reconcile");
            return Err(ReconcilerError::KubeError(e));
        }
    };

    let is_ready = node
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|cs| cs.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
        .unwrap_or(false);
    if !is_ready {
        debug!(node = %input.node_name, "Node not Ready yet — deferring taint apply");
        return Ok(NodeTaintReconcileOutcome::NodeNotReady);
    }

    let current_taints = node
        .spec
        .as_ref()
        .and_then(|s| s.taints.clone())
        .unwrap_or_default();
    let plan = diff_node_taints(&current_taints, input.desired, input.previously_applied);

    if !plan.conflicts.is_empty() {
        warn!(
            node = %input.node_name,
            conflicts = plan.conflicts.len(),
            "Node taint ownership conflict — refusing to overwrite admin-owned taints"
        );
        return Ok(NodeTaintReconcileOutcome::Conflict {
            conflicts: plan.conflicts,
        });
    }

    apply_node_taints(client, input.node_name, &plan).await?;

    let applied: Vec<_> = input.desired.to_vec();
    Ok(NodeTaintReconcileOutcome::Applied { applied })
}

/// Patch `ScheduledMachine.status.appliedNodeTaints` to the given list.
///
/// Thin wrapper around the status subresource using a Merge patch, mirroring
/// [`patch_machine_refs_status`]. No-op when `applied` equals the caller's
/// belief — the caller should only invoke this when the list has changed.
///
/// # Errors
/// Returns [`ReconcilerError`] on patch failure.
pub async fn patch_applied_node_taints_status(
    client: &Client,
    namespace: &str,
    name: &str,
    applied: &[crate::crd::NodeTaint],
) -> Result<(), ReconcilerError> {
    let patch = json!({
        "status": {
            "appliedNodeTaints": applied,
        }
    });
    let api: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);
    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    debug!(
        resource = %name,
        namespace = %namespace,
        count = applied.len(),
        "Patched status.appliedNodeTaints"
    );
    Ok(())
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod helpers_tests;
