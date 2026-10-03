// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # `ScheduledCapacity` reconciler (ADR 0011)
//!
//! Gates one numeric capacity field on a **foreign** object from a
//! spot-schedule provider's verdict. The non-handover pattern: the physical
//! machine stays in the cluster the whole time and a bounded slice of it is
//! conceded while the schedule is open.
//!
//! ## What this reconciler will and will not do
//!
//! It issues exactly one kind of mutation against the governed object: a JSON
//! merge patch setting the single field named by
//! `spec.capacity.path`. It never creates that object, never deletes it, never
//! sets an `ownerReference` on it, and never touches any other field. An
//! unresolved `targetRef` is a condition, not a create (ADR 0011 decision 2).
//!
//! That restraint is the whole design. A CAPI `Machine` is safe for 5-Spot to
//! delete because 5-Spot owns the drain; a consumer's pool with live claims is
//! not, and the consumer already owns those semantics.
//!
//! ## Division of labour
//!
//! | Concern | Lives in |
//! | --- | --- |
//! | What to write and report | [`capacity_decision`](super::capacity_decision) (pure) |
//! | Path validation, patch construction, readback | [`capacity_path`](super::capacity_path) (pure) |
//! | Provider verdict | [`resolve_spot_schedule`] (shared with the machine controller, unchanged) |
//! | Everything touching the API | this module |
//!
//! Sharing [`resolve_spot_schedule`] is what made a separate controller
//! affordable: the `Unresolved`-is-not-`Inactive` rule, the `Ready` gate and
//! the precedence order cannot drift between the two controllers because there
//! is one copy of them.
//!
//! ## Event-driven
//!
//! Two dynamic watches feed this controller (see
//! [`dynamic_ref_watch`](super::dynamic_ref_watch)): one on the provider object
//! named by `spec.schedule`, one on the object named by `spec.targetRef`. The
//! second is what makes handback event-driven, since the drain signal is a
//! field on the target. The only timer in the module is the handback deadline
//! backstop, which exists solely to notice an expired deadline if no further
//! target event ever arrives.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::StreamExt;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use kube::api::{Api, Patch, PatchParams, PostParams};
use kube::core::{DynamicObject, GroupVersionKind};
use kube::discovery::pinned_kind;
use kube::runtime::controller::Action;
use kube::runtime::{watcher, Controller};
use kube::{Client, ResourceExt};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::constants::{
    ALLOWED_CAPACITY_TARGET_API_GROUPS, CAPACITY_HANDBACK_DEADLINE_CHECK_SECS,
    CONDITION_STATUS_FALSE, CONDITION_STATUS_TRUE, CONDITION_TYPE_CAPACITY_WRITTEN,
    CONDITION_TYPE_HANDBACK_COMPLETE, CONDITION_TYPE_HOST_GOVERNANCE_CONFLICT,
    CONDITION_TYPE_READY, CONDITION_TYPE_SPOT_SCHEDULE_RESOLVED, CONDITION_TYPE_TARGET_RESOLVED,
    DEFAULT_REQUEUE_SECS, ERROR_REQUEUE_SECS, PHASE_CAPACITY_ACTIVE,
    PHASE_CAPACITY_HANDBACK_TIMED_OUT, PHASE_CAPACITY_HANDING_BACK, RBAC_VERB_PATCH,
    REASON_HOST_GOVERNANCE_CONFLICT, REASON_NO_HOST_GOVERNANCE_CONFLICT,
    REASON_TARGET_CRD_NOT_INSTALLED, REASON_TARGET_GROUP_NOT_ALLOWED, REASON_TARGET_NOT_FOUND,
    REASON_TARGET_NOT_WRITABLE, REASON_TARGET_RESOLVED,
};
use crate::crd::{Condition, ScheduledCapacity, ScheduledMachine};
use crate::reconcilers::capacity_decision::{decide, CapacityDecision, CapacityDecisionInput};
use crate::reconcilers::capacity_path::{
    build_merge_patch, read_i64_at, validate_drained_path, validate_write_path, CapacityPathError,
};
use crate::reconcilers::spot_schedule::{resolve_spot_schedule, SpotScheduleVerdict};

