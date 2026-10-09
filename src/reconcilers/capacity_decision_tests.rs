// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};

    const ACTIVE_VALUE: i64 = 10;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, 12, 0, 0)
            .single()
            .expect("valid instant")
    }

    fn active() -> SpotScheduleVerdict {
        SpotScheduleVerdict::Active {
            provider_generation: Some(1),
        }
    }

    fn inactive() -> SpotScheduleVerdict {
        SpotScheduleVerdict::Inactive {
            provider_generation: Some(1),
        }
    }

    fn unresolved() -> SpotScheduleVerdict {
        SpotScheduleVerdict::Unresolved {
            reason: "ProviderNotFound",
            message: "gone".to_string(),
        }
    }

    /// Baseline: enabled, resolved target, a drain signal configured, nothing
    /// written yet, schedule active. Tests vary one field at a time from here.
    fn input<'a>(verdict: &'a SpotScheduleVerdict) -> CapacityDecisionInput<'a> {
        CapacityDecisionInput {
            enabled: true,
            kill_switch: false,
            active_value: ACTIVE_VALUE,
            verdict,
            last_known_active: None,
            last_written: None,
            target_resolved: true,
            has_drain_signal: true,
            observed_drained: Some(0),
            handback_timeout: ChronoDuration::minutes(10),
            handback_deadline: None,
            conflict: false,
            now: now(),
        }
    }

    // ========================================================================
    // Precedence 1: host governance conflict fails closed
    // ========================================================================

    /// ADR-0014 amends ADR-0011 decision 7. A conflict drives capacity to
    /// ZERO; it does not refuse to write.
    ///
    /// "Refuse to write" sounded like failing closed but was not: a value
    /// written before the conflict appeared simply stood, leaving a node
    /// drained for handover while the consumer still believed it could size
    /// guests on it. Observed live, with `HostGovernanceConflict=True` and the
    /// target still holding the active value.
    #[test]
    fn test_conflict_drives_capacity_to_zero() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            conflict: true,
            last_written: Some(ACTIVE_VALUE),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ERROR);
        assert_eq!(decision.reason, REASON_HOST_GOVERNANCE_CONFLICT);
        assert_eq!(
            decision.write,
            Some(CAPACITY_INACTIVE_VALUE),
            "zero is the safe direction: it stops replenishment on a node that \
             is being drained for handover"
        );
        assert!(decision.conflict);
        assert!(!decision.ready);
    }

    /// The "fail closed" intent of decision 7 is preserved exactly where it
    /// matters: a conflicted object can never concede capacity, whatever the
    /// schedule says.
    #[test]
    fn test_conflict_never_writes_the_active_value() {
        for verdict in [active(), inactive(), unresolved()] {
            for last_known in [None, Some(true), Some(false)] {
                for last in [None, Some(0), Some(ACTIVE_VALUE)] {
                    let decision = decide(&CapacityDecisionInput {
                        conflict: true,
                        last_known_active: last_known,
                        last_written: last,
                        ..input(&verdict)
                    });
                    assert_ne!(
                        decision.write,
                        Some(ACTIVE_VALUE),
                        "a conflicted object must never write the active value"
                    );
                }
            }
        }
    }

    /// Already at zero: nothing to write, but the object stays loudly in
    /// Error. It must not drift to Inactive, because it is not inactive by
    /// schedule, it is misconfigured.
    #[test]
    fn test_conflict_already_zero_writes_nothing_but_stays_in_error() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            conflict: true,
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ERROR);
        assert_eq!(decision.write, None, "no redundant patch");
        assert!(decision.conflict);
    }

    /// No handback is claimed. A contradiction was stopped; nothing was
    /// handed back, and the machine controller owns the node's drain and its
    /// timeout (ADR-0014 decision 3).
    #[test]
    fn test_conflict_claims_no_handback_and_runs_no_drain_clock() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            conflict: true,
            last_written: Some(ACTIVE_VALUE),
            observed_drained: Some(3),
            ..input(&verdict)
        });
        assert!(
            !decision.handback_complete,
            "a conflict is not a completed handback"
        );
        assert_eq!(
            decision.handback_deadline, None,
            "no second clock against a drain the machine controller owns"
        );
    }

    /// Conflict outranks everything, including killSwitch. Only the branch's
    /// action changed in ADR-0014, not its precedence.
    #[test]
    fn test_conflict_outranks_kill_switch_and_schedule() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            conflict: true,
            kill_switch: true,
            last_written: Some(ACTIVE_VALUE),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ERROR);
        assert_eq!(decision.write, Some(CAPACITY_INACTIVE_VALUE));
        assert_eq!(
            decision.reason, REASON_HOST_GOVERNANCE_CONFLICT,
            "the conflict is the reason reported, not the kill switch"
        );
    }

    /// Recovery needs no operator action beyond fixing the reference: the
    /// next reconcile is an ordinary one.
    #[test]
    fn test_clearing_the_conflict_restores_capacity() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            conflict: false,
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ACTIVE);
        assert_eq!(decision.write, Some(ACTIVE_VALUE));
        assert!(!decision.conflict);
    }

    // ========================================================================
    // Precedence 2: unresolved target
    // ========================================================================

    #[test]
    fn test_unresolved_target_holds_without_writing() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            target_resolved: false,
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_PENDING);
        assert_eq!(decision.reason, REASON_TARGET_NOT_ACTUABLE);
        assert_eq!(decision.write, None, "never a create (ADR 0011 decision 2)");
    }

    // ========================================================================
    // Precedence 3: killSwitch
    // ========================================================================

    #[test]
    fn test_kill_switch_zeroes_immediately_without_waiting() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            kill_switch: true,
            last_written: Some(ACTIVE_VALUE),
            observed_drained: Some(5), // still in use, deliberately
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_TERMINATED);
        assert_eq!(decision.write, Some(CAPACITY_INACTIVE_VALUE));
        assert!(
            decision.handback_complete,
            "killSwitch is terminal by operator demand; it does not wait for a drain"
        );
        assert_eq!(decision.handback_deadline, None);
    }

    #[test]
    fn test_kill_switch_outranks_an_active_schedule() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            kill_switch: true,
            ..input(&verdict)
        });
        assert_ne!(decision.phase, PHASE_CAPACITY_ACTIVE);
    }

    // ========================================================================
    // Active
    // ========================================================================

    #[test]
    fn test_active_schedule_writes_the_active_value() {
        let verdict = active();
        let decision = decide(&input(&verdict));
        assert_eq!(decision.phase, PHASE_CAPACITY_ACTIVE);
        assert_eq!(decision.write, Some(ACTIVE_VALUE));
        assert_eq!(decision.reason, REASON_CAPACITY_ACTIVE);
        assert!(decision.ready, "ready is true only in Active");
    }

    /// The no-op suppression that keeps the controller from bumping
    /// resourceVersion on every reconcile and waking the consumer's controller.
    #[test]
    fn test_active_does_not_rewrite_a_value_already_correct() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(ACTIVE_VALUE),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ACTIVE);
        assert_eq!(decision.write, None, "already correct; no patch");
        assert!(decision.ready);
    }

    #[test]
    fn test_active_rewrites_when_the_value_changed() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(4),
            ..input(&verdict)
        });
        assert_eq!(decision.write, Some(ACTIVE_VALUE));
    }

    /// Reopening the window mid-handback must clear the wait and restore
    /// capacity. This reversibility is what ADR 0011 bought by scaling rather
    /// than deleting.
    #[test]
    fn test_schedule_reopening_mid_handback_restores_capacity_and_clears_deadline() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(3),
            handback_deadline: Some(now() + ChronoDuration::minutes(5)),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ACTIVE);
        assert_eq!(decision.write, Some(ACTIVE_VALUE));
        assert_eq!(decision.handback_deadline, None);
    }

    // ========================================================================
    // spec.enabled
    // ========================================================================

    #[test]
    fn test_disabled_zeroes_and_reports_disabled_not_inactive() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            enabled: false,
            last_written: Some(ACTIVE_VALUE),
            observed_drained: Some(0),
            ..input(&verdict)
        });
        assert_eq!(
            decision.phase, PHASE_CAPACITY_DISABLED,
            "Disabled names WHY it is inactive, which Inactive would hide"
        );
        assert_eq!(decision.write, Some(CAPACITY_INACTIVE_VALUE));
        assert!(decision.handback_complete);
    }

    /// Disabled still goes through the drain wait: it is an administrative
    /// off-switch, not an emergency, so it owes the consumer the same courtesy
    /// an inactive schedule does.
    #[test]
    fn test_disabled_still_waits_for_the_drain() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            enabled: false,
            last_written: Some(ACTIVE_VALUE),
            observed_drained: Some(2),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_HANDING_BACK);
        assert!(!decision.handback_complete);
    }

    // ========================================================================
    // Unresolved provider: hold last state, never treat as inactive
    // ========================================================================

    #[test]
    fn test_unresolved_holds_last_known_active() {
        let verdict = unresolved();
        let decision = decide(&CapacityDecisionInput {
            last_known_active: Some(true),
            last_written: Some(ACTIVE_VALUE),
            ..input(&verdict)
        });
        assert_eq!(
            decision.phase, PHASE_CAPACITY_ACTIVE,
            "Unresolved must never mean Inactive (ADR 0006 §4)"
        );
        assert_eq!(decision.write, None, "held value is already correct");
    }

    #[test]
    fn test_unresolved_holds_last_known_inactive() {
        let verdict = unresolved();
        let decision = decide(&CapacityDecisionInput {
            last_known_active: Some(false),
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(0),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_INACTIVE);
    }

    /// Never resolved at all: fail inactive rather than concede capacity on the
    /// strength of a provider nobody has heard from.
    #[test]
    fn test_unresolved_and_never_known_fails_inactive() {
        let verdict = unresolved();
        let decision = decide(&CapacityDecisionInput {
            last_known_active: None,
            observed_drained: Some(0),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_INACTIVE);
        assert_eq!(decision.write, Some(CAPACITY_INACTIVE_VALUE));
    }

    // ========================================================================
    // Handback
    // ========================================================================

    #[test]
    fn test_deactivation_writes_zero_first_then_waits() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(ACTIVE_VALUE),
            observed_drained: Some(3),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_HANDING_BACK);
        assert_eq!(
            decision.write,
            Some(CAPACITY_INACTIVE_VALUE),
            "zero goes on immediately: it is what makes the drain converge"
        );
        assert_eq!(decision.reason, REASON_HANDBACK_WAITING);
        assert_eq!(
            decision.handback_deadline,
            Some(now() + ChronoDuration::minutes(10)),
            "the clock starts on this reconcile"
        );
        assert!(!decision.handback_complete);
    }

    #[test]
    fn test_existing_deadline_is_not_restarted() {
        let verdict = inactive();
        let existing = now() + ChronoDuration::minutes(2);
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(3),
            handback_deadline: Some(existing),
            ..input(&verdict)
        });
        assert_eq!(
            decision.handback_deadline,
            Some(existing),
            "a running clock must not be reset by a reconcile, or it never expires"
        );
    }

    #[test]
    fn test_drain_reaching_zero_completes_handback() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(0),
            handback_deadline: Some(now() + ChronoDuration::minutes(5)),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_INACTIVE);
        assert!(decision.handback_complete);
        assert_eq!(
            decision.handback_deadline, None,
            "a completed handback clears its deadline"
        );
        assert_eq!(decision.write, None, "zero is already written");
    }

    /// The distinction the whole handback turns on: an unread counter is not a
    /// drained one, so `None` must keep waiting rather than complete.
    #[test]
    fn test_unread_drain_counter_is_not_drained() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: None,
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_HANDING_BACK);
        assert!(!decision.handback_complete);
        assert!(
            decision.message.contains("unreported"),
            "the message must say the counter was not read, got: {}",
            decision.message
        );
    }

    #[test]
    fn test_no_drain_signal_completes_on_the_zero_write() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            has_drain_signal: false,
            last_written: Some(ACTIVE_VALUE),
            observed_drained: None,
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_INACTIVE);
        assert_eq!(decision.write, Some(CAPACITY_INACTIVE_VALUE));
        assert!(decision.handback_complete);
        assert_eq!(decision.reason, REASON_HANDBACK_NOT_OBSERVED);
        assert_eq!(decision.handback_deadline, None);
    }

    // ========================================================================
    // Handback timeout: HOLD, never force (ADR 0011 decision 6)
    // ========================================================================

    #[test]
    fn test_expired_deadline_holds_and_reports_loudly() {
        let verdict = inactive();
        let expired = now() - ChronoDuration::seconds(1);
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(3),
            handback_deadline: Some(expired),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_HANDBACK_TIMED_OUT);
        assert_eq!(decision.reason, REASON_HANDBACK_TIMED_OUT);
        assert!(!decision.handback_complete, "it did NOT complete");
        assert_eq!(
            decision.write, None,
            "zero is already written; there is nothing further to write and \
             nothing is forced or deleted"
        );
        assert_eq!(
            decision.handback_deadline,
            Some(expired),
            "the expired deadline is kept so operators can see when it lapsed"
        );
        assert!(
            decision.message.contains("not forcing"),
            "the message must say it is not forcing, got: {}",
            decision.message
        );
    }

    /// Exactly at the deadline counts as expired: a wait that needs one more
    /// nanosecond is an off-by-one waiting to happen.
    #[test]
    fn test_deadline_exactly_now_is_expired() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(1),
            handback_deadline: Some(now()),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_HANDBACK_TIMED_OUT);
    }

    /// A timed-out handback that finally drains must recover, not stay stuck in
    /// the timed-out phase forever.
    #[test]
    fn test_timed_out_handback_recovers_when_the_drain_finally_completes() {
        let verdict = inactive();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(0),
            handback_deadline: Some(now() - ChronoDuration::hours(1)),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_INACTIVE);
        assert!(decision.handback_complete);
    }

    /// And a timed-out handback whose window reopens must go straight back to
    /// Active: the field write was reversible all along.
    #[test]
    fn test_timed_out_handback_reactivates_when_the_window_reopens() {
        let verdict = active();
        let decision = decide(&CapacityDecisionInput {
            last_written: Some(CAPACITY_INACTIVE_VALUE),
            observed_drained: Some(3),
            handback_deadline: Some(now() - ChronoDuration::hours(1)),
            ..input(&verdict)
        });
        assert_eq!(decision.phase, PHASE_CAPACITY_ACTIVE);
        assert_eq!(decision.write, Some(ACTIVE_VALUE));
        assert_eq!(decision.handback_deadline, None);
    }

    // ========================================================================
    // Invariants across the whole decision space
    // ========================================================================

    /// Nothing but zero or the configured active value may ever be written. A
    /// decision that wrote anything else would be writing a number nobody
    /// declared.
    #[test]
    fn test_only_zero_or_the_active_value_is_ever_written() {
        let verdicts = [active(), inactive(), unresolved()];
        let mut checked = 0_usize;
        for verdict in &verdicts {
            for enabled in [true, false] {
                for kill_switch in [true, false] {
                    for conflict in [true, false] {
                        for target_resolved in [true, false] {
                            for has_drain_signal in [true, false] {
                                for observed in [None, Some(0), Some(7)] {
                                    for last in [None, Some(0), Some(ACTIVE_VALUE), Some(3)] {
                                        for deadline in [
                                            None,
                                            Some(now() - ChronoDuration::hours(1)),
                                            Some(now() + ChronoDuration::hours(1)),
                                        ] {
                                            for last_known in [None, Some(true), Some(false)] {
                                                let decision = decide(&CapacityDecisionInput {
                                                    enabled,
                                                    kill_switch,
                                                    conflict,
                                                    target_resolved,
                                                    has_drain_signal,
                                                    observed_drained: observed,
                                                    last_written: last,
                                                    handback_deadline: deadline,
                                                    last_known_active: last_known,
                                                    ..input(verdict)
                                                });
                                                checked += 1;
                                                if let Some(value) = decision.write {
                                                    assert!(
                                                        value == CAPACITY_INACTIVE_VALUE
                                                            || value == ACTIVE_VALUE,
                                                        "wrote {value}, which is neither zero \
                                                         nor the active value"
                                                    );
                                                }
                                                // ready implies Active, always.
                                                assert_eq!(
                                                    decision.ready,
                                                    decision.phase == PHASE_CAPACITY_ACTIVE
                                                );
                                                // A conflict never writes the
                                                // ACTIVE value (ADR-0014); it
                                                // may write zero.
                                                if decision.conflict {
                                                    assert_ne!(decision.write, Some(ACTIVE_VALUE));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 2_000, "expected a broad sweep, ran {checked}");
    }

    /// `write` must never be a no-op patch: if it is `Some`, the value differs
    /// from what was last written.
    #[test]
    fn test_write_is_never_a_redundant_patch() {
        let verdicts = [active(), inactive()];
        for verdict in &verdicts {
            for last in [None, Some(0), Some(ACTIVE_VALUE)] {
                for enabled in [true, false] {
                    let decision = decide(&CapacityDecisionInput {
                        last_written: last,
                        enabled,
                        observed_drained: Some(0),
                        ..input(verdict)
                    });
                    if let Some(value) = decision.write {
                        assert_ne!(
                            Some(value),
                            last,
                            "would have patched {value} when it was already written"
                        );
                    }
                }
            }
        }
    }
}
