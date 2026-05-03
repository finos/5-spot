// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
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
//! Any state ──► Terminated       (kill switch)
//! Any state ──► EmergencyRemove  (node-side reclaim annotation)
//! EmergencyRemove ──► Disabled   (after drain + delete + enabled=false flip)
//! Any state ──► Error            (unrecoverable failure)
//! Error     ──► Pending          (automatic recovery attempt)
//! ```
//!
//! ## Entry points
//! - [`reconcile_scheduled_machine`] — called by the `kube-rs` controller loop
//! - [`crate::reconcilers::error_policy`] — determines requeue interval after a reconciliation error
//!
//! ## Multi-instance distribution
//! When `instance_count > 1`, each resource is deterministically assigned to
//! one instance via consistent hashing on `namespace/name`.  Instances that
//! are not assigned to a resource skip it with `Action::await_change`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
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
use tracing::{debug, error, info, warn, Instrument};

use crate::constants::{
    ERROR_REQUEUE_SECS, PHASE_ACTIVE, PHASE_DISABLED, PHASE_EMERGENCY_REMOVE, PHASE_ERROR,
    PHASE_INACTIVE, PHASE_PENDING, PHASE_SHUTTING_DOWN, PHASE_TERMINATED, REASON_GRACE_PERIOD,
    REASON_MACHINE_CREATED, REASON_MACHINE_DELETED, REASON_SCHEDULE_ACTIVE,
    REASON_SCHEDULE_DISABLED, REASON_SCHEDULE_INACTIVE, TIMER_REQUEUE_SECS,
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
    /// Whether this instance currently holds the leader lease.
    ///
    /// Always `true` when leader election is disabled (every instance acts as
    /// leader).  When leader election is enabled, `main.rs` sets this to `false`
    /// at startup and the [`LeaseManager`](kube_lease_manager::LeaseManager)
    /// background task flips it to `true` once the lease is acquired.
    ///
    /// `reconcile_guarded` checks this flag before doing any work; non-leaders
    /// return `Action::await_change()` immediately so they can react quickly when
    /// they do become the leader.
    pub is_leader: Arc<AtomicBool>,
    /// Per-resource reconciliation error counts, keyed by `"namespace/name"`.
    ///
    /// Incremented by [`error_policy`](crate::reconcilers::error_policy) on each
    /// failure and cleared by `reconcile_guarded` on success, so back-off resets
    /// after the resource recovers.  Uses a `std::sync::Mutex` because
    /// `error_policy` is synchronous.
    pub retry_counts: Arc<Mutex<HashMap<String, u32>>>,
    /// Recent emergency-reclaim event timestamps per `"namespace/name"`.
    ///
    /// Tracked in-memory only — see [`crate::loop_protection`] for the
    /// reasoning. The reconciler appends a timestamp on every successful
    /// entry into the `EmergencyRemove` flow and emits a `RapidReReclaim`
    /// Warning Event when the deque length within
    /// [`crate::constants::RAPID_RE_RECLAIM_WINDOW_SECS`] crosses
    /// [`crate::constants::RAPID_RE_RECLAIM_THRESHOLD`]. Capped at
    /// [`crate::constants::RAPID_RE_RECLAIM_MAX_TRACKED`] entries per SM
    /// to bound worst-case allocation.
    pub recent_reclaims:
        Arc<Mutex<HashMap<String, std::collections::VecDeque<chrono::DateTime<chrono::Utc>>>>>,
    /// When `true` (default), `handle_deletion` force-removes the finalizer if
    /// CAPI cleanup exceeds [`crate::constants::FINALIZER_CLEANUP_TIMEOUT_SECS`],
    /// surfacing a `FinalizerCleanupTimedOut` Warning event and incrementing
    /// `fivespot_finalizer_cleanup_timeouts_total`. Trade-off: namespace
    /// deletion is unblocked but orphan CAPI resources are possible.
    ///
    /// Operators who require strict-cleanup-or-stall semantics can set
    /// `--force-finalizer-on-timeout=false` (env: `FORCE_FINALIZER_ON_TIMEOUT`)
    /// to revert to the original behaviour of returning `TimeoutError` and
    /// keeping the finalizer in place. Use only when an external sweep is in
    /// place to garbage-collect stuck SMs.
    pub force_finalizer_on_timeout: bool,
}