/// Errors raised while reconciling a `ScheduledCapacity`.
///
/// Deliberately **not** `ReconcilerError`: that type carries CAPI, bootstrap
/// and drain variants this controller has no business being able to produce.
#[derive(Debug, thiserror::Error)]
pub enum CapacityError {
    /// The object has no namespace (impossible for a namespaced CRD).
    #[error("ScheduledCapacity must be namespaced")]
    Unnamespaced,

    /// A Kubernetes API call failed.
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),

    /// `spec.capacity.path` or `spec.handback.drainedPath` is not a legal
    /// capacity path. Admission should have caught this; reaching here means
    /// the deployed CRD is older than this controller.
    #[error("invalid capacity path: {0}")]
    Path(#[from] CapacityPathError),

    /// `spec.handback.timeout` is not a parseable duration.
    #[error("invalid handback timeout: {0}")]
    Timeout(String),
}

/// Controller context: the client plus the leader flag.
pub struct CapacityContext {
    /// Kubernetes client authenticated as the capacity controller's
    /// ServiceAccount. Deliberately a different identity from the machine
    /// controller's (ADR 0011 decision 3).
    pub client: Client,
    /// Whether this replica holds the capacity controller's own Lease.
    pub is_leader: Arc<AtomicBool>,
}

/// How `spec.targetRef` resolved, and what to report if it did not.
struct TargetResolution {
    /// The resolved object, when it exists.
    object: Option<DynamicObject>,
    /// `true` when the object exists and this ServiceAccount may patch it.
    writable: bool,
    /// Condition reason.
    reason: &'static str,
    /// Condition message.
    message: String,
}

/// Reconcile one `ScheduledCapacity`.
///
/// # Errors
/// [`CapacityError`] for a transient API failure (so the controller retries
/// with back-off) or for a path/timeout that admission should have rejected.
/// An absent provider or target is **not** an error: it is a held state with a
/// condition explaining why.
pub async fn reconcile(
    capacity: Arc<ScheduledCapacity>,
    ctx: Arc<CapacityContext>,
) -> Result<Action, CapacityError> {
    // Standby replicas do nothing and wake the instant they win the lease.
    if !ctx.is_leader.load(Ordering::Acquire) {
        return Ok(Action::await_change());
    }

    let namespace = capacity.namespace().ok_or(CapacityError::Unnamespaced)?;
    let name = capacity.name_any();
    let now = Utc::now();

    // Re-validate both paths here, not only at admission. The schema guarantee
    // holds exactly as long as the deployed CRD matches this binary, which is
    // precisely the case a schema cannot defend against.
    validate_write_path(&capacity.spec.capacity.path)?;
    let drained_path = match capacity
        .spec
        .handback
        .as_ref()
        .and_then(|h| h.drained_path.as_ref())
    {
        Some(path) => {
            validate_drained_path(path)?;
            Some(path.clone())
        }
        None => None,
    };

    let verdict = resolve_spot_schedule(&ctx.client, &namespace, &capacity.spec.schedule)
        .await
        .unwrap_or_else(|error| {
            // A transient API error resolving the provider must not zero
            // capacity: hold, exactly as an Unresolved verdict does.
            warn!(
                capacity = %name,
                namespace = %namespace,
                %error,
                "provider resolution failed transiently; holding last known state"
            );
            SpotScheduleVerdict::Unresolved {
                reason: crate::constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_FOUND,
                message: format!("transient provider resolution failure: {error}"),
            }
        });

    let target = resolve_target(&ctx.client, &namespace, &capacity).await?;

    let observed_drained = match (&target.object, &drained_path) {
        (Some(object), Some(path)) => read_i64_at(&object.data, path),
        _ => None,
    };

    let conflict = detect_host_governance_conflict(&ctx.client, &namespace, &capacity).await?;

    let handback_timeout = parse_handback_timeout(&capacity)?;
    let status = capacity.status.as_ref();

    let decision = decide(&CapacityDecisionInput {
        enabled: capacity.spec.enabled,
        kill_switch: capacity.spec.kill_switch,
        active_value: capacity.spec.capacity.active_value,
        verdict: &verdict,
        last_known_active: status
            .and_then(|s| s.spot_schedule.as_ref())
            .and_then(|s| s.active),
        last_written: status.and_then(|s| s.written_value),
        target_resolved: target.writable,
        has_drain_signal: drained_path.is_some(),
        observed_drained,
        handback_timeout,
        handback_deadline: status
            .and_then(|s| s.handback_deadline.as_deref())
            .and_then(parse_rfc3339),
        conflict,
        now,
    });

    // Write first, then status: a status claiming a value that was never
    // written is the one lie this controller must never tell.
    let written = match decision.write {
        Some(value) => {
            write_capacity(&ctx.client, &namespace, &capacity, value).await?;
            info!(
                capacity = %name,
                namespace = %namespace,
                path = %capacity.spec.capacity.path,
                value,
                phase = decision.phase,
                "wrote capacity to target"
            );
            crate::metrics::set_capacity_written_value(&namespace, &name, value);
            Some(value)
        }
        None => status.and_then(|s| s.written_value),
    };

    let previous_phase = status.and_then(|s| s.phase.as_deref());
    record_decision_metrics(&namespace, &name, &decision, previous_phase);
    patch_status(
        &ctx.client,
        &namespace,
        &name,
        &capacity,
        &decision,
        &verdict,
        &target,
        written,
        observed_drained,
        now,
    )
    .await?;

    Ok(requeue_for(&decision))
}

