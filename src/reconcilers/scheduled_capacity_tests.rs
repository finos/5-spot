// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::crd::{ScheduledCapacity, ScheduledCapacitySpec, ScheduledCapacityStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;

    fn capacity_with(spec_json: serde_json::Value) -> ScheduledCapacity {
        let spec: ScheduledCapacitySpec =
            serde_json::from_value(spec_json).expect("valid ScheduledCapacitySpec");
        ScheduledCapacity {
            metadata: ObjectMeta {
                name: Some("scap-a".to_string()),
                namespace: Some("sandboxes".to_string()),
                generation: Some(2),
                ..ObjectMeta::default()
            },
            spec,
            status: None,
        }
    }

    fn base_spec() -> serde_json::Value {
        json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "CapitalMarketsSchedule",
                "name": "nyse"
            },
            "targetRef": {
                "apiVersion": "banlieue.io/v1alpha1",
                "kind": "VirtualMachinePool",
                "name": "pool-a"
            },
            "capacity": { "path": "spec.warmReplicas", "activeValue": 10 }
        })
    }

    // ========================================================================
    // parse_handback_timeout
    // ========================================================================

    #[test]
    fn test_parse_handback_timeout_reads_the_configured_value() {
        let mut spec = base_spec();
        spec["handback"] = json!({ "drainedPath": "status.claimed", "timeout": "5m" });
        let capacity = capacity_with(spec);
        assert_eq!(
            parse_handback_timeout(&capacity).expect("parses"),
            ChronoDuration::minutes(5)
        );
    }

    #[test]
    fn test_parse_handback_timeout_uses_the_crd_default_when_omitted() {
        let mut spec = base_spec();
        spec["handback"] = json!({ "drainedPath": "status.claimed" });
        let capacity = capacity_with(spec);
        assert_eq!(
            parse_handback_timeout(&capacity).expect("parses"),
            ChronoDuration::minutes(10),
            "the CRD default is 10m, so the wait is never unbounded"
        );
    }

    /// No handback block means no wait at all, so the timeout is irrelevant and
    /// must not be an error.
    #[test]
    fn test_parse_handback_timeout_is_zero_without_a_handback_block() {
        let capacity = capacity_with(base_spec());
        assert_eq!(
            parse_handback_timeout(&capacity).expect("parses"),
            ChronoDuration::seconds(0)
        );
    }

    #[test]
    fn test_parse_handback_timeout_rejects_garbage() {
        let mut spec = base_spec();
        spec["handback"] = json!({ "drainedPath": "status.claimed", "timeout": "soon" });
        let capacity = capacity_with(spec);
        let error = parse_handback_timeout(&capacity).expect_err("not a duration");
        assert!(matches!(error, CapacityError::Timeout(_)), "got {error:?}");
    }

    #[test]
    fn test_parse_handback_timeout_accepts_seconds_and_hours() {
        for (value, expected) in [
            ("30s", ChronoDuration::seconds(30)),
            ("1h", ChronoDuration::hours(1)),
        ] {
            let mut spec = base_spec();
            spec["handback"] = json!({ "drainedPath": "status.claimed", "timeout": value });
            let capacity = capacity_with(spec);
            assert_eq!(parse_handback_timeout(&capacity).expect("parses"), expected);
        }
    }

    // ========================================================================
    // parse_rfc3339
    // ========================================================================

    #[test]
    fn test_parse_rfc3339_round_trips_a_deadline() {
        let instant = Utc::now();
        let parsed = parse_rfc3339(&instant.to_rfc3339()).expect("parses");
        assert_eq!(parsed.timestamp(), instant.timestamp());
    }

    /// A corrupted status timestamp is treated as "no deadline recorded" rather
    /// than failing the reconcile: the controller would otherwise be wedged by
    /// a field only it writes.
    #[test]
    fn test_parse_rfc3339_returns_none_on_garbage() {
        for value in ["", "not-a-date", "2026-13-45T99:99:99Z", "1759500000"] {
            assert!(parse_rfc3339(value).is_none(), "{value} must not parse");
        }
    }

    // ========================================================================
    // requeue_for
    // ========================================================================

    /// An observation with nothing resolved and nothing written: the shape a
    /// first reconcile against a missing target produces.
    fn null_observation() -> ObservedState {
        ObservedState {
            written: None,
            observed_drained: None,
            target_ref: serde_json::Value::Null,
            now: Utc::now(),
        }
    }

    fn decision_in(phase: &'static str) -> CapacityDecision {
        CapacityDecision {
            phase,
            write: None,
            handback_deadline: None,
            reason: "R",
            message: "m".to_string(),
            ready: false,
            handback_complete: false,
            conflict: false,
        }
    }

    /// The handback requeue is a deadline backstop, so it must be materially
    /// shorter than the steady-state one, else an expired deadline could go
    /// unnoticed for minutes.
    #[test]
    fn test_handing_back_requeues_sooner_than_steady_state() {
        let handing_back = requeue_for(&decision_in(PHASE_CAPACITY_HANDING_BACK));
        let active = requeue_for(&decision_in(PHASE_CAPACITY_ACTIVE));
        assert_eq!(
            handing_back,
            Action::requeue(Duration::from_secs(CAPACITY_HANDBACK_DEADLINE_CHECK_SECS))
        );
        assert_eq!(
            active,
            Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS))
        );
        // A compile-time invariant, so assert it as one.
        const {
            assert!(
                CAPACITY_HANDBACK_DEADLINE_CHECK_SECS < DEFAULT_REQUEUE_SECS,
                "the backstop must be shorter than the steady-state requeue"
            )
        };
    }

    /// A timed-out handback is a settled state: it must NOT keep the short
    /// backstop interval, or a stuck consumer would be polled forever.
    #[test]
    fn test_timed_out_requeues_at_steady_state_not_the_backstop() {
        assert_eq!(
            requeue_for(&decision_in(PHASE_CAPACITY_HANDBACK_TIMED_OUT)),
            Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS))
        );
    }

    #[test]
    fn test_every_phase_requeues_eventually() {
        for phase in [
            crate::constants::PHASE_CAPACITY_PENDING,
            PHASE_CAPACITY_ACTIVE,
            PHASE_CAPACITY_HANDING_BACK,
            PHASE_CAPACITY_HANDBACK_TIMED_OUT,
            crate::constants::PHASE_CAPACITY_INACTIVE,
            crate::constants::PHASE_CAPACITY_DISABLED,
            crate::constants::PHASE_CAPACITY_TERMINATED,
            crate::constants::PHASE_CAPACITY_ERROR,
        ] {
            // No phase may return `await_change()`: a ScheduledCapacity with no
            // further events must still re-check its deadline and its target.
            assert_ne!(
                requeue_for(&decision_in(phase)),
                Action::await_change(),
                "{phase} must requeue"
            );
        }
    }

    // ========================================================================
    // verdict_message
    // ========================================================================

    #[test]
    fn test_verdict_message_distinguishes_the_three_outcomes() {
        let active = verdict_message(&SpotScheduleVerdict::Active {
            provider_generation: None,
        });
        let inactive = verdict_message(&SpotScheduleVerdict::Inactive {
            provider_generation: None,
        });
        let unresolved = verdict_message(&SpotScheduleVerdict::Unresolved {
            reason: "ProviderNotFound",
            message: "pool-a is gone".to_string(),
        });
        assert!(active.contains("active"));
        assert!(inactive.contains("inactive"));
        assert_eq!(
            unresolved, "pool-a is gone",
            "an unresolved verdict must surface the provider's own detail, \
             not a generic string"
        );
    }

    // ========================================================================
    // Leader gating
    // ========================================================================

    /// A non-leader must do no work at all and wake instantly on promotion.
    /// It must not patch a status, which would make two replicas fight.
    #[tokio::test]
    async fn test_non_leader_does_nothing() {
        use http::{Request, Response};
        use kube::client::Body;
        use tower_test::mock;

        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let ctx = Arc::new(CapacityContext {
            client: kube::Client::new(svc, "default"),
            // No responder is wired up, so if the reconciler tried any API call
            // it would hang or error rather than return await_change().
            is_leader: Arc::new(AtomicBool::new(false)),
        });
        let action = reconcile(Arc::new(capacity_with(base_spec())), ctx)
            .await
            .expect("non-leader short-circuits without touching the API");
        assert_eq!(action, Action::await_change());
    }

    // ========================================================================
    // Path re-validation in the reconciler (defence against a stale CRD)
    // ========================================================================

    /// Admission should reject these, but a cluster running an older CRD would
    /// let them through. The reconciler is the check that still holds, so it
    /// must refuse before any API call is made.
    #[tokio::test]
    async fn test_reconcile_rejects_an_illegal_write_path_before_any_api_call() {
        use http::{Request, Response};
        use kube::client::Body;
        use tower_test::mock;

        for bad_path in [
            "metadata.ownerReferences",
            "metadata.finalizers",
            "status.claimed",
            "spec.items[0].count",
        ] {
            let mut spec = base_spec();
            spec["capacity"]["path"] = json!(bad_path);
            // Deserialising the spec itself is fine: the CRD schema is the
            // admission gate, and this test is about what happens when that
            // gate was an older schema.
            let capacity = capacity_with(spec);

            let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
            let ctx = Arc::new(CapacityContext {
                client: kube::Client::new(svc, "default"),
                is_leader: Arc::new(AtomicBool::new(true)),
            });
            let error = reconcile(Arc::new(capacity), ctx)
                .await
                .expect_err(bad_path);
            assert!(
                matches!(error, CapacityError::Path(_)),
                "{bad_path} must be refused as a path error, got {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_reconcile_rejects_an_illegal_drained_path() {
        use http::{Request, Response};
        use kube::client::Body;
        use tower_test::mock;

        let mut spec = base_spec();
        spec["handback"] = json!({ "drainedPath": "spec.warmReplicas", "timeout": "5m" });
        let capacity = capacity_with(spec);

        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let ctx = Arc::new(CapacityContext {
            client: kube::Client::new(svc, "default"),
            is_leader: Arc::new(AtomicBool::new(true)),
        });
        let error = reconcile(Arc::new(capacity), ctx)
            .await
            .expect_err("spec. drain path");
        assert!(matches!(error, CapacityError::Path(_)), "got {error:?}");
    }

    // ========================================================================
    // Unnamespaced
    // ========================================================================

    #[tokio::test]
    async fn test_reconcile_rejects_an_unnamespaced_object() {
        use http::{Request, Response};
        use kube::client::Body;
        use tower_test::mock;

        let mut capacity = capacity_with(base_spec());
        capacity.metadata.namespace = None;

        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let ctx = Arc::new(CapacityContext {
            client: kube::Client::new(svc, "default"),
            is_leader: Arc::new(AtomicBool::new(true)),
        });
        let error = reconcile(Arc::new(capacity), ctx)
            .await
            .expect_err("no namespace");
        assert!(
            matches!(error, CapacityError::Unnamespaced),
            "got {error:?}"
        );
    }

    // ========================================================================
    // Host governance conflict detection: the no-nodeName short circuit
    // ========================================================================

    /// Without `spec.nodeName` there is nothing to correlate, so the check must
    /// return false **without listing ScheduledMachines** (an API call the mock
    /// client has no responder for).
    #[tokio::test]
    async fn test_conflict_detection_short_circuits_without_a_node_name() {
        use http::{Request, Response};
        use kube::client::Body;
        use tower_test::mock;

        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(svc, "default");
        let capacity = capacity_with(base_spec());
        assert!(capacity.spec.node_name.is_none());

        let conflict = detect_host_governance_conflict(&client, "sandboxes", &capacity)
            .await
            .expect("no API call needed");
        assert!(!conflict);
    }

    // ========================================================================
    // error_policy
    // ========================================================================

    // `error_policy` is synchronous, but building the mock client it needs a
    // context for requires a Tokio reactor, hence the async test.
    #[tokio::test]
    async fn test_error_policy_requeues_with_backoff() {
        let ctx = Arc::new(CapacityContext {
            client: {
                use http::{Request, Response};
                use kube::client::Body;
                use tower_test::mock;
                let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
                kube::Client::new(svc, "default")
            },
            is_leader: Arc::new(AtomicBool::new(true)),
        });
        let action = error_policy(
            Arc::new(capacity_with(base_spec())),
            &CapacityError::Unnamespaced,
            ctx,
        );
        assert_eq!(
            action,
            Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS))
        );
    }

    // ========================================================================
    // Status shape
    // ========================================================================

    // ========================================================================
    // Condition timestamp stability: the hot-reconcile-loop guard
    //
    // `Condition::new` stamps `lastTransitionTime` with `now`. If that reaches
    // the status patch unconditionally, the computed status never equals the
    // stored one, so every reconcile writes, which re-triggers this
    // controller's own watch, which reconciles again: an unbounded tight loop.
    // Measured live at ~50 resourceVersion bumps/second before this guard, with
    // the handback-timeout counter reaching 100 in seconds.
    // ========================================================================

    fn condition(kind: &str, status: &str, stamp: &str) -> Condition {
        Condition {
            r#type: kind.to_string(),
            status: status.to_string(),
            last_transition_time: stamp.to_string(),
            reason: "R".to_string(),
            message: "m".to_string(),
        }
    }

    #[test]
    fn test_unchanged_condition_keeps_its_original_timestamp() {
        let previous = vec![condition("Ready", "False", "2026-01-01T00:00:00+00:00")];
        let computed = vec![condition("Ready", "False", "2026-10-03T12:00:00+00:00")];
        let merged = merge_condition_timestamps(computed, &previous);
        assert_eq!(
            merged[0].last_transition_time, "2026-01-01T00:00:00+00:00",
            "an unchanged condition must keep its original timestamp, else the \
             status differs on every reconcile and the controller loops"
        );
    }

    #[test]
    fn test_changed_condition_gets_the_new_timestamp() {
        let previous = vec![condition("Ready", "False", "2026-01-01T00:00:00+00:00")];
        let computed = vec![condition("Ready", "True", "2026-10-03T12:00:00+00:00")];
        let merged = merge_condition_timestamps(computed, &previous);
        assert_eq!(
            merged[0].last_transition_time, "2026-10-03T12:00:00+00:00",
            "a real status transition must be stamped with the new time"
        );
    }

    #[test]
    fn test_new_condition_keeps_its_timestamp() {
        let computed = vec![condition(
            "TargetResolved",
            "True",
            "2026-10-03T12:00:00+00:00",
        )];
        let merged = merge_condition_timestamps(computed, &[]);
        assert_eq!(merged[0].last_transition_time, "2026-10-03T12:00:00+00:00");
    }

    /// Reason and message changes do NOT restamp: only `status` transitions do,
    /// per Kubernetes condition conventions. A drifting message (for example a
    /// drain counter ticking down) must not restamp, or it reopens the loop.
    #[test]
    fn test_message_change_alone_does_not_restamp() {
        let previous = vec![Condition {
            message: "3 still in use".to_string(),
            ..condition("HandbackComplete", "False", "2026-01-01T00:00:00+00:00")
        }];
        let computed = vec![Condition {
            message: "2 still in use".to_string(),
            ..condition("HandbackComplete", "False", "2026-10-03T12:00:00+00:00")
        }];
        let merged = merge_condition_timestamps(computed, &previous);
        assert_eq!(merged[0].last_transition_time, "2026-01-01T00:00:00+00:00");
        assert_eq!(
            merged[0].message, "2 still in use",
            "the message still updates"
        );
    }

    #[test]
    fn test_merge_matches_conditions_by_type_not_position() {
        let previous = vec![
            condition("TargetResolved", "True", "2026-01-01T00:00:00+00:00"),
            condition("Ready", "True", "2026-02-02T00:00:00+00:00"),
        ];
        // Same two conditions, opposite order.
        let computed = vec![
            condition("Ready", "True", "2026-10-03T12:00:00+00:00"),
            condition("TargetResolved", "True", "2026-10-03T12:00:00+00:00"),
        ];
        let merged = merge_condition_timestamps(computed, &previous);
        assert_eq!(merged[0].last_transition_time, "2026-02-02T00:00:00+00:00");
        assert_eq!(merged[1].last_transition_time, "2026-01-01T00:00:00+00:00");
    }

    /// The whole point: merging a status against itself must be a fixed point.
    /// If it is not, the controller writes forever.
    #[test]
    fn test_merging_a_status_against_itself_is_a_fixed_point() {
        let stored = vec![
            condition("Ready", "False", "2026-01-01T00:00:00+00:00"),
            condition("TargetResolved", "True", "2026-01-01T00:00:00+00:00"),
        ];
        let recomputed = vec![
            condition("Ready", "False", "2026-10-03T12:00:00+00:00"),
            condition("TargetResolved", "True", "2026-10-03T12:00:00+00:00"),
        ];
        let once = merge_condition_timestamps(recomputed.clone(), &stored);
        let twice = merge_condition_timestamps(recomputed, &once);
        assert_eq!(once, twice, "merge must converge, not oscillate");
        assert_eq!(
            once,
            stored.clone(),
            "and converge onto the stored timestamps"
        );
    }

    // ========================================================================
    // Clearable status fields
    //
    // A JSON merge patch that OMITS a key leaves the stored value untouched, so
    // an omitted `handbackDeadline` could never be cleared once set: a
    // completed handback would keep advertising a deadline forever. The keys
    // this controller owns are therefore always present, with an explicit null
    // when they should be removed.
    // ========================================================================

    #[test]
    fn test_cleared_handback_deadline_is_an_explicit_null_not_an_omission() {
        let capacity = capacity_with(base_spec());
        let decision = decision_in(crate::constants::PHASE_CAPACITY_INACTIVE);
        assert_eq!(decision.handback_deadline, None);
        let status = build_status_value(
            &capacity,
            &decision,
            &null_observation(),
            vec![],
            &SpotScheduleVerdict::Inactive {
                provider_generation: None,
            },
        );
        assert_eq!(
            status.get("handbackDeadline"),
            Some(&serde_json::Value::Null),
            "must be an explicit null so the merge patch deletes it; an omitted \
             key would leave a stale deadline on a completed handback"
        );
    }

    #[test]
    fn test_set_handback_deadline_is_serialised() {
        let capacity = capacity_with(base_spec());
        let deadline = Utc::now();
        let decision = CapacityDecision {
            handback_deadline: Some(deadline),
            ..decision_in(PHASE_CAPACITY_HANDING_BACK)
        };
        let status = build_status_value(
            &capacity,
            &decision,
            &null_observation(),
            vec![],
            &SpotScheduleVerdict::Inactive {
                provider_generation: None,
            },
        );
        assert_eq!(
            status.get("handbackDeadline").and_then(|v| v.as_str()),
            Some(deadline.to_rfc3339().as_str())
        );
    }

    #[test]
    fn test_absent_observed_drain_is_an_explicit_null() {
        let capacity = capacity_with(base_spec());
        let status = build_status_value(
            &capacity,
            &decision_in(PHASE_CAPACITY_ACTIVE),
            &null_observation(),
            vec![],
            &SpotScheduleVerdict::Inactive {
                provider_generation: None,
            },
        );
        assert_eq!(
            status.get("observedDrainedValue"),
            Some(&serde_json::Value::Null)
        );
    }

    /// `governedNode` mirrors spec.nodeName, so it is null when unset.
    #[test]
    fn test_governed_node_tracks_spec_node_name() {
        let mut spec = base_spec();
        spec["nodeName"] = json!("worker-3");
        let with_node = capacity_with(spec);
        let status = build_status_value(
            &with_node,
            &decision_in(PHASE_CAPACITY_ACTIVE),
            &null_observation(),
            vec![],
            &SpotScheduleVerdict::Inactive {
                provider_generation: None,
            },
        );
        assert_eq!(
            status.get("governedNode").and_then(|v| v.as_str()),
            Some("worker-3")
        );

        let without = capacity_with(base_spec());
        let status = build_status_value(
            &without,
            &decision_in(PHASE_CAPACITY_ACTIVE),
            &null_observation(),
            vec![],
            &SpotScheduleVerdict::Inactive {
                provider_generation: None,
            },
        );
        assert_eq!(status.get("governedNode"), Some(&serde_json::Value::Null));
    }

    /// **The regression test for the no-op guard.** Build a status, round-trip
    /// it through the typed `ScheduledCapacityStatus` the guard compares
    /// against, serialise it again, and require the two to match.
    ///
    /// A key that `build_status_value` forgets (or that some other code path
    /// sets) survives this round trip as `None` while the stored object keeps
    /// its value, so the guard can never report equality and every reconcile
    /// patches again. That is exactly how `targetRef` silently disabled the
    /// guard: the symptom was not a crash but a controller that looked idle
    /// while re-patching forever.
    #[test]
    fn test_status_round_trips_through_the_typed_status_without_losing_keys() {
        let mut spec = base_spec();
        spec["nodeName"] = json!("worker-3");
        let capacity = capacity_with(spec);
        let decision = CapacityDecision {
            handback_deadline: Some(Utc::now()),
            ..decision_in(PHASE_CAPACITY_HANDING_BACK)
        };
        let target_ref = json!({
            "apiVersion": "banlieue.io/v1alpha1",
            "kind": "VirtualMachinePool",
            "name": "pool-a",
            "namespace": "sandboxes",
            "uid": "abc-123",
        });
        let conditions = vec![condition("Ready", "False", "2026-01-01T00:00:00+00:00")];

        let built = build_status_value(
            &capacity,
            &decision,
            &ObservedState {
                written: Some(0),
                observed_drained: Some(3),
                target_ref,
                now: Utc::now(),
            },
            conditions,
            &SpotScheduleVerdict::Active {
                provider_generation: Some(7),
            },
        );

        // The guard's own comparison path.
        let typed: crate::crd::ScheduledCapacityStatus =
            serde_json::from_value(built.clone()).expect("status must deserialise");

        // Every non-null key we built must survive into the typed form.
        for key in [
            "phase",
            "writtenValue",
            "observedDrainedValue",
            "governedNode",
            "handbackDeadline",
            "targetRef",
            "conditions",
        ] {
            assert!(
                built.get(key).is_some_and(|v| !v.is_null()),
                "build_status_value must emit {key}"
            );
        }
        assert_eq!(typed.written_value, Some(0));
        assert_eq!(typed.observed_drained_value, Some(3));
        assert_eq!(typed.governed_node.as_deref(), Some("worker-3"));
        assert!(typed.handback_deadline.is_some());
        assert!(
            typed.target_ref.is_some(),
            "targetRef must survive the round trip, else the no-op guard never matches"
        );

        // And the guard must consider a stored copy of this status equal, which
        // is the property that stops the controller re-patching forever.
        let stored = typed.clone();
        let rebuilt: crate::crd::ScheduledCapacityStatus =
            serde_json::from_value(built).expect("re-deserialise");
        assert_eq!(
            stored, rebuilt,
            "the guard comparison must be a fixed point"
        );
    }

    /// **The invariant that keeps the no-op guard alive.**
    ///
    /// Every field of `ScheduledCapacityStatus` must be emitted by
    /// `build_status_value`. A field declared on the type but missing from the
    /// builder deserialises to `None` while the stored object keeps its value,
    /// so the guard's equality check can never succeed and the controller
    /// re-patches on every reconcile, forever.
    ///
    /// This is not hypothetical: rewriting `patch_status` dropped `targetRef`
    /// and `spotSchedule`, and the symptom was not a crash or a failing test
    /// but a controller that looked idle while silently patching. Comparing
    /// against the serialised type, rather than a hand-written list of keys,
    /// is what makes this test catch the next one.
    #[test]
    fn test_build_status_value_emits_every_field_of_the_status_type() {
        use crate::crd::{ScheduledCapacityStatus, SpotScheduleStatus};

        // A status with every Option populated, so serde emits every key.
        let fully_populated = ScheduledCapacityStatus {
            phase: Some("Active".to_string()),
            message: Some("m".to_string()),
            written_value: Some(1),
            last_write_time: Some("2026-01-01T00:00:00+00:00".to_string()),
            target_ref: Some(crate::crd::ObjectReference {
                api_version: "banlieue.io/v1alpha1".to_string(),
                kind: "VirtualMachinePool".to_string(),
                name: "pool-a".to_string(),
                namespace: Some("sandboxes".to_string()),
            }),
            governed_node: Some("worker-3".to_string()),
            observed_drained_value: Some(0),
            handback_deadline: Some("2026-01-01T00:00:00+00:00".to_string()),
            conditions: vec![condition("Ready", "True", "2026-01-01T00:00:00+00:00")],
            observed_generation: Some(1),
            ready: true,
            spot_schedule: Some(SpotScheduleStatus::default()),
        };
        let expected_keys: std::collections::BTreeSet<String> =
            serde_json::to_value(&fully_populated)
                .expect("serialise")
                .as_object()
                .expect("object")
                .keys()
                .cloned()
                .collect();

        let mut spec = base_spec();
        spec["nodeName"] = json!("worker-3");
        let capacity = capacity_with(spec);
        let built = build_status_value(
            &capacity,
            &CapacityDecision {
                handback_deadline: Some(Utc::now()),
                ..decision_in(PHASE_CAPACITY_HANDING_BACK)
            },
            &ObservedState {
                written: Some(0),
                observed_drained: Some(3),
                target_ref: json!({"apiVersion": "banlieue.io/v1alpha1",
                                   "kind": "VirtualMachinePool",
                                   "name": "pool-a", "namespace": "sandboxes"}),
                now: Utc::now(),
            },
            vec![condition("Ready", "False", "2026-01-01T00:00:00+00:00")],
            &SpotScheduleVerdict::Active {
                provider_generation: Some(7),
            },
        );
        let built_keys: std::collections::BTreeSet<String> =
            built.as_object().expect("object").keys().cloned().collect();

        let missing: Vec<&String> = expected_keys.difference(&built_keys).collect();
        assert!(
            missing.is_empty(),
            "build_status_value omits {missing:?}, which permanently disables the \
             no-op patch guard for those fields"
        );
    }

    /// The mirrored provider timestamp must also be stable, for the same reason
    /// condition timestamps are: it is part of the compared status.
    #[test]
    fn test_spot_schedule_transition_time_is_held_when_active_is_unchanged() {
        use crate::crd::{ScheduledCapacityStatus, SpotScheduleStatus};
        let mut capacity = capacity_with(base_spec());
        capacity.status = Some(ScheduledCapacityStatus {
            spot_schedule: Some(SpotScheduleStatus {
                active: Some(true),
                last_transition_time: Some("2026-01-01T00:00:00+00:00".to_string()),
                ..SpotScheduleStatus::default()
            }),
            ..ScheduledCapacityStatus::default()
        });
        let held = spot_schedule_transition_time(
            &capacity,
            &SpotScheduleVerdict::Active {
                provider_generation: None,
            },
            Utc::now(),
        );
        assert_eq!(held.as_deref(), Some("2026-01-01T00:00:00+00:00"));

        let bumped = spot_schedule_transition_time(
            &capacity,
            &SpotScheduleVerdict::Inactive {
                provider_generation: None,
            },
            Utc::now(),
        );
        assert_ne!(
            bumped.as_deref(),
            Some("2026-01-01T00:00:00+00:00"),
            "a real active flip must restamp"
        );
    }

    // ========================================================================
    // Metric transitions
    // ========================================================================

    /// The timeout counter must count ENTERING the timed-out phase, not every
    /// reconcile spent in it. Counting per reconcile made the live counter read
    /// 100 for a single timeout.
    #[test]
    fn test_timeout_metric_counts_only_the_transition_into_the_phase() {
        assert!(
            entered_phase(
                PHASE_CAPACITY_HANDBACK_TIMED_OUT,
                Some(PHASE_CAPACITY_HANDING_BACK)
            ),
            "HandingBack -> HandbackTimedOut is the transition to count"
        );
        assert!(
            !entered_phase(
                PHASE_CAPACITY_HANDBACK_TIMED_OUT,
                Some(PHASE_CAPACITY_HANDBACK_TIMED_OUT)
            ),
            "staying in the phase must not count again"
        );
        assert!(
            entered_phase(PHASE_CAPACITY_HANDBACK_TIMED_OUT, None),
            "a first reconcile that is already timed out counts once"
        );
    }

    /// The status must never claim a written value it did not write. This pins
    /// the carry-forward: when a decision writes nothing, the previously
    /// written value is preserved rather than dropped or invented.
    #[test]
    fn test_status_written_value_carries_forward_when_nothing_is_written() {
        let mut capacity = capacity_with(base_spec());
        capacity.status = Some(ScheduledCapacityStatus {
            written_value: Some(10),
            ..ScheduledCapacityStatus::default()
        });
        let decision = decision_in(PHASE_CAPACITY_ACTIVE);
        assert_eq!(decision.write, None);

        // Mirrors the reconciler's `written` computation.
        let written = match decision.write {
            Some(value) => Some(value),
            None => capacity.status.as_ref().and_then(|s| s.written_value),
        };
        assert_eq!(written, Some(10));
    }
}