impl Context {
    /// Create a new `Context`, initialising the event recorder from the client.
    ///
    /// The recorder uses `POD_NAME` from the environment (injected via the
    /// downward API) as the reporting instance name — optional; falls back
    /// to `None` when unset (e.g. local `cargo run`).
    #[must_use]
    pub fn new(client: Client, instance_id: u32, instance_count: u32) -> Self {
        let reporter = Reporter {
            controller: CONTROLLER_NAME.to_string(),
            instance: std::env::var("POD_NAME").ok(),
        };
        let recorder = Recorder::new(client.clone(), reporter);
        Self {
            client,
            instance_id,
            instance_count,
            recorder,
            retry_counts: Arc::new(Mutex::new(HashMap::new())),
            recent_reclaims: Arc::new(Mutex::new(HashMap::new())),
            // Default to true so existing single-instance deployments without
            // leader election continue to work without any configuration change.
            is_leader: Arc::new(AtomicBool::new(true)),
            // Default-true: prefer unblocking namespace deletion over
            // strict-cleanup. See field rustdoc for the trade-off.
            force_finalizer_on_timeout: true,
        }
    }

    /// Override the [`Self::force_finalizer_on_timeout`] flag after
    /// construction. Used by `main.rs` to wire the CLI / env var.
    #[must_use]
    pub fn with_force_finalizer_on_timeout(mut self, force: bool) -> Self {
        self.force_finalizer_on_timeout = force;
        self
    }
}

// ============================================================================
// Error types
// ============================================================================

/// All errors that can be returned by the reconciliation path.
///
/// Variants map to specific Prometheus error label values recorded via
/// [`record_error`] so each failure mode is
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

    // Leader election guard — non-leaders do nothing and wait for a state change.
    // This lets standby replicas react quickly the moment they acquire the lease
    // without hammering the API server while waiting (Basel III HA / NIST SI-2).
    if !ctx.is_leader.load(Ordering::Acquire) {
        debug!(
            resource = %name,
            namespace = %namespace,
            "Not the leader — skipping reconciliation"
        );
        return Ok(Action::await_change());
    }

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

    // Defence-in-depth input validation. ValidatingAdmissionPolicy is the
    // first line of defence (rejected at CREATE/UPDATE), but clusters that
    // have not enabled VAP still need these bounds enforced. Runs early so
    // every downstream phase handler sees a sanitised spec — notably
    // cluster_name before any log/metric emission, killIfCommands before
    // the reclaim-agent projection.
    super::helpers::validate_cluster_name(&resource.spec.cluster_name)?;
    super::helpers::validate_kill_if_commands(resource.spec.kill_if_commands.as_deref())?;

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

    // Check node-driven emergency reclaim. Runs AFTER kill_switch (which is
    // terminal) but BEFORE schedule evaluation, so that a still-enabled
    // schedule cannot race the annotation observation and re-add the node.
    if let Some(action) = check_emergency_reclaim(&resource, &ctx).await? {
        return Ok(action);
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
        PHASE_EMERGENCY_REMOVE => handle_emergency_remove_phase(resource, ctx).await,
        PHASE_ERROR => handle_error_phase(resource, ctx),
        // PHASE_PENDING and unknown phases handled by default
        _ => handle_pending_phase(resource, ctx, should_be_active).await,
    }
}

/// Check whether the Node backing this `ScheduledMachine` carries a
/// reclaim-request annotation and — if so — drive the full emergency
/// remove flow.
///
/// Returns:
/// - `Ok(None)` when there's no node yet (pending first provision), the
///   node has been deleted, or the annotation is absent: reconcile continues
///   normally.
/// - `Ok(Some(action))` when the emergency path fired: the caller MUST
///   return this action immediately without continuing to the schedule
///   evaluation or phase dispatch.
/// - `Err(_)` when the Node API call fails in a non-404 way, or the
///   handler itself surfaces a fatal error (status/spec PATCH failure).
async fn check_emergency_reclaim(
    resource: &Arc<ScheduledMachine>,
    ctx: &Arc<Context>,
) -> Result<Option<Action>, ReconcilerError> {
    use k8s_openapi::api::core::v1::Node;
    use kube::api::Api;

    // Guard: no nodeRef yet means no node to reclaim.
    let Some(node_ref) = resource.status.as_ref().and_then(|s| s.node_ref.as_ref()) else {
        return Ok(None);
    };
    let node_name = &node_ref.name;
    if node_name.is_empty() {
        return Ok(None);
    }

    // Fetch the Node. 404 is benign — means the node was already removed,
    // nothing to reclaim. Other errors propagate so the reconciler retries.
    let nodes: Api<Node> = Api::all(ctx.client.clone());
    let node = match nodes.get(node_name).await {
        Ok(n) => n,
        Err(kube::Error::Api(e)) if e.code == 404 => {
            debug!(
                node = %node_name,
                "Referenced Node not found — skipping reclaim check"
            );
            return Ok(None);
        }
        Err(e) => return Err(ReconcilerError::KubeError(e)),
    };

    let Some(request) = super::helpers::node_reclaim_request(&node) else {
        return Ok(None);
    };

    info!(
        resource = %resource.name_any(),
        node = %node_name,
        reason = request.reason.as_deref().unwrap_or("(none)"),
        "Reclaim annotation observed — engaging emergency remove"
    );
    let action = super::helpers::handle_emergency_remove(
        Arc::clone(resource),
        Arc::clone(ctx),
        node_name,
        &request,
    )
    .await?;
    Ok(Some(action))
}