/// Resolve `spec.targetRef`: allowlist the group, discover the kind, fetch the
/// object, and pre-flight the `patch` permission.
///
/// Returns a [`TargetResolution`] rather than an error for every "cannot write"
/// outcome, so each becomes a condition instead of a failed reconcile.
async fn resolve_target(
    client: &Client,
    namespace: &str,
    capacity: &ScheduledCapacity,
) -> Result<TargetResolution, CapacityError> {
    let reference = &capacity.spec.target_ref;
    let (group, version) = reference
        .api_version
        .split_once('/')
        .unwrap_or(("", reference.api_version.as_str()));

    // The allowlist is checked here rather than in the CRD schema so the set of
    // API groups 5-Spot will write lives in exactly one greppable place
    // (src/constants.rs) and cannot be loosened by a stale CRD.
    if !ALLOWED_CAPACITY_TARGET_API_GROUPS.contains(&group) {
        return Ok(TargetResolution {
            object: None,
            writable: false,
            reason: REASON_TARGET_GROUP_NOT_ALLOWED,
            message: format!(
                "target API group '{group}' is not allowed; permitted groups: \
                 {ALLOWED_CAPACITY_TARGET_API_GROUPS:?}"
            ),
        });
    }

    let gvk = GroupVersionKind::gvk(group, version, &reference.kind);
    let Ok((api_resource, _capabilities)) = pinned_kind(client, &gvk).await else {
        return Ok(TargetResolution {
            object: None,
            writable: false,
            reason: REASON_TARGET_CRD_NOT_INSTALLED,
            message: format!(
                "no CRD for {group}/{version} kind {} is installed",
                reference.kind
            ),
        });
    };

    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let Some(object) = api.get_opt(&reference.name).await? else {
        return Ok(TargetResolution {
            object: None,
            writable: false,
            reason: REASON_TARGET_NOT_FOUND,
            message: format!(
                "target {} {namespace}/{} not found",
                reference.kind, reference.name
            ),
        });
    };

    // Pre-flight the permission so an RBAC gap is a clear condition rather than
    // an opaque 403 partway through actuation (ADR 0011 decision 5).
    if !can_patch(client, namespace, group, &api_resource.plural).await? {
        return Ok(TargetResolution {
            object: Some(object),
            writable: false,
            reason: REASON_TARGET_NOT_WRITABLE,
            message: format!(
                "capacity controller service account may not patch '{}' in API group \
                 '{group}' (namespace '{namespace}')",
                api_resource.plural
            ),
        });
    }

    Ok(TargetResolution {
        object: Some(object),
        writable: true,
        reason: REASON_TARGET_RESOLVED,
        message: format!("target {} {}", reference.kind, reference.name),
    })
}

