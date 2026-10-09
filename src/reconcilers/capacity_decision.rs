// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # The capacity decision (ADR 0011): pure, no I/O
//!
//! One function, [`decide`], answers "what should this `ScheduledCapacity` look
//! like right now" from a snapshot of inputs. Everything that touches the
//! Kubernetes API lives in [`scheduled_capacity`](super::scheduled_capacity);
//! everything that *decides* lives here, so each transition is a table row in a
//! unit test rather than a mocked cluster.
//!
//! ## Precedence
//!
//! Guard clauses, in this order, each one terminal:
//!
//! 1. **Host governance conflict** - one host cannot be both handed over and
//!    shared, so capacity is driven to zero and the active value is withheld
//!    (ADR 0014, amending ADR 0011 decision 7).
//! 2. **Target unresolved** - nothing to write to; hold and report why.
//! 3. **`killSwitch`** - immediate, terminal, operator-demanded.
//! 4. **The composed should-be-active decision** - `spec.enabled` and the
//!    provider verdict, via [`compose_should_be_active`], the *same* function
//!    the machine controller uses so the two cannot drift.
//!
//! ## The handback sequence, and why zero is written first
//!
//! On deactivation the controller writes zero **immediately** and then waits
//! for `spec.handback.drainedPath` to reach zero.
//!
//! Writing zero first is not an escalation, it is what makes the drain
//! converge. The gated field is a *warm pool* target: zero stops the consumer
//! replenishing idle capacity, while work already claimed keeps running and
//! finishes on its own. Holding the field above zero until the consumer
//! reported drained would be circular, because a pool that keeps handing out
//! warm members never reports zero in use.
//!
//! When the deadline passes with the consumer still not drained, the controller
//! **holds** (the field stays at zero) and reports loudly. It does not delete
//! the object, does not force anything, and does not escalate: a missed
//! handover is visible and recoverable, while destroying an agent mid-task is
//! neither. That is ADR 0011 decision 6.
//!
//! If the schedule reopens mid-handback the active value is simply written
//! again and the object returns to `Active`. That reversibility is precisely
//! what ADR 0011 bought by making actuation a scale rather than a delete.

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::constants::{
    CAPACITY_INACTIVE_VALUE, PHASE_CAPACITY_ACTIVE, PHASE_CAPACITY_DISABLED, PHASE_CAPACITY_ERROR,
    PHASE_CAPACITY_HANDBACK_TIMED_OUT, PHASE_CAPACITY_HANDING_BACK, PHASE_CAPACITY_INACTIVE,
    PHASE_CAPACITY_PENDING, PHASE_CAPACITY_TERMINATED, REASON_CAPACITY_ACTIVE,
    REASON_CAPACITY_INACTIVE, REASON_HANDBACK_NOT_OBSERVED, REASON_HANDBACK_TIMED_OUT,
    REASON_HANDBACK_WAITING, REASON_HOST_GOVERNANCE_CONFLICT, REASON_TARGET_NOT_ACTUABLE,
};
use crate::reconcilers::helpers::compose_should_be_active;
use crate::reconcilers::spot_schedule::SpotScheduleVerdict;

/// Snapshot of everything [`decide`] needs. Borrowed, so building one costs
/// nothing and a test can vary one field at a time.
#[derive(Debug, Clone)]
pub struct CapacityDecisionInput<'a> {
    /// `spec.enabled`.
    pub enabled: bool,
    /// `spec.killSwitch`.
    pub kill_switch: bool,
    /// `spec.capacity.activeValue`.
    pub active_value: i64,
    /// The resolved provider verdict for `spec.schedule`.
    pub verdict: &'a SpotScheduleVerdict,
    /// `status.spotSchedule.active`: the value held while the provider is
    /// unresolved (hold-last-state, ADR 0006 §4).
    pub last_known_active: Option<bool>,
    /// `status.writtenValue`: what this controller last wrote, used to avoid
    /// rewriting a value that is already correct.
    pub last_written: Option<i64>,
    /// `true` when `spec.targetRef` resolved and is writable.
    pub target_resolved: bool,
    /// `true` when `spec.handback.drainedPath` is configured. Without it there
    /// is nothing to observe and handback completes on the zero write.
    pub has_drain_signal: bool,
    /// Last value read from `spec.handback.drainedPath`. `None` means not read
    /// (or not numeric), which is **not** the same as `Some(0)`.
    pub observed_drained: Option<i64>,
    /// `spec.handback.timeout`, parsed.
    pub handback_timeout: ChronoDuration,
    /// `status.handbackDeadline`, if a wait is already running.
    pub handback_deadline: Option<DateTime<Utc>>,
    /// `true` when `spec.nodeName` is also a `ScheduledMachine`'s
    /// `status.nodeRef.name` in this namespace.
    pub conflict: bool,
    /// Reconcile instant.
    pub now: DateTime<Utc>,
}