/// Handle the `EmergencyRemove` phase — idempotent catch for a controller
/// that crashed between step 6 (annotation clear) and step 7 (phase flip
/// to Disabled) of the emergency-reclaim ordering contract.
///
/// If the reclaim annotation is still present the earlier
/// [`check_emergency_reclaim`] guard has already re-driven the full flow
/// before reaching this match arm — so by the time we get here, the
/// annotation is cleared and all that's left is to finish the transition.
async fn handle_emergency_remove_phase(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    warn!(
        resource = %name,
        namespace = %namespace,
        "Resource stuck in EmergencyRemove after annotation clear — completing transition to Disabled"
    );

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
            false, // in_schedule: disabled means not in schedule
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
            false, // in_schedule: outside window
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
            true, // in_schedule: error occurred while in schedule
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
        true, // in_schedule: machine created because we're in schedule
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
            false, // in_schedule: disabled means not in schedule
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
            false, // in_schedule: outside schedule window
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)));
    }

    // Happy path: machine active and in schedule.
    //
    // Surface the CAPI Machine's providerID and full nodeRef on our own
    // status so operators can correlate `kubectl get scheduledmachine` output
    // to the underlying VM and Node without manual cross-referencing.
    // Best-effort: if the fetch or patch fails, log and continue — status
    // enrichment must never block the reconcile.
    let machine_name = format!("{name}-machine");
    match fetch_capi_machine(&ctx.client, &namespace, &machine_name).await {
        Ok(Some(machine)) => {
            let (provider_id, node_ref) = extract_machine_refs(&machine);
            if let Err(e) = patch_machine_refs_status(
                &ctx.client,
                &namespace,
                &name,
                provider_id.as_deref(),
                node_ref.as_ref(),
            )
            .await
            {
                warn!(
                    resource = %name,
                    namespace = %namespace,
                    error = %e,
                    "Failed to patch providerID/nodeRef status (non-fatal)"
                );
            }
            provision_reclaim_agent_best_effort(&resource, &ctx, &node_ref).await;
            reconcile_node_taints_best_effort(&resource, &ctx, &node_ref).await;
        }
        Ok(None) => {
            debug!(resource = %name, "CAPI Machine not found yet — skipping status enrichment");
        }
        Err(e) => {
            warn!(
                resource = %name,
                namespace = %namespace,
                error = %e,
                "Failed to fetch CAPI Machine for status enrichment (non-fatal)"
            );
        }
    }

    debug!(resource = %name, namespace = %namespace, "Machine active and in schedule");
    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

/// Best-effort projection of the reclaim-agent label + per-node
/// `ConfigMap` based on the current `spec.killIfCommands` list.
///
/// Runs from the `Active` phase once a `nodeRef` is known so the
/// projection follows the Node the machine is actually bound to. A
/// failure here must not block the reconcile — a missing or stale
/// projection degrades the emergency-reclaim path but does not break
/// day-to-day scheduling. Non-fatal errors are logged.
async fn provision_reclaim_agent_best_effort(
    resource: &Arc<ScheduledMachine>,
    ctx: &Arc<Context>,
    node_ref: &Option<crate::crd::NodeRef>,
) {
    let Some(node_name) = node_ref.as_ref().map(|n| n.name.as_str()) else {
        return;
    };
    if node_name.is_empty() {
        return;
    }
    let commands = resource.spec.kill_if_commands.as_deref().unwrap_or(&[]);
    if let Err(e) =
        super::helpers::reconcile_reclaim_agent_provision(&ctx.client, node_name, commands).await
    {
        warn!(
            resource = %resource.name_any(),
            node = %node_name,
            error = %e,
            "Failed to project reclaim-agent label/ConfigMap (non-fatal)"
        );
    }
}