/// `true` if this ServiceAccount may `patch` `plural` in `group`/`namespace`,
/// per a [`SelfSubjectAccessReview`].
///
/// # Errors
/// [`CapacityError::Kube`] if the review call itself fails.
async fn can_patch(
    client: &Client,
    namespace: &str,
    group: &str,
    plural: &str,
) -> Result<bool, CapacityError> {
    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(ResourceAttributes {
                verb: Some(RBAC_VERB_PATCH.to_string()),
                group: Some(group.to_string()),
                resource: Some(plural.to_string()),
                namespace: Some(namespace.to_string()),
                ..ResourceAttributes::default()
            }),
            ..SelfSubjectAccessReviewSpec::default()
        },
        ..SelfSubjectAccessReview::default()
    };
    let api: Api<SelfSubjectAccessReview> = Api::all(client.clone());
    let result = api.create(&PostParams::default(), &review).await?;
    Ok(result.status.unwrap_or_default().allowed)
}

/// `true` when `spec.nodeName` is also the `status.nodeRef.name` of some
/// `ScheduledMachine` in this namespace.
///
/// A host is governed by `ScheduledMachine` **or** by `ScheduledCapacity`,
/// never both: one removes the node, the other keeps it and shares it, and both
/// at once half-works (the node is drained for handover while a capacity gate
/// is still sizing guests on it).
///
/// This check is why ADR 0011 decision 7 is controller-side rather than a
/// `ValidatingAdmissionPolicy`: a VAP evaluates one request against its own
/// object and its bound `paramRef`, and cannot look up other objects at all.
///
/// Without `spec.nodeName` the two objects cannot be correlated and this
/// returns `false`, leaving the invariant as documentation.
async fn detect_host_governance_conflict(
    client: &Client,
    namespace: &str,
    capacity: &ScheduledCapacity,
) -> Result<bool, CapacityError> {
    let Some(node_name) = capacity.spec.node_name.as_deref() else {
        return Ok(false);
    };

    let api: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);
    let machines = api.list(&kube::api::ListParams::default()).await?;
    let conflicting = machines.items.iter().find(|machine| {
        machine
            .status
            .as_ref()
            .and_then(|status| status.node_ref.as_ref())
            .is_some_and(|node_ref| node_ref.name == node_name)
    });

    if let Some(machine) = conflicting {
        warn!(
            capacity = %capacity.name_any(),
            namespace = %namespace,
            node = %node_name,
            scheduled_machine = %machine.name_any(),
            "node is governed by both a ScheduledMachine and a ScheduledCapacity; \
             refusing to write capacity"
        );
        return Ok(true);
    }
    Ok(false)
}

/// Merge-patch the single field named by `spec.capacity.path` on the target.
///
/// # Errors
/// [`CapacityError::Path`] if the path is invalid (already checked by the
/// caller, re-checked by the builder), [`CapacityError::Kube`] on API failure.
async fn write_capacity(
    client: &Client,
    namespace: &str,
    capacity: &ScheduledCapacity,
    value: i64,
) -> Result<(), CapacityError> {
    let reference = &capacity.spec.target_ref;
    let (group, version) = reference
        .api_version
        .split_once('/')
        .unwrap_or(("", reference.api_version.as_str()));
    let gvk = GroupVersionKind::gvk(group, version, &reference.kind);
    let (api_resource, _capabilities) = pinned_kind(client, &gvk).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);

    let patch = build_merge_patch(&capacity.spec.capacity.path, value)?;
    api.patch(
        &reference.name,
        &PatchParams::default(),
        &Patch::Merge(&patch),
    )
    .await?;
    Ok(())
}

/// Parse `spec.handback.timeout`, defaulting when there is no handback block.
///
/// # Errors
/// [`CapacityError::Timeout`] when the value is not a parseable duration.
fn parse_handback_timeout(capacity: &ScheduledCapacity) -> Result<ChronoDuration, CapacityError> {
    let Some(handback) = capacity.spec.handback.as_ref() else {
        return Ok(ChronoDuration::seconds(0));
    };
    let parsed = crate::reconcilers::helpers::parse_duration(&handback.timeout)
        .map_err(|error| CapacityError::Timeout(error.to_string()))?;
    ChronoDuration::from_std(parsed)
        .map_err(|error| CapacityError::Timeout(format!("duration out of range: {error}")))
}