/// What the controller should do and report this reconcile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityDecision {
    /// Phase to publish.
    pub phase: &'static str,
    /// Value to write to the target, or `None` to leave the field untouched.
    ///
    /// `None` covers three different situations on purpose: nothing may be
    /// written (conflict, unresolved target), nothing *needs* writing (the
    /// value is already correct), and nothing *more* will be written (handback
    /// timed out, holding).
    pub write: Option<i64>,
    /// The handback deadline that should be recorded, or `None` to clear it.
    pub handback_deadline: Option<DateTime<Utc>>,
    /// Machine-readable reason.
    pub reason: &'static str,
    /// Human-readable detail.
    pub message: String,
    /// `status.ready`: true only in `Active`.
    pub ready: bool,
    /// `HandbackComplete` condition status.
    pub handback_complete: bool,
    /// `HostGovernanceConflict` condition status.
    pub conflict: bool,
}

impl CapacityDecision {
    /// A decision that writes nothing and reports `phase`/`reason`.
    fn hold(phase: &'static str, reason: &'static str, message: String) -> Self {
        Self {
            phase,
            write: None,
            handback_deadline: None,
            reason,
            message,
            ready: false,
            handback_complete: false,
            conflict: false,
        }
    }
}

/// Decide what to write and report. Pure.
///
/// See the module docs for the precedence order and the handback sequence.
#[must_use]
#[allow(clippy::too_many_lines)] // One linear decision table; splitting it would hide the precedence.
pub fn decide(input: &CapacityDecisionInput<'_>) -> CapacityDecision {
    // 1. A contradiction drives capacity to ZERO (ADR 0011 decision 7, as
    //    amended by ADR 0014). A host governed by both a ScheduledMachine and a
    //    ScheduledCapacity is drained for handover while capacity is still
    //    being sized on it.
    //
    //    The original rule refused to write anything, which sounded like
    //    failing closed and was not: a value written before the conflict
    //    appeared simply stood, which is the contradiction itself. The two
    //    write directions are not symmetrical here. The active value concedes
    //    capacity on a node that is going away; zero stops replenishment while
    //    claimed work finishes on its own, which is what the machine
    //    controller is already doing to that node. Zero is the safe direction,
    //    so declining to move in it was the error.
    //
    //    "Fail closed" is preserved where it matters: the active value is
    //    never written to a conflicted object, whatever the schedule says.
    //    No drain clock runs here, because the machine controller owns the
    //    node's drain and its timeout; a second clock against the same event
    //    is the mistake ADR 0011 avoided by leaving drain where the domain
    //    knowledge is.
    if input.conflict {
        return CapacityDecision {
            phase: PHASE_CAPACITY_ERROR,
            write: write_if_changed(input.last_written, CAPACITY_INACTIVE_VALUE),
            handback_deadline: None,
            reason: REASON_HOST_GOVERNANCE_CONFLICT,
            message: format!(
                "node is also governed by a ScheduledMachine in this namespace; \
                 capacity driven to {CAPACITY_INACTIVE_VALUE} and the active value \
                 withheld until the overlap is resolved"
            ),
            ready: false,
            handback_complete: false,
            conflict: true,
        };
    }

    // 2. The owned object cannot be created or written: the CRD is absent, the
    //    group is not allowed, the name is taken by an object 5-Spot does not
    //    own, or RBAC says no. Hold without writing and let the
    //    TargetResolved condition say which.
    if !input.target_resolved {
        return CapacityDecision::hold(
            PHASE_CAPACITY_PENDING,
            REASON_TARGET_NOT_ACTUABLE,
            "spec.target is not actuable; holding without writing".to_string(),
        );
    }

    // 3. killSwitch: immediate and terminal, by operator demand. This is the
    //    one path that does not wait for a drain, because the operator has
    //    explicitly asked for the slice back now. It is still only a field
    //    write: nothing is deleted.
    if input.kill_switch {
        return CapacityDecision {
            phase: PHASE_CAPACITY_TERMINATED,
            write: write_if_changed(input.last_written, CAPACITY_INACTIVE_VALUE),
            handback_deadline: None,
            reason: REASON_CAPACITY_INACTIVE,
            message: "spec.killSwitch is set; capacity driven to zero immediately".to_string(),
            ready: false,
            handback_complete: true,
            conflict: false,
        };
    }

    // 4. The shared should-be-active rule. `compose_should_be_active` is the
    //    machine controller's own function: Active/Inactive pass through, and
    //    Unresolved holds the last known value rather than meaning Inactive.
    let schedule_active = compose_should_be_active(input.verdict, input.last_known_active);
    let should_be_active = input.enabled && schedule_active;

    if should_be_active {
        return CapacityDecision {
            phase: PHASE_CAPACITY_ACTIVE,
            write: write_if_changed(input.last_written, input.active_value),
            // Reopening the window clears any handback wait: the slice is
            // conceded again, so there is nothing to hand back.
            handback_deadline: None,
            reason: REASON_CAPACITY_ACTIVE,
            message: format!(
                "schedule is active; capacity held at {}",
                input.active_value
            ),
            ready: true,
            handback_complete: false,
            conflict: false,
        };
    }

    // Should be inactive. Zero goes on now, whether or not a drain wait
    // follows: it is what makes the drain converge.
    let write = write_if_changed(input.last_written, CAPACITY_INACTIVE_VALUE);

    // The phase a completed handback lands in: `Disabled` names the *reason* it
    // is inactive when an operator turned it off, which `Inactive` would hide.
    let settled_phase = if input.enabled {
        PHASE_CAPACITY_INACTIVE
    } else {
        PHASE_CAPACITY_DISABLED
    };

    // No drain signal configured: nothing to observe, so handback is complete
    // as soon as zero is written (ADR 0011 decision 6, as amended).
    if !input.has_drain_signal {
        return CapacityDecision {
            phase: settled_phase,
            write,
            handback_deadline: None,
            reason: REASON_HANDBACK_NOT_OBSERVED,
            message: "capacity zeroed; no spec.handback.drainedPath configured, \
                      so handback is complete"
                .to_string(),
            ready: false,
            handback_complete: true,
            conflict: false,
        };
    }

    // Drained: the consumer reports nothing in use. `Some(0)` only, never
    // `None`: an unread counter is not a drained one.
    if input.observed_drained == Some(CAPACITY_INACTIVE_VALUE) {
        return CapacityDecision {
            phase: settled_phase,
            write,
            handback_deadline: None,
            reason: REASON_CAPACITY_INACTIVE,
            message: "capacity zeroed and the consumer reports the slice drained".to_string(),
            ready: false,
            handback_complete: true,
            conflict: false,
        };
    }

    // Still in use. Start the clock if it is not already running.
    let deadline = input
        .handback_deadline
        .unwrap_or(input.now + input.handback_timeout);

    if input.now >= deadline {
        // Deadline passed. HOLD: the field stays at zero, nothing is deleted,
        // nothing is forced. The report is the whole point of this phase.
        return CapacityDecision {
            phase: PHASE_CAPACITY_HANDBACK_TIMED_OUT,
            write,
            handback_deadline: Some(deadline),
            reason: REASON_HANDBACK_TIMED_OUT,
            message: format!(
                "handback timeout expired with {} still in use; holding capacity at {} \
                 and not forcing, because a missed handover is recoverable and \
                 killed work is not",
                describe_drained(input.observed_drained),
                CAPACITY_INACTIVE_VALUE
            ),
            ready: false,
            handback_complete: false,
            conflict: false,
        };
    }

    CapacityDecision {
        phase: PHASE_CAPACITY_HANDING_BACK,
        write,
        handback_deadline: Some(deadline),
        reason: REASON_HANDBACK_WAITING,
        message: format!(
            "capacity zeroed; waiting for the consumer to drain ({} still in use)",
            describe_drained(input.observed_drained)
        ),
        ready: false,
        handback_complete: false,
        conflict: false,
    }
}

/// `Some(desired)` when the target does not already hold it, else `None`.
///
/// Suppressing the no-op write is not just tidiness: an unconditional merge
/// patch every reconcile would bump `resourceVersion`, wake the consumer's own
/// controller, and in a shared-watch setup feed a reconcile loop. The machine
/// controller learned this the hard way with its provider status patches.
fn write_if_changed(last_written: Option<i64>, desired: i64) -> Option<i64> {
    if last_written == Some(desired) {
        return None;
    }
    Some(desired)
}

/// Render the observed drain counter for a status message, distinguishing
/// "not reported" from a number.
fn describe_drained(observed: Option<i64>) -> String {
    match observed {
        Some(value) => value.to_string(),
        None => "an unreported amount".to_string(),
    }
}

#[cfg(test)]
#[path = "capacity_decision_tests.rs"]
mod tests;