/// Best-effort reconciliation of user-defined Node taints from
/// `spec.nodeTaints` onto the bound Node.
///
/// Runs from the `Active` phase once a `nodeRef` is known. Failures here are
/// logged but must never block the reconcile — taint drift degrades workload
/// scheduling preferences but does not threaten availability. If the outcome
/// is `Applied` and the applied list differs from `status.appliedNodeTaints`,
/// we persist the new list via a status merge-patch. Non-Ready / not-yet-
/// materialised Nodes and ownership conflicts are logged at warn/debug; the
/// Node watch will re-enqueue us when the Node transitions Ready.
async fn reconcile_node_taints_best_effort(
    resource: &Arc<ScheduledMachine>,
    ctx: &Arc<Context>,
    node_ref: &Option<crate::crd::NodeRef>,
) {
    // Termination guards (Phase 5). Each one is a correctness property, not a
    // performance optimisation: applying taints on a resource that is about
    // to be drained-and-deleted leaves a short-lived tainted Node behind,
    // which confuses humans reading `kubectl describe node` post-mortem.
    if resource.meta().deletion_timestamp.is_some() {
        debug!(
            resource = %resource.name_any(),
            "ScheduledMachine is being deleted — skipping node taint reconcile"
        );
        return;
    }
    if resource.spec.kill_switch {
        debug!(
            resource = %resource.name_any(),
            "kill_switch active — skipping node taint reconcile; drain is the sanctioned eviction path"
        );
        return;
    }

    let Some(node_name) = node_ref.as_ref().map(|n| n.name.as_str()) else {
        return;
    };
    if node_name.is_empty() {
        return;
    }

    let desired: &[crate::crd::NodeTaint] = resource.spec.node_taints.as_slice();
    let previously_applied: &[crate::crd::NodeTaint] = resource
        .status
        .as_ref()
        .map(|s| s.applied_node_taints.as_slice())
        .unwrap_or(&[]);

    if desired.is_empty() && previously_applied.is_empty() {
        return;
    }

    let input = super::helpers::ReconcileNodeTaintsInput {
        node_name,
        desired,
        previously_applied,
    };
    let outcome = match super::helpers::reconcile_node_taints(&ctx.client, input).await {
        Ok(o) => o,
        Err(e) => {
            warn!(
                resource = %resource.name_any(),
                node = %node_name,
                error = %e,
                "Failed to reconcile node taints (non-fatal)"
            );
            return;
        }
    };

    match outcome {
        super::helpers::NodeTaintReconcileOutcome::NoNodeYet => {
            debug!(
                resource = %resource.name_any(),
                node = %node_name,
                "Node not materialised yet — deferring taint apply"
            );
        }
        super::helpers::NodeTaintReconcileOutcome::NodeNotReady => {
            debug!(
                resource = %resource.name_any(),
                node = %node_name,
                "Node not Ready yet — deferring taint apply"
            );
        }
        super::helpers::NodeTaintReconcileOutcome::Conflict { conflicts } => {
            warn!(
                resource = %resource.name_any(),
                node = %node_name,
                conflicts = conflicts.len(),
                "Node taint ownership conflict — admin-owned taint blocks overwrite"
            );
        }
        super::helpers::NodeTaintReconcileOutcome::Applied { applied } => {
            if applied.as_slice() == previously_applied {
                debug!(
                    resource = %resource.name_any(),
                    node = %node_name,
                    "Node taints already up to date"
                );
                return;
            }
            let Some(namespace) = resource.namespace() else {
                return;
            };
            let name = resource.name_any();
            if let Err(e) = super::helpers::patch_applied_node_taints_status(
                &ctx.client,
                &namespace,
                &name,
                &applied,
            )
            .await
            {
                warn!(
                    resource = %name,
                    node = %node_name,
                    error = %e,
                    "Failed to patch status.appliedNodeTaints (non-fatal)"
                );
            }
        }
    }
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
                false, // in_schedule: shutting down means outside schedule
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
            false, // in_schedule: inactive means outside schedule
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
        true, // in_schedule: transitioning because schedule is now active
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
            false, // in_schedule: will be evaluated in pending phase
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
/// Simply requeues with the error backoff delay WITHOUT updating status.
/// This avoids the tight retry loop caused by status updates triggering
/// watch events. The resource stays in Error phase until the user fixes
/// the underlying issue (e.g., invalid annotation), at which point the
/// spec change triggers a new reconciliation that succeeds.
#[allow(clippy::needless_pass_by_value)] // Consistent with other phase handlers
fn handle_error_phase(
    resource: Arc<ScheduledMachine>,
    _ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().ok_or_else(|| {
        ReconcilerError::InvalidConfig("ScheduledMachine must be namespaced".to_string())
    })?;
    let name = resource.name_any();

    // Log at warn level (not error) since this is expected during backoff
    warn!(
        resource = %name,
        namespace = %namespace,
        "Resource in Error phase - waiting for spec change or manual intervention"
    );

    // Requeue with backoff delay but do NOT update status.
    // This prevents the watch event that causes tight loops.
    Ok(Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS)))
}

// Re-export helper functions for use in this module
use super::helpers::{
    add_finalizer, add_machine_to_cluster, check_grace_period_elapsed, drain_node_with_timeout,
    evaluate_schedule, extract_machine_refs, fetch_capi_machine, get_node_from_machine,
    handle_deletion, handle_kill_switch, has_finalizer, parse_duration, patch_machine_refs_status,
    remove_machine_from_cluster, should_process_resource, update_phase,
    update_phase_with_grace_period,
};

#[cfg(test)]
#[path = "scheduled_machine_tests.rs"]
mod tests;