/// Parse an RFC3339 timestamp, returning `None` rather than failing: a status
/// field this controller wrote should always parse, and a corrupted one is
/// better treated as "no deadline recorded" than as a reconcile error.
fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Record the metrics a decision implies, counting **transitions** rather than
/// reconciles (see [`entered_phase`]).
fn record_decision_metrics(
    namespace: &str,
    name: &str,
    decision: &CapacityDecision,
    previous_phase: Option<&str>,
) {
    if decision.phase == PHASE_CAPACITY_HANDBACK_TIMED_OUT
        && entered_phase(decision.phase, previous_phase)
    {
        crate::metrics::record_capacity_handback_timeout(namespace, name);
    }
    if decision.conflict && entered_phase(decision.phase, previous_phase) {
        crate::metrics::record_capacity_host_governance_conflict(namespace, name);
    }
}

/// Preserve each condition's `lastTransitionTime` wherever its `status` has not
/// changed, matching conditions by `type`.
///
/// **This is the hot-reconcile-loop guard, not cosmetics.** `Condition::new`
/// stamps `lastTransitionTime` with `now`, so an unconditional status patch
/// makes the computed status differ from the stored one on *every* reconcile.
/// That write re-triggers this controller's own watch, which reconciles again,
/// forever. Measured live before this guard: ~50 `resourceVersion` bumps per
/// second, and a single handback timeout counted 100 times.
///
/// Only a `status` transition restamps. A changed `reason` or `message` (a
/// drain counter ticking down, say) updates the text but keeps the timestamp,
/// per Kubernetes condition conventions, because restamping on message drift
/// would reopen the same loop.
fn merge_condition_timestamps(computed: Vec<Condition>, previous: &[Condition]) -> Vec<Condition> {
    computed
        .into_iter()
        .map(|mut condition| {
            let unchanged = previous
                .iter()
                .find(|stored| stored.r#type == condition.r#type)
                .filter(|stored| stored.status == condition.status);
            if let Some(stored) = unchanged {
                condition.last_transition_time = stored.last_transition_time.clone();
            }
            condition
        })
        .collect()
}

/// `true` when this reconcile is the one that *entered* `phase`.
///
/// Metrics that mean "this happened" must count transitions, not reconciles. A
/// counter incremented once per reconcile while the object sits in a phase
/// reports a single handback timeout as a hundred of them, which is worse than
/// no metric because it is trusted.
fn entered_phase(phase: &str, previous_phase: Option<&str>) -> bool {
    previous_phase != Some(phase)
}

/// What one reconcile *observed*, as distinct from what it *decided*.
///
/// Grouped so the status builder takes a readable argument list, and so adding
/// an observation is one field rather than one more positional parameter nobody
/// can keep straight at the call site.
struct ObservedState {
    /// Value currently written to the target, carried forward when this
    /// reconcile wrote nothing.
    written: Option<i64>,
    /// Last value read from `spec.handback.drainedPath`.
    observed_drained: Option<i64>,
    /// Resolved `targetRef` for the status, or `Value::Null`.
    target_ref: Value,
    /// Reconcile instant.
    now: DateTime<Utc>,
}

/// Build the `status` patch body.
///
/// Every key this controller owns is **always present**, with an explicit
/// `null` when it should be absent. A JSON merge patch that merely *omits* a
/// key leaves the stored value untouched, so an omitted `handbackDeadline`
/// could never be cleared: a completed handback would keep advertising the
/// deadline it already met.
fn build_status_value(
    capacity: &ScheduledCapacity,
    decision: &CapacityDecision,
    observed: &ObservedState,
    conditions: Vec<Condition>,
    verdict: &SpotScheduleVerdict,
) -> Value {
    let ObservedState {
        written,
        observed_drained,
        target_ref,
        now,
    } = observed;
    let (written, observed_drained, now) = (*written, *observed_drained, *now);
    let previous_written = capacity.status.as_ref().and_then(|s| s.written_value);
    // Stamp a write time only when the value actually changed, for the same
    // reason conditions keep their timestamps.
    let last_write_time = if written != previous_written && written.is_some() {
        Some(now.to_rfc3339())
    } else {
        capacity
            .status
            .as_ref()
            .and_then(|s| s.last_write_time.clone())
    };

    json!({
        "phase": decision.phase,
        "message": decision.message,
        "ready": decision.ready,
        "conditions": conditions,
        "observedGeneration": capacity.metadata.generation,
        "writtenValue": written,
        "lastWriteTime": last_write_time,
        "observedDrainedValue": observed_drained,
        "governedNode": capacity.spec.node_name,
        "handbackDeadline": decision.handback_deadline.map(|d| d.to_rfc3339()),
        // Every key this controller owns must appear here. A key that is built
        // somewhere else, or omitted, makes the no-op guard below compare a
        // desired status that can never equal the stored one, so the guard
        // silently stops working and every reconcile patches again. The
        // exhaustive key-coverage test is what keeps this list honest.
        "targetRef": target_ref.clone(),
        "spotSchedule": {
            "resolved": verdict.is_resolved(),
            "active": verdict.active(),
            "reason": verdict.reason(),
            "message": verdict_message(verdict),
            "providerGeneration": verdict.provider_generation(),
            "lastTransitionTime": spot_schedule_transition_time(capacity, verdict, now),
        },
    })
}

/// `lastTransitionTime` for the mirrored spot-schedule status: bumped only when
/// the provider's `active` actually flips, held otherwise.
///
/// Same reasoning as [`merge_condition_timestamps`]: a timestamp that moves on
/// every reconcile makes the computed status differ from the stored one
/// forever.
fn spot_schedule_transition_time(
    capacity: &ScheduledCapacity,
    verdict: &SpotScheduleVerdict,
    now: DateTime<Utc>,
) -> Option<String> {
    let stored = capacity
        .status
        .as_ref()
        .and_then(|s| s.spot_schedule.as_ref());
    let previous_active = stored.and_then(|s| s.active);
    if previous_active == verdict.active() {
        return stored.and_then(|s| s.last_transition_time.clone());
    }
    Some(now.to_rfc3339())
}

/// The resolved `targetRef` for the status, or `Value::Null` when the target
/// did not resolve.
fn target_ref_value(
    capacity: &ScheduledCapacity,
    namespace: &str,
    target: &TargetResolution,
) -> Value {
    match target.object.as_ref() {
        Some(object) => json!({
            "apiVersion": capacity.spec.target_ref.api_version,
            "kind": capacity.spec.target_ref.kind,
            "name": capacity.spec.target_ref.name,
            "namespace": namespace,
            "uid": object.metadata.uid,
        }),
        None => Value::Null,
    }
}

/// Patch `status` with the decision's outcome and the full condition set,
/// **skipping the call entirely when nothing changed**.
///
/// The no-op guard is the second half of the loop fix. Even with stable
/// condition timestamps, a same-value `patch_status` still counts as an object
/// update and re-triggers the watch, so the cheapest correct thing is not to
/// send it.
#[allow(clippy::too_many_arguments)] // One status write; a struct would only move the list.
async fn patch_status(
    client: &Client,
    namespace: &str,
    name: &str,
    capacity: &ScheduledCapacity,
    decision: &CapacityDecision,
    verdict: &SpotScheduleVerdict,
    target: &TargetResolution,
    written: Option<i64>,
    observed_drained: Option<i64>,
    now: DateTime<Utc>,
) -> Result<(), CapacityError> {
    let bool_status = |value: bool| {
        if value {
            CONDITION_STATUS_TRUE
        } else {
            CONDITION_STATUS_FALSE
        }
    };

    let computed = vec![
        Condition::new(
            CONDITION_TYPE_READY,
            bool_status(decision.ready),
            decision.reason,
            &decision.message,
        ),
        Condition::new(
            CONDITION_TYPE_SPOT_SCHEDULE_RESOLVED,
            bool_status(verdict.is_resolved()),
            verdict.reason(),
            &verdict_message(verdict),
        ),
        Condition::new(
            CONDITION_TYPE_TARGET_RESOLVED,
            bool_status(target.writable),
            target.reason,
            &target.message,
        ),
        Condition::new(
            CONDITION_TYPE_CAPACITY_WRITTEN,
            bool_status(written.is_some()),
            decision.reason,
            &match written {
                Some(value) => format!("{} is {value}", capacity.spec.capacity.path),
                None => "nothing has been written to the target".to_string(),
            },
        ),
        Condition::new(
            CONDITION_TYPE_HANDBACK_COMPLETE,
            bool_status(decision.handback_complete),
            decision.reason,
            &decision.message,
        ),
        Condition::new(
            CONDITION_TYPE_HOST_GOVERNANCE_CONFLICT,
            bool_status(decision.conflict),
            if decision.conflict {
                REASON_HOST_GOVERNANCE_CONFLICT
            } else {
                REASON_NO_HOST_GOVERNANCE_CONFLICT
            },
            &governance_message(capacity, decision.conflict),
        ),
    ];

    let stored_conditions = capacity
        .status
        .as_ref()
        .map(|s| s.conditions.as_slice())
        .unwrap_or_default();
    let conditions = merge_condition_timestamps(computed, stored_conditions);

    let observed = ObservedState {
        written,
        observed_drained,
        target_ref: target_ref_value(capacity, namespace, target),
        now,
    };
    let status = build_status_value(capacity, decision, &observed, conditions, verdict);

    // No-op guard. Deserialising back into the typed status is what makes the
    // comparison honest: it normalises exactly the way the API server will
    // store it, so "equal" means the patch would genuinely change nothing.
    match serde_json::from_value::<crate::crd::ScheduledCapacityStatus>(status.clone()) {
        Ok(desired) => {
            if capacity.status.as_ref() == Some(&desired) {
                debug!(
                    capacity = %name,
                    namespace = %namespace,
                    "status unchanged; skipping patch to avoid re-triggering our own watch"
                );
                return Ok(());
            }
            // Say *what* differs. A guard that silently stops matching looks
            // exactly like a guard that is working, so the diff is the only way
            // to tell the two apart from a log.
            debug!(
                capacity = %name,
                namespace = %namespace,
                stored = ?capacity.status.as_ref(),
                desired = ?desired,
                "status changed; patching"
            );
        }
        Err(error) => {
            warn!(
                capacity = %name,
                namespace = %namespace,
                %error,
                "computed status did not deserialise into ScheduledCapacityStatus; \
                 patching unconditionally. This disables the no-op guard, so the \
                 status shape and the type have drifted apart."
            );
        }
    }

    let api: Api<ScheduledCapacity> = Api::namespaced(client.clone(), namespace);
    api.patch_status(
        name,
        &PatchParams::default(),
        &Patch::Merge(&json!({ "status": status })),
    )
    .await?;
    Ok(())
}

/// Message for the `HostGovernanceConflict` condition.
///
/// Says which node is believed governed, or that no node was declared, rather
/// than echoing the phase message: an operator reading this condition is asking
/// a governance question, not a scheduling one.
fn governance_message(capacity: &ScheduledCapacity, conflict: bool) -> String {
    match (capacity.spec.node_name.as_deref(), conflict) {
        (Some(node), true) => format!(
            "node {node} is also the status.nodeRef of a ScheduledMachine in this \
             namespace; refusing to write capacity"
        ),
        (Some(node), false) => {
            format!("no ScheduledMachine in this namespace claims node {node}")
        }
        (None, _) => "spec.nodeName is not set, so overlap with a ScheduledMachine \
                      cannot be detected; the one-host-one-governor invariant is \
                      documentation only"
            .to_string(),
    }
}

/// Human-readable provider resolution detail for a condition message.
fn verdict_message(verdict: &SpotScheduleVerdict) -> String {
    match verdict {
        SpotScheduleVerdict::Active { .. } => "provider reports active".to_string(),
        SpotScheduleVerdict::Inactive { .. } => "provider reports inactive".to_string(),
        SpotScheduleVerdict::Unresolved { message, .. } => message.clone(),
    }
}

/// Requeue interval for a decision.
///
/// Only the handback states get a short interval, and it is a **deadline
/// backstop, not a poll**: the drain signal arrives through the target watch.
/// Without it, an expired `handback.timeout` would go unnoticed when the
/// consumer stops emitting events entirely, which is exactly the situation in
/// which the deadline matters.
#[must_use]
fn requeue_for(decision: &CapacityDecision) -> Action {
    match decision.phase {
        PHASE_CAPACITY_HANDING_BACK => {
            Action::requeue(Duration::from_secs(CAPACITY_HANDBACK_DEADLINE_CHECK_SECS))
        }
        PHASE_CAPACITY_ACTIVE | PHASE_CAPACITY_HANDBACK_TIMED_OUT => {
            Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS))
        }
        _ => Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS)),
    }
}

/// Controller error policy: log and requeue with a fixed back-off.
#[must_use]
pub fn error_policy(
    capacity: Arc<ScheduledCapacity>,
    error: &CapacityError,
    _ctx: Arc<CapacityContext>,
) -> Action {
    warn!(
        capacity = %capacity.name_any(),
        error = %error,
        "ScheduledCapacity reconcile error; requeuing"
    );
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS))
}

/// Run the capacity controller until shutdown.
///
/// Wires the two dynamic reference watches (provider and target) into
/// `reconcile_on` so both a schedule flip and a drain-counter change wake the
/// relevant object at watch latency.
///
/// # Errors
/// Returns an error only if the controller stream fails to start.
pub async fn run(client: Client, is_leader: Arc<AtomicBool>) -> anyhow::Result<()> {
    use crate::constants::SPOT_SCHEDULE_EVENT_CHANNEL_CAP;
    use crate::reconcilers::dynamic_ref_watch::{
        capacity_schedule_key_for, capacity_target_key_for, CapacityRefWatchManager,
        WATCH_LABEL_CAPACITY_TARGET, WATCH_LABEL_SPOT_SCHEDULE,
    };

    let api = Api::<ScheduledCapacity>::all(client.clone());
    let ctx = Arc::new(CapacityContext {
        client: client.clone(),
        is_leader,
    });

    let (schedule_tx, schedule_rx) = tokio::sync::mpsc::channel(SPOT_SCHEDULE_EVENT_CHANNEL_CAP);
    let (target_tx, target_rx) = tokio::sync::mpsc::channel(SPOT_SCHEDULE_EVENT_CHANNEL_CAP);

    let schedule_watch = CapacityRefWatchManager::new(
        client.clone(),
        schedule_tx,
        capacity_schedule_key_for,
        WATCH_LABEL_SPOT_SCHEDULE,
    );
    let target_watch = CapacityRefWatchManager::new(
        client.clone(),
        target_tx,
        capacity_target_key_for,
        WATCH_LABEL_CAPACITY_TARGET,
    );

    // One reflector feeds both indexes. On restart its initial list rebuilds
    // them from cluster state, so nothing is stored outside Kubernetes.
    let reflector_api = Api::<ScheduledCapacity>::all(client);
    tokio::spawn(async move {
        let mut stream = watcher::watcher(reflector_api, watcher::Config::default()).boxed();
        while let Some(event) = stream.next().await {
            match event {
                Ok(watcher::Event::Apply(obj) | watcher::Event::InitApply(obj)) => {
                    schedule_watch.observe(&obj);
                    target_watch.observe(&obj);
                }
                Ok(watcher::Event::Delete(obj)) => {
                    schedule_watch.forget(&obj);
                    target_watch.forget(&obj);
                }
                Ok(watcher::Event::Init | watcher::Event::InitDone) => {}
                Err(error) => {
                    warn!(%error, "ScheduledCapacity reflector error; kube-runtime will reconnect");
                }
            }
        }
    });

    info!("Starting ScheduledCapacity controller");
    Controller::new(api, watcher::Config::default())
        .reconcile_on(tokio_stream::wrappers::ReceiverStream::new(schedule_rx))
        .reconcile_on(tokio_stream::wrappers::ReceiverStream::new(target_rx))
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|result| async move {
            // Log every outcome, not just failures. A controller that is silent
            // on success gives no way to tell "nothing needed doing" from
            // "nothing ran", which is exactly the ambiguity that hid a dead
            // watch during development.
            match result {
                Ok((object, action)) => {
                    info!(
                        capacity = %object.name,
                        namespace = ?object.namespace,
                        next_action = ?action,
                        "ScheduledCapacity reconciliation completed"
                    );
                }
                Err(error) => {
                    warn!(error = %error, "ScheduledCapacity reconciliation error");
                }
            }
        })
        .await;
    info!("ScheduledCapacity controller shut down");
    Ok(())
}

#[cfg(test)]
#[path = "scheduled_capacity_tests.rs"]
mod tests;
