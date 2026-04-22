// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::{
        ALLOWED_BOOTSTRAP_API_GROUPS, ALLOWED_INFRASTRUCTURE_API_GROUPS, MAX_DURATION_SECS,
    };
    use std::collections::BTreeMap;

    // ========================================================================
    // parse_duration — overflow & bounds protection
    // ========================================================================

    #[test]
    fn test_parse_duration_seconds() {
        let d = parse_duration("30s").unwrap();
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn test_parse_duration_minutes() {
        let d = parse_duration("5m").unwrap();
        assert_eq!(d.as_secs(), 300);
    }

    #[test]
    fn test_parse_duration_hours() {
        let d = parse_duration("1h").unwrap();
        assert_eq!(d.as_secs(), 3600);
    }

    #[test]
    fn test_parse_duration_max_allowed() {
        // 24h == MAX_DURATION_SECS exactly — should be accepted
        let d = parse_duration("24h").unwrap();
        assert_eq!(d.as_secs(), MAX_DURATION_SECS);
    }

    #[test]
    fn test_parse_duration_exceeds_max_hours() {
        let err = parse_duration("25h").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exceeds maximum"),
            "expected 'exceeds maximum' in error, got: {msg}"
        );
    }

    #[test]
    fn test_parse_duration_exceeds_max_seconds() {
        // 86401s is one second over the 24h limit
        let err = parse_duration("86401s").unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_parse_duration_overflow_u64() {
        // This would overflow u64 on multiplication: 9999999999999999h * 3600
        let err = parse_duration("9999999999999999h").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("overflow") || msg.contains("exceeds maximum"),
            "expected overflow or bounds error, got: {msg}"
        );
    }

    #[test]
    fn test_parse_duration_overflow_minutes() {
        // u64::MAX / 60 + 1 should overflow
        let value = u64::MAX / 60 + 1;
        let input = format!("{value}m");
        let err = parse_duration(&input).unwrap_err();
        assert!(
            err.to_string().contains("overflow") || err.to_string().contains("exceeds maximum")
        );
    }

    #[test]
    fn test_parse_duration_empty() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("   ").is_err());
    }

    #[test]
    fn test_parse_duration_invalid_unit() {
        let err = parse_duration("5d").unwrap_err();
        assert!(err.to_string().contains("Invalid duration unit"));
    }

    #[test]
    fn test_parse_duration_invalid_value() {
        assert!(parse_duration("abch").is_err());
        assert!(parse_duration("s").is_err());
    }

    #[test]
    fn test_parse_duration_rejects_non_ascii() {
        // Regression: fuzz discovered that split_at(len - 1) panics when the
        // trailing byte is mid-UTF-8-code-point (e.g. "٠" = U+0660, bytes
        // D9 A0). Non-ASCII input must return Err, not panic.
        let err = parse_duration("٠").unwrap_err();
        assert!(err.to_string().contains("non-ASCII"));
        assert!(parse_duration("5٠").is_err());
        assert!(parse_duration("🕐").is_err());
    }

    // ========================================================================
    // validate_labels — reserved prefix rejection
    // ========================================================================

    #[test]
    fn test_validate_labels_clean() {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "my-app".to_string());
        labels.insert("team".to_string(), "platform".to_string());
        assert!(validate_labels(&labels, "labels").is_ok());
    }

    #[test]
    fn test_validate_labels_rejects_kubernetes_io() {
        let mut labels = BTreeMap::new();
        labels.insert("kubernetes.io/hostname".to_string(), "node1".to_string());
        let err = validate_labels(&labels, "labels").unwrap_err();
        assert!(err.to_string().contains("reserved prefix"));
        assert!(err.to_string().contains("kubernetes.io/"));
    }

    #[test]
    fn test_validate_labels_rejects_k8s_io() {
        let mut labels = BTreeMap::new();
        labels.insert("k8s.io/something".to_string(), "val".to_string());
        assert!(validate_labels(&labels, "labels").is_err());
    }

    #[test]
    fn test_validate_labels_rejects_cluster_x_k8s_io() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "cluster.x-k8s.io/cluster-name".to_string(),
            "injected-cluster".to_string(),
        );
        let err = validate_labels(&labels, "labels").unwrap_err();
        assert!(err.to_string().contains("reserved prefix"));
    }

    #[test]
    fn test_validate_labels_rejects_5spot_io() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "5spot.finos.org/scheduled-machine".to_string(),
            "injected".to_string(),
        );
        assert!(validate_labels(&labels, "labels").is_err());
    }

    #[test]
    fn test_validate_labels_empty_map() {
        let labels = BTreeMap::new();
        assert!(validate_labels(&labels, "labels").is_ok());
    }

    #[test]
    fn test_validate_annotations_rejects_reserved() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "kubernetes.io/created-by".to_string(),
            "attacker".to_string(),
        );
        let err = validate_labels(&annotations, "annotations").unwrap_err();
        assert!(err.to_string().contains("annotations"));
        assert!(err.to_string().contains("reserved prefix"));
    }

    // ========================================================================
    // validate_api_group — allowlist enforcement
    // ========================================================================

    #[test]
    fn test_validate_api_group_valid_bootstrap() {
        assert!(validate_api_group(
            "bootstrap.cluster.x-k8s.io/v1beta1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_api_group_valid_k0smotron_bootstrap() {
        assert!(validate_api_group(
            "k0smotron.io/v1beta1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_api_group_valid_infrastructure() {
        assert!(validate_api_group(
            "infrastructure.cluster.x-k8s.io/v1beta1",
            ALLOWED_INFRASTRUCTURE_API_GROUPS,
            "infrastructure"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_api_group_rejects_core_api() {
        // Core API (no slash) must be rejected
        let err = validate_api_group("v1", ALLOWED_BOOTSTRAP_API_GROUPS, "bootstrap").unwrap_err();
        assert!(err.to_string().contains("namespaced API group"));
    }

    #[test]
    fn test_validate_api_group_rejects_rbac() {
        let err = validate_api_group(
            "rbac.authorization.k8s.io/v1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
        assert!(err.to_string().contains("rbac.authorization.k8s.io"));
    }

    #[test]
    fn test_validate_api_group_rejects_apps() {
        let err = validate_api_group(
            "apps/v1",
            ALLOWED_INFRASTRUCTURE_API_GROUPS,
            "infrastructure",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn test_validate_api_group_rejects_wrong_side() {
        // Infrastructure group used as bootstrap should be rejected
        let err = validate_api_group(
            "infrastructure.cluster.x-k8s.io/v1beta1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn test_validate_api_group_rejects_kube_system_trick() {
        // Attempt to sneak in a system API group
        let err = validate_api_group(
            "admissionregistration.k8s.io/v1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    // ========================================================================
    // build_phase_transition_event — pure function, no API calls
    // ========================================================================

    #[test]
    fn test_phase_event_normal_type_for_active_transition() {
        use kube::runtime::events::EventType;
        let event = build_phase_transition_event(
            Some("Pending"),
            "Active",
            "MachineCreated",
            "CAPI Machine created",
        );
        assert_eq!(event.type_, EventType::Normal);
    }

    #[test]
    fn test_phase_event_warning_type_for_error_transition() {
        use kube::runtime::events::EventType;
        let event = build_phase_transition_event(
            Some("Pending"),
            "Error",
            "MachineCreationFailed",
            "CAPI API unreachable",
        );
        assert_eq!(event.type_, EventType::Warning);
    }

    #[test]
    fn test_phase_event_warning_type_for_terminated_transition() {
        use kube::runtime::events::EventType;
        let event = build_phase_transition_event(
            Some("Active"),
            "Terminated",
            "KillSwitch",
            "Kill switch activated",
        );
        assert_eq!(event.type_, EventType::Warning);
    }

    #[test]
    fn test_phase_event_note_contains_from_and_to_phase() {
        let event = build_phase_transition_event(
            Some("Inactive"),
            "Pending",
            "ScheduleActive",
            "Schedule became active",
        );
        let note = event.note.expect("note should be set");
        assert!(note.contains("Inactive"), "note should contain from-phase");
        assert!(note.contains("Pending"), "note should contain to-phase");
    }

    #[test]
    fn test_phase_event_unknown_from_phase_when_none() {
        let event =
            build_phase_transition_event(None, "Inactive", "ScheduleInactive", "Outside schedule");
        let note = event.note.expect("note should be set");
        assert!(
            note.contains("Unknown"),
            "note should show 'Unknown' for missing from-phase"
        );
    }

    #[test]
    fn test_phase_event_action_contains_to_phase() {
        let event = build_phase_transition_event(
            Some("Pending"),
            "Active",
            "MachineCreated",
            "Machine ready",
        );
        assert!(
            event.action.contains("Active"),
            "action should reference the target phase"
        );
    }

    #[test]
    fn test_phase_event_reason_matches_input() {
        let event = build_phase_transition_event(
            Some("Active"),
            "ShuttingDown",
            "GracePeriod",
            "Outside schedule window",
        );
        assert_eq!(event.reason, "GracePeriod");
    }

    // Additional coverage: every non-error phase → Normal
    #[test]
    fn test_phase_event_normal_for_all_non_error_phases() {
        use kube::runtime::events::EventType;
        for phase in &["Pending", "Active", "ShuttingDown", "Inactive", "Disabled"] {
            let event = build_phase_transition_event(None, phase, "Reason", "msg");
            assert_eq!(
                event.type_,
                EventType::Normal,
                "phase '{phase}' should produce Normal event"
            );
        }
    }

    #[test]
    fn test_phase_event_note_contains_message() {
        let event = build_phase_transition_event(
            Some("Pending"),
            "Active",
            "MachineCreated",
            "CAPI resources provisioned",
        );
        let note = event.note.expect("note should be set");
        assert!(
            note.contains("CAPI resources provisioned"),
            "note should include the message"
        );
    }

    #[test]
    fn test_phase_event_secondary_is_none() {
        // secondary object reference is not used for phase transitions
        let event =
            build_phase_transition_event(Some("Inactive"), "Active", "ScheduleActive", "msg");
        assert!(event.secondary.is_none());
    }

    // ========================================================================
    // update_phase — mock API tests
    // ========================================================================

    use http::{Request, Response};
    use kube::client::Body;
    use std::pin::pin;
    use tower_test::mock;

    fn mock_client_pair() -> (kube::Client, mock::Handle<Request<Body>, Response<Body>>) {
        let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
        (kube::Client::new(svc, "default"), handle)
    }

    fn make_test_context(client: kube::Client) -> crate::reconcilers::Context {
        crate::reconcilers::Context::new(client, 0, 1)
    }

    /// Minimal `ScheduledMachine` JSON for `patch_status` responses.
    fn sm_response_body(name: &str, namespace: &str, phase: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "5spot.finos.org/v1alpha1",
            "kind": "ScheduledMachine",
            "metadata": {
                "name": name,
                "namespace": namespace,
                "resourceVersion": "2"
            },
            "spec": {
                "clusterName": "test",
                "bootstrapSpec": {
                    "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                    "kind": "K0sWorkerConfig",
                    "spec": {}
                },
                "infrastructureSpec": {
                    "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                    "kind": "RemoteMachine",
                    "spec": {}
                },
                "schedule": {
                    "daysOfWeek": ["mon-fri"],
                    "hoursOfDay": ["9-17"],
                    "timezone": "UTC",
                    "enabled": true
                },
                "gracefulShutdownTimeout": "5m",
                "nodeDrainTimeout": "5m"
            },
            "status": { "phase": phase }
        }))
        .unwrap()
    }

    /// Minimal events.k8s.io/v1 Event JSON for recorder responses.
    fn k8s_event_response_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "events.k8s.io/v1",
            "kind": "Event",
            "metadata": {
                "name": "5spot-test.17a0b1c",
                "namespace": "default",
                "resourceVersion": "1"
            },
            "eventTime": "2026-04-08T00:00:00.000000Z",
            "reportingController": "5spot-controller",
            "reportingInstance": "5spot-0",
            "action": "PhaseTransitionToActive",
            "reason": "MachineCreated",
            "type": "Normal",
            "regarding": {
                "apiVersion": "5spot.finos.org/v1alpha1",
                "kind": "ScheduledMachine",
                "name": "test-sm",
                "namespace": "default"
            }
        }))
        .unwrap()
    }

    fn k8s_error_body(code: u16, msg: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "kind": "Status",
            "apiVersion": "v1",
            "status": "Failure",
            "message": msg,
            "code": code
        }))
        .unwrap()
    }

    // ---- Positive: successful full path ----

    #[tokio::test]
    async fn test_update_phase_success_patches_status() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // 1. Event publication (events.k8s.io)
            let (_req, send) = h.next_request().await.expect("expected events call");
            send.send_response(
                Response::builder()
                    .status(201)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_event_response_body()))
                    .unwrap(),
            );
            // 2. Status patch
            let (req, send) = h.next_request().await.expect("expected patch_status call");
            assert_eq!(req.method(), http::Method::PATCH);
            assert!(
                req.uri().path().ends_with("/status"),
                "should target /status subresource, got: {}",
                req.uri().path()
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body("test-sm", "default", "Active")))
                    .unwrap(),
            );
        });

        update_phase(
            &ctx,
            "default",
            "test-sm",
            Some("Pending"),
            "Active",
            Some("MachineCreated"),
            Some("CAPI Machine created"),
            true,
        )
        .await
        .expect("update_phase should return Ok on success");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_update_phase_uses_default_reason_when_none() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(201)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_event_response_body()))
                    .unwrap(),
            );
            let (_req, send) = h.next_request().await.expect("patch_status call");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body(
                        "test-sm", "default", "Inactive",
                    )))
                    .unwrap(),
            );
        });

        // Passing None for reason and message — should use defaults without panicking
        update_phase(
            &ctx, "default", "test-sm", None, "Inactive", None, None, false,
        )
        .await
        .expect("should succeed with default reason/message");

        srv.await.unwrap();
    }

    // ---- Negative: Kubernetes API returns an error on patch_status ----

    #[tokio::test]
    async fn test_update_phase_returns_kube_error_when_patch_fails() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // Event call succeeds
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(201)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_event_response_body()))
                    .unwrap(),
            );
            // Status patch returns 500
            let (_req, send) = h.next_request().await.expect("patch_status call");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "internal server error")))
                    .unwrap(),
            );
        });

        let result = update_phase(
            &ctx,
            "default",
            "test-sm",
            Some("Active"),
            "Error",
            None,
            None,
            false,
        )
        .await;
        assert!(result.is_err(), "should return Err when patch_status fails");
        assert!(
            matches!(result.unwrap_err(), ReconcilerError::KubeError(_)),
            "error variant should be KubeError"
        );

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_update_phase_returns_kube_error_on_404_not_found() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(201)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_event_response_body()))
                    .unwrap(),
            );
            let (_req, send) = h.next_request().await.expect("patch_status call");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        404,
                        "scheduledmachines \"test-sm\" not found",
                    )))
                    .unwrap(),
            );
        });

        let result = update_phase(
            &ctx,
            "default",
            "test-sm",
            Some("Pending"),
            "Active",
            None,
            None,
            true,
        )
        .await;
        assert!(result.is_err(), "should return Err on 404");
        assert!(matches!(result.unwrap_err(), ReconcilerError::KubeError(_)));

        srv.await.unwrap();
    }

    // ---- Exception: event recording fails — best-effort, must not block status patch ----

    #[tokio::test]
    async fn test_update_phase_event_failure_is_best_effort_patch_still_succeeds() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // Event call fails with 500
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "events API unavailable")))
                    .unwrap(),
            );
            // Status patch MUST still be called and returns success
            let (req, send) = h
                .next_request()
                .await
                .expect("patch_status must be called even after event failure");
            assert_eq!(req.method(), http::Method::PATCH);
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body("test-sm", "default", "Active")))
                    .unwrap(),
            );
        });

        let result = update_phase(
            &ctx,
            "default",
            "test-sm",
            Some("Pending"),
            "Active",
            None,
            None,
            true,
        )
        .await;
        assert!(
            result.is_ok(),
            "event failure must not abort phase transition, got: {result:?}",
        );

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_update_phase_event_failure_plus_patch_failure_returns_kube_error() {
        // Both event AND patch fail — should still return the patch error, not the event error
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // Event fails
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(503)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(503, "service unavailable")))
                    .unwrap(),
            );
            // Patch also fails
            let (_req, send) = h.next_request().await.expect("patch_status call");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "internal error")))
                    .unwrap(),
            );
        });

        let result = update_phase(
            &ctx,
            "default",
            "test-sm",
            Some("Active"),
            "Error",
            None,
            None,
            false,
        )
        .await;
        assert!(
            result.is_err(),
            "should propagate patch error when both calls fail"
        );
        assert!(matches!(result.unwrap_err(), ReconcilerError::KubeError(_)));

        srv.await.unwrap();
    }

    // ---- update_phase_with_grace_period ----

    #[tokio::test]
    async fn test_update_phase_with_grace_period_success() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(201)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_event_response_body()))
                    .unwrap(),
            );
            let (req, send) = h.next_request().await.expect("patch_status call");
            assert!(req.uri().path().ends_with("/status"));
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body(
                        "test-sm",
                        "default",
                        "ShuttingDown",
                    )))
                    .unwrap(),
            );
        });

        update_phase_with_grace_period(
            &ctx,
            "default",
            "test-sm",
            Some("Active"),
            "ShuttingDown",
            None,
            None,
            false,
        )
        .await
        .expect("grace period update should succeed");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_update_phase_with_grace_period_patch_failure() {
        let (client, handle) = mock_client_pair();
        let ctx = make_test_context(client);

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("events call");
            send.send_response(
                Response::builder()
                    .status(201)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_event_response_body()))
                    .unwrap(),
            );
            let (_req, send) = h.next_request().await.expect("patch_status call");
            send.send_response(
                Response::builder()
                    .status(409)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        409,
                        "conflict: resource version mismatch",
                    )))
                    .unwrap(),
            );
        });

        let result = update_phase_with_grace_period(
            &ctx,
            "default",
            "test-sm",
            Some("Active"),
            "ShuttingDown",
            None,
            None,
            false,
        )
        .await;
        assert!(result.is_err(), "409 conflict should return error");
        assert!(matches!(result.unwrap_err(), ReconcilerError::KubeError(_)));

        srv.await.unwrap();
    }

    // ---- Context::new — unit tests ----

    #[tokio::test]
    async fn test_context_stores_instance_fields() {
        // Can't fully inspect Recorder internals, but verify Context fields are set correctly.
        // Requires async context because tower's buffer needs a Tokio runtime.
        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(svc, "default");
        let ctx = crate::reconcilers::Context::new(client, 2, 5);
        assert_eq!(ctx.instance_id, 2);
        assert_eq!(ctx.instance_count, 5);
    }

    #[test]
    fn test_controller_name_constant_is_correct() {
        use crate::reconcilers::scheduled_machine::CONTROLLER_NAME;
        assert_eq!(CONTROLLER_NAME, "5spot-controller");
    }

    // ========================================================================
    // evict_pod — P2-6 mock API tests (TDD)
    // ========================================================================

    /// Minimal Pod JSON body — used as the success response for a DELETE pod call.
    fn pod_response_body(name: &str, namespace: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": name,
                "namespace": namespace,
                "resourceVersion": "1"
            },
            "spec": { "containers": [] },
            "status": {}
        }))
        .unwrap()
    }

    // ---- Positive: successful eviction (200) ----

    #[tokio::test]
    async fn test_evict_pod_success_returns_ok() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected DELETE pod call");
            assert_eq!(req.method(), http::Method::DELETE);
            assert!(
                req.uri().path().contains("/pods/test-pod"),
                "should target the pod, got: {}",
                req.uri().path()
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(pod_response_body("test-pod", "test-ns")))
                    .unwrap(),
            );
        });

        super::super::evict_pod(&client, "test-pod", "test-ns", "test-node")
            .await
            .expect("successful eviction should return Ok");

        srv.await.unwrap();
    }

    // ---- Positive: 404 means pod already gone — treat as success ----

    #[tokio::test]
    async fn test_evict_pod_404_already_deleted_returns_ok() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected DELETE pod call");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        404,
                        "pods \"test-pod\" not found",
                    )))
                    .unwrap(),
            );
        });

        super::super::evict_pod(&client, "test-pod", "test-ns", "test-node")
            .await
            .expect("404 (already deleted) should return Ok");

        srv.await.unwrap();
    }

    // ---- Negative: 429 PDB-blocked eviction MUST propagate as CapiError ----

    #[tokio::test]
    async fn test_evict_pod_429_pdb_blocked_returns_capi_error() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected DELETE pod call");
            send.send_response(
                Response::builder()
                    .status(429)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        429,
                        "cannot evict pod as it would violate the pod's disruption budget",
                    )))
                    .unwrap(),
            );
        });

        let result = super::super::evict_pod(&client, "test-pod", "test-ns", "test-node").await;
        assert!(
            result.is_err(),
            "429 PDB-blocked eviction must return Err, not Ok"
        );
        assert!(
            matches!(result.unwrap_err(), ReconcilerError::CapiError(_)),
            "error variant should be CapiError for 429"
        );

        srv.await.unwrap();
    }

    // ---- Negative: unexpected 500 server error propagates as CapiError ----

    #[tokio::test]
    async fn test_evict_pod_500_server_error_returns_capi_error() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected DELETE pod call");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "internal server error")))
                    .unwrap(),
            );
        });

        let result = super::super::evict_pod(&client, "test-pod", "test-ns", "test-node").await;
        assert!(result.is_err(), "500 error should return Err");
        assert!(
            matches!(result.unwrap_err(), ReconcilerError::CapiError(_)),
            "error variant should be CapiError for unexpected API error"
        );

        srv.await.unwrap();
    }

    // ---- Exception: 403 Forbidden also propagates as CapiError ----

    #[tokio::test]
    async fn test_evict_pod_403_forbidden_returns_capi_error() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected DELETE pod call");
            send.send_response(
                Response::builder()
                    .status(403)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        403,
                        "forbidden: User cannot delete pods",
                    )))
                    .unwrap(),
            );
        });

        let result = super::super::evict_pod(&client, "test-pod", "test-ns", "test-node").await;
        assert!(result.is_err(), "403 forbidden should return Err");
        assert!(
            matches!(result.unwrap_err(), ReconcilerError::CapiError(_)),
            "error variant should be CapiError for 403"
        );

        srv.await.unwrap();
    }

    // ========================================================================
    // compute_backoff_secs — exponential backoff with cap
    // ========================================================================

    #[test]
    fn test_backoff_secs_retry_0_returns_base() {
        // First error: should return the base ERROR_REQUEUE_SECS (30s)
        let secs = super::super::compute_backoff_secs(0);
        assert_eq!(secs, 30, "retry 0 should return base 30s");
    }

    #[test]
    fn test_backoff_secs_doubles_each_retry() {
        assert_eq!(super::super::compute_backoff_secs(1), 60);
        assert_eq!(super::super::compute_backoff_secs(2), 120);
        assert_eq!(super::super::compute_backoff_secs(3), 240);
    }

    #[test]
    fn test_backoff_secs_caps_at_max_backoff() {
        // Retry 4: 30 * 2^4 = 480 > 300 → should be capped at MAX_BACKOFF_SECS (300)
        let secs = super::super::compute_backoff_secs(4);
        assert_eq!(secs, 300, "retry 4 should be capped at 300s");
    }

    #[test]
    fn test_backoff_secs_at_max_retries_returns_max() {
        use crate::constants::MAX_RECONCILE_RETRIES;
        let secs = super::super::compute_backoff_secs(MAX_RECONCILE_RETRIES);
        assert_eq!(
            secs, 300,
            "at MAX_RECONCILE_RETRIES should return max backoff"
        );
    }

    #[test]
    fn test_backoff_secs_well_beyond_max_retries_returns_max() {
        let secs = super::super::compute_backoff_secs(100);
        assert_eq!(
            secs, 300,
            "large retry count should always return max backoff"
        );
    }

    // ========================================================================
    // extract_machine_refs — pulls providerID + full NodeRef from a CAPI Machine
    // ========================================================================

    fn machine_dyn(data: serde_json::Value) -> kube::core::DynamicObject {
        kube::core::DynamicObject {
            types: None,
            metadata: kube::core::ObjectMeta::default(),
            data,
        }
    }

    #[test]
    fn test_extract_machine_refs_fully_populated() {
        let m = machine_dyn(serde_json::json!({
            "spec": { "providerID": "libvirt:///uuid-abc-123" },
            "status": {
                "nodeRef": {
                    "apiVersion": "v1",
                    "kind": "Node",
                    "name": "worker-01",
                    "uid": "11111111-2222-3333-4444-555555555555"
                }
            }
        }));
        let (provider_id, node_ref) = extract_machine_refs(&m);
        assert_eq!(provider_id.as_deref(), Some("libvirt:///uuid-abc-123"));
        let nref = node_ref.expect("nodeRef must be populated");
        assert_eq!(nref.api_version, "v1");
        assert_eq!(nref.kind, "Node");
        assert_eq!(nref.name, "worker-01");
        assert_eq!(
            nref.uid.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
    }

    #[test]
    fn test_extract_machine_refs_empty_machine_returns_none_none() {
        let m = machine_dyn(serde_json::json!({}));
        let (p, n) = extract_machine_refs(&m);
        assert!(p.is_none());
        assert!(n.is_none());
    }

    #[test]
    fn test_extract_machine_refs_only_provider_id() {
        let m = machine_dyn(serde_json::json!({
            "spec": { "providerID": "aws:///us-east-1a/i-0abcd1234" }
        }));
        let (p, n) = extract_machine_refs(&m);
        assert_eq!(p.as_deref(), Some("aws:///us-east-1a/i-0abcd1234"));
        assert!(n.is_none(), "no status.nodeRef means no NodeRef");
    }

    #[test]
    fn test_extract_machine_refs_node_ref_without_uid() {
        let m = machine_dyn(serde_json::json!({
            "status": {
                "nodeRef": {
                    "apiVersion": "v1",
                    "kind": "Node",
                    "name": "worker-02"
                }
            }
        }));
        let (p, n) = extract_machine_refs(&m);
        assert!(p.is_none());
        let nref = n.expect("nodeRef must be populated");
        assert_eq!(nref.name, "worker-02");
        assert!(nref.uid.is_none(), "uid is optional");
    }

    #[test]
    fn test_extract_machine_refs_incomplete_node_ref_returns_none() {
        // Old CAPI versions or in-flight Machines may have a partial nodeRef.
        // We treat anything missing apiVersion/kind/name as "not ready yet" so
        // the status is only populated once CAPI has fully resolved the Node.
        let m = machine_dyn(serde_json::json!({
            "status": { "nodeRef": { "name": "legacy" } }
        }));
        let (_, n) = extract_machine_refs(&m);
        assert!(
            n.is_none(),
            "nodeRef missing apiVersion/kind must be treated as None"
        );
    }

    #[test]
    fn test_extract_machine_refs_provider_id_wrong_type_ignored() {
        // Defensive: if providerID is somehow not a string, we do NOT want to
        // panic or return garbage — treat as absent.
        let m = machine_dyn(serde_json::json!({
            "spec": { "providerID": 42 }
        }));
        let (p, _) = extract_machine_refs(&m);
        assert!(p.is_none());
    }

    // ========================================================================
    // fetch_capi_machine — mock-Client tests (positive / negative / exception)
    // ========================================================================

    fn capi_machine_body(
        name: &str,
        namespace: &str,
        provider_id: Option<&str>,
        node_ref_name: Option<&str>,
    ) -> Vec<u8> {
        let mut obj = serde_json::json!({
            "apiVersion": "cluster.x-k8s.io/v1beta1",
            "kind": "Machine",
            "metadata": { "name": name, "namespace": namespace, "resourceVersion": "1" },
            "spec": {},
            "status": {}
        });
        if let Some(pid) = provider_id {
            obj["spec"]["providerID"] = serde_json::json!(pid);
        }
        if let Some(nname) = node_ref_name {
            obj["status"]["nodeRef"] = serde_json::json!({
                "apiVersion": "v1",
                "kind": "Node",
                "name": nname,
                "uid": "uid-1"
            });
        }
        serde_json::to_vec(&obj).unwrap()
    }

    #[tokio::test]
    async fn test_fetch_capi_machine_success_returns_some() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected GET Machine");
            assert_eq!(req.method(), http::Method::GET);
            assert!(
                req.uri()
                    .path()
                    .contains("/apis/cluster.x-k8s.io/v1beta1/namespaces/default/machines/sm-m"),
                "should target CAPI Machine path, got: {}",
                req.uri().path()
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(capi_machine_body(
                        "sm-m",
                        "default",
                        Some("libvirt:///abc"),
                        Some("node-1"),
                    )))
                    .unwrap(),
            );
        });

        let machine = fetch_capi_machine(&client, "default", "sm-m")
            .await
            .expect("200 response must succeed");
        let machine = machine.expect("200 must yield Some");
        let (pid, nref) = extract_machine_refs(&machine);
        assert_eq!(pid.as_deref(), Some("libvirt:///abc"));
        assert_eq!(nref.expect("nodeRef").name, "node-1");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_fetch_capi_machine_404_returns_ok_none() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        404,
                        "machines \"missing\" not found",
                    )))
                    .unwrap(),
            );
        });

        let result = fetch_capi_machine(&client, "default", "missing")
            .await
            .expect("404 must map to Ok(None), not Err");
        assert!(
            result.is_none(),
            "404 must yield None (Machine not yet created)"
        );

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_fetch_capi_machine_500_returns_capi_error() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "internal server error")))
                    .unwrap(),
            );
        });

        let err = fetch_capi_machine(&client, "default", "broken")
            .await
            .expect_err("500 must propagate as Err");
        assert!(
            matches!(err, ReconcilerError::CapiError(_)),
            "non-404 must map to CapiError, got: {err:?}"
        );

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_fetch_capi_machine_403_forbidden_returns_capi_error() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(403)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        403,
                        "forbidden: watch Machines in namespace default",
                    )))
                    .unwrap(),
            );
        });

        let err = fetch_capi_machine(&client, "default", "anything")
            .await
            .expect_err("403 must propagate as Err");
        assert!(
            matches!(err, ReconcilerError::CapiError(_)),
            "403 must map to CapiError, got: {err:?}"
        );

        srv.await.unwrap();
    }

    // ========================================================================
    // patch_machine_refs_status — mock-Client tests + body-shape assertions
    // ========================================================================

    async fn collect_json_body(body: Body) -> serde_json::Value {
        use http_body_util::BodyExt;
        let bytes = body.collect().await.expect("body collect").to_bytes();
        serde_json::from_slice(&bytes).expect("body must be JSON")
    }

    fn sample_node_ref() -> NodeRef {
        NodeRef {
            api_version: "v1".to_string(),
            kind: "Node".to_string(),
            name: "node-1".to_string(),
            uid: Some("uid-abc".to_string()),
        }
    }

    #[tokio::test]
    async fn test_patch_machine_refs_status_both_fields_success() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected PATCH");
            assert_eq!(req.method(), http::Method::PATCH);
            assert!(
                req.uri()
                    .path()
                    .ends_with("/scheduledmachines/test-sm/status"),
                "must target /status subresource, got: {}",
                req.uri().path()
            );
            let body = collect_json_body(req.into_body()).await;
            assert_eq!(body["status"]["providerID"], "libvirt:///abc");
            assert_eq!(body["status"]["nodeRef"]["name"], "node-1");
            assert_eq!(body["status"]["nodeRef"]["apiVersion"], "v1");
            assert_eq!(body["status"]["nodeRef"]["kind"], "Node");
            assert_eq!(body["status"]["nodeRef"]["uid"], "uid-abc");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body("test-sm", "default", "Active")))
                    .unwrap(),
            );
        });

        let nref = sample_node_ref();
        patch_machine_refs_status(
            &client,
            "default",
            "test-sm",
            Some("libvirt:///abc"),
            Some(&nref),
        )
        .await
        .expect("both-field patch must succeed");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_machine_refs_status_only_provider_id_omits_node_ref() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("PATCH");
            let body = collect_json_body(req.into_body()).await;
            assert_eq!(body["status"]["providerID"], "aws:///i-0");
            assert!(
                body["status"].get("nodeRef").is_none(),
                "nodeRef MUST be omitted (not null) when None — merge patch must not clear existing value"
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body("test-sm", "default", "Active")))
                    .unwrap(),
            );
        });

        patch_machine_refs_status(&client, "default", "test-sm", Some("aws:///i-0"), None)
            .await
            .expect("provider-id-only patch must succeed");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_machine_refs_status_only_node_ref_omits_provider_id() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("PATCH");
            let body = collect_json_body(req.into_body()).await;
            assert!(
                body["status"].get("providerID").is_none(),
                "providerID MUST be omitted when None"
            );
            assert_eq!(body["status"]["nodeRef"]["name"], "node-1");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(sm_response_body("test-sm", "default", "Active")))
                    .unwrap(),
            );
        });

        let nref = sample_node_ref();
        patch_machine_refs_status(&client, "default", "test-sm", None, Some(&nref))
            .await
            .expect("node-ref-only patch must succeed");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_machine_refs_status_both_none_is_noop_no_http_call() {
        // When both args are None there MUST be zero network traffic —
        // calling with nothing to write should not issue a patch.
        let (client, handle) = mock_client_pair();
        let watcher = tokio::spawn(async move {
            let mut h = pin!(handle);
            // Race with a tiny timeout — if a request is made, fail.
            tokio::select! {
                req = h.next_request() => {
                    panic!("no HTTP call expected when both fields are None, got: {:?}",
                        req.map(|(r, _)| r.uri().to_string()));
                }
                () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                    // expected: no request
                }
            }
        });

        patch_machine_refs_status(&client, "default", "test-sm", None, None)
            .await
            .expect("no-op patch must return Ok");

        watcher.await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_machine_refs_status_500_returns_kube_error() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("PATCH");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "internal server error")))
                    .unwrap(),
            );
        });

        let nref = sample_node_ref();
        let err = patch_machine_refs_status(
            &client,
            "default",
            "test-sm",
            Some("libvirt:///x"),
            Some(&nref),
        )
        .await
        .expect_err("500 must propagate as Err");
        assert!(
            matches!(err, ReconcilerError::KubeError(_)),
            "patch_status errors flow through kube::Error -> KubeError, got: {err:?}"
        );

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_machine_refs_status_404_returns_kube_error() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("PATCH");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        404,
                        "scheduledmachines \"gone\" not found",
                    )))
                    .unwrap(),
            );
        });

        let err = patch_machine_refs_status(&client, "default", "gone", Some("libvirt:///x"), None)
            .await
            .expect_err("404 must propagate as Err (no silent success)");
        assert!(matches!(err, ReconcilerError::KubeError(_)));

        srv.await.unwrap();
    }

    // ========================================================================
    // get_node_from_machine — thin wrapper: verify delegation behaviour
    // ========================================================================

    #[tokio::test]
    async fn test_get_node_from_machine_success_returns_node_name() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(capi_machine_body(
                        "sm-m",
                        "default",
                        None,
                        Some("worker-99"),
                    )))
                    .unwrap(),
            );
        });

        let node = get_node_from_machine(&client, "default", "sm-m")
            .await
            .expect("fetch must succeed");
        assert_eq!(node.as_deref(), Some("worker-99"));

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_get_node_from_machine_machine_404_returns_ok_none() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(404, "not found")))
                    .unwrap(),
            );
        });

        let node = get_node_from_machine(&client, "default", "sm-m")
            .await
            .expect("404 on machine must yield Ok(None)");
        assert!(node.is_none());

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_get_node_from_machine_no_node_ref_returns_none() {
        // Machine exists but status.nodeRef isn't populated yet — CAPI is still reconciling.
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(capi_machine_body("sm-m", "default", None, None)))
                    .unwrap(),
            );
        });

        let node = get_node_from_machine(&client, "default", "sm-m")
            .await
            .expect("must succeed");
        assert!(node.is_none(), "no nodeRef → None (wait for CAPI)");

        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_get_node_from_machine_500_returns_capi_error() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("GET Machine");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "boom")))
                    .unwrap(),
            );
        });

        let err = get_node_from_machine(&client, "default", "sm-m")
            .await
            .expect_err("500 must propagate");
        assert!(matches!(err, ReconcilerError::CapiError(_)));

        srv.await.unwrap();
    }

    // ========================================================================
    // machine_to_scheduled_machine — label-based reverse mapper (Phase 3)
    //   A CAPI Machine event → 0 or 1 ObjectRef<ScheduledMachine> lookups
    //   via `5spot.eribourg.dev/scheduled-machine` label.
    // ========================================================================

    fn machine_with_labels(
        labels: &[(&str, &str)],
        namespace: Option<&str>,
    ) -> kube::core::DynamicObject {
        let mut meta = kube::core::ObjectMeta::default();
        if !labels.is_empty() {
            let mut m = std::collections::BTreeMap::new();
            for (k, v) in labels {
                m.insert((*k).to_string(), (*v).to_string());
            }
            meta.labels = Some(m);
        }
        meta.namespace = namespace.map(std::string::ToString::to_string);
        kube::core::DynamicObject {
            types: None,
            metadata: meta,
            data: serde_json::json!({}),
        }
    }

    #[test]
    fn test_machine_to_sm_label_present_returns_one_ref() {
        let m = machine_with_labels(
            &[(crate::labels::LABEL_SCHEDULED_MACHINE, "my-sm")],
            Some("team-ns"),
        );
        let refs = machine_to_scheduled_machine(&m);
        assert_eq!(refs.len(), 1, "exactly one ObjectRef expected");
        let r = &refs[0];
        assert_eq!(r.name, "my-sm");
        assert_eq!(r.namespace.as_deref(), Some("team-ns"));
    }

    #[test]
    fn test_machine_to_sm_missing_label_returns_empty() {
        let m = machine_with_labels(
            &[("app.kubernetes.io/name", "something-else")],
            Some("default"),
        );
        assert!(machine_to_scheduled_machine(&m).is_empty());
    }

    #[test]
    fn test_machine_to_sm_no_labels_at_all_returns_empty() {
        let m = machine_with_labels(&[], Some("default"));
        assert!(machine_to_scheduled_machine(&m).is_empty());
    }

    #[test]
    fn test_machine_to_sm_empty_label_value_is_rejected() {
        // Defensive: an empty-string SM name is never valid and must NOT enqueue a reconcile.
        let m = machine_with_labels(
            &[(crate::labels::LABEL_SCHEDULED_MACHINE, "")],
            Some("default"),
        );
        assert!(
            machine_to_scheduled_machine(&m).is_empty(),
            "empty label value must produce no ObjectRef (would enqueue invalid reconcile)"
        );
    }

    #[test]
    fn test_machine_to_sm_whitespace_label_value_is_rejected() {
        // Defensive: whitespace-only SM names are also invalid.
        let m = machine_with_labels(
            &[(crate::labels::LABEL_SCHEDULED_MACHINE, "   ")],
            Some("default"),
        );
        assert!(machine_to_scheduled_machine(&m).is_empty());
    }

    #[test]
    fn test_machine_to_sm_missing_namespace_returns_empty() {
        // A namespaced CAPI Machine without a namespace is malformed — skip it rather than
        // enqueue a cluster-scoped lookup that would never resolve.
        let m = machine_with_labels(&[(crate::labels::LABEL_SCHEDULED_MACHINE, "my-sm")], None);
        assert!(
            machine_to_scheduled_machine(&m).is_empty(),
            "Machine without namespace must produce no ObjectRef"
        );
    }

    #[test]
    fn test_machine_to_sm_wrong_prefix_label_is_ignored() {
        // Similar key (different prefix) must not be confused with the real label.
        let m = machine_with_labels(
            &[("5spot.finos.org/scheduled-machine", "imposter")],
            Some("default"),
        );
        assert!(machine_to_scheduled_machine(&m).is_empty());
    }

    // ========================================================================
    // node_to_scheduled_machines — name-lookup mapper (Phase 4)
    //   A Node event → ObjectRef<ScheduledMachine> for each SM whose
    //   status.nodeRef.name matches node.metadata.name.
    // ========================================================================

    fn sm_with_node_ref(
        name: &str,
        namespace: &str,
        node_name: Option<&str>,
    ) -> crate::crd::ScheduledMachine {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        let mut sm = crate::crd::ScheduledMachine {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            spec: crate::crd::ScheduledMachineSpec {
                cluster_name: "test-cluster".to_string(),
                bootstrap_spec: crate::crd::EmbeddedResource(serde_json::json!({
                    "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                    "kind": "K0sWorkerConfig",
                    "spec": {}
                })),
                infrastructure_spec: crate::crd::EmbeddedResource(serde_json::json!({
                    "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                    "kind": "RemoteMachine",
                    "spec": {}
                })),
                machine_template: None,
                schedule: crate::crd::ScheduleSpec {
                    days_of_week: vec!["mon-fri".to_string()],
                    hours_of_day: vec!["9-17".to_string()],
                    timezone: "UTC".to_string(),
                    enabled: true,
                },
                priority: 50,
                graceful_shutdown_timeout: "5m".to_string(),
                node_drain_timeout: "5m".to_string(),
                kill_switch: false,
                node_taints: vec![],
                kill_if_commands: None,
            },
            status: None,
        };
        if let Some(nname) = node_name {
            sm.status = Some(crate::crd::ScheduledMachineStatus {
                phase: Some("Active".to_string()),
                node_ref: Some(NodeRef {
                    api_version: "v1".to_string(),
                    kind: "Node".to_string(),
                    name: nname.to_string(),
                    uid: None,
                }),
                ..Default::default()
            });
        }
        sm
    }

    fn node_named(name: &str) -> k8s_openapi::api::core::v1::Node {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        k8s_openapi::api::core::v1::Node {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_node_to_sms_single_match_returns_one_ref() {
        let node = node_named("worker-1");
        let sms = [
            sm_with_node_ref("sm-a", "default", Some("worker-1")),
            sm_with_node_ref("sm-b", "default", Some("worker-2")),
        ];
        let refs = node_to_scheduled_machines(&node, sms.iter());
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "sm-a");
        assert_eq!(refs[0].namespace.as_deref(), Some("default"));
    }

    #[test]
    fn test_node_to_sms_multiple_matches_returns_all() {
        // Defensive: if two SMs claim the same Node (misconfiguration), reconcile BOTH
        // so the operator can surface the conflict via status.
        let node = node_named("worker-1");
        let sms = [
            sm_with_node_ref("sm-a", "ns-1", Some("worker-1")),
            sm_with_node_ref("sm-b", "ns-2", Some("worker-1")),
        ];
        let refs = node_to_scheduled_machines(&node, sms.iter());
        assert_eq!(refs.len(), 2, "both conflicting SMs must be enqueued");
        let names: std::collections::HashSet<_> = refs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains("sm-a"));
        assert!(names.contains("sm-b"));
    }

    #[test]
    fn test_node_to_sms_no_match_returns_empty() {
        let node = node_named("worker-999");
        let sms = [
            sm_with_node_ref("sm-a", "default", Some("worker-1")),
            sm_with_node_ref("sm-b", "default", Some("worker-2")),
        ];
        assert!(node_to_scheduled_machines(&node, sms.iter()).is_empty());
    }

    #[test]
    fn test_node_to_sms_empty_list_returns_empty() {
        let node = node_named("worker-1");
        let sms: Vec<crate::crd::ScheduledMachine> = Vec::new();
        assert!(node_to_scheduled_machines(&node, sms.iter()).is_empty());
    }

    #[test]
    fn test_node_to_sms_sm_without_status_is_skipped() {
        let node = node_named("worker-1");
        let sms = [sm_with_node_ref("sm-a", "default", None)];
        assert!(
            node_to_scheduled_machines(&node, sms.iter()).is_empty(),
            "SMs with no status.nodeRef must be skipped, not matched"
        );
    }

    #[test]
    fn test_node_to_sms_node_with_no_name_returns_empty() {
        // Defensive: Node without metadata.name should not match anything — especially
        // not SMs that also happen to have an empty/missing nodeRef.name.
        let node = k8s_openapi::api::core::v1::Node::default();
        let sms = [sm_with_node_ref("sm-a", "default", Some(""))];
        assert!(node_to_scheduled_machines(&node, sms.iter()).is_empty());
    }

    #[test]
    fn test_node_to_sms_empty_node_ref_name_is_not_matched() {
        // SM with an empty-string nodeRef.name must never match anything.
        let node = node_named("worker-1");
        let sms = [sm_with_node_ref("sm-a", "default", Some(""))];
        assert!(node_to_scheduled_machines(&node, sms.iter()).is_empty());
    }

    // ========================================================================
    // node_reclaim_request — parse agent-written annotations (roadmap Phase 3)
    //   The controller reads these from the Node on every reconcile; when
    //   present they trigger transition into the Emergency Remove path.
    // ========================================================================

    fn node_with_annotations(
        name: &str,
        annotations: &[(&str, &str)],
    ) -> k8s_openapi::api::core::v1::Node {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        let map: std::collections::BTreeMap<String, String> = annotations
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        k8s_openapi::api::core::v1::Node {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                annotations: Some(map),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_node_reclaim_request_none_when_no_annotations() {
        let node = node_named("worker-1");
        assert!(
            node_reclaim_request(&node).is_none(),
            "bare node must not look like a reclaim request"
        );
    }

    #[test]
    fn test_node_reclaim_request_none_when_requested_is_absent() {
        // Reason + timestamp present but the trigger annotation missing:
        // must NOT fire. Guards against stale annotation residue.
        let node = node_with_annotations(
            "worker-1",
            &[
                (
                    crate::constants::RECLAIM_REASON_ANNOTATION,
                    "process-match: java",
                ),
                (
                    crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION,
                    "2026-04-19T21:30:00Z",
                ),
            ],
        );
        assert!(node_reclaim_request(&node).is_none());
    }

    #[test]
    fn test_node_reclaim_request_none_when_requested_value_is_not_true() {
        // Any value other than the literal "true" is treated as not-requested.
        let node = node_with_annotations(
            "worker-1",
            &[(crate::constants::RECLAIM_REQUESTED_ANNOTATION, "false")],
        );
        assert!(node_reclaim_request(&node).is_none());
    }

    #[test]
    fn test_node_reclaim_request_parses_all_three_fields() {
        let node = node_with_annotations(
            "worker-1",
            &[
                (crate::constants::RECLAIM_REQUESTED_ANNOTATION, "true"),
                (
                    crate::constants::RECLAIM_REASON_ANNOTATION,
                    "process-match: java",
                ),
                (
                    crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION,
                    "2026-04-19T21:30:00Z",
                ),
            ],
        );
        let req = node_reclaim_request(&node).expect("should parse");
        assert_eq!(req.reason.as_deref(), Some("process-match: java"));
        assert_eq!(req.requested_at.as_deref(), Some("2026-04-19T21:30:00Z"));
    }

    #[test]
    fn test_node_reclaim_request_tolerates_missing_reason_and_timestamp() {
        // Agent should always write all three, but the controller must not
        // refuse to reclaim if the reason/timestamp were somehow dropped —
        // the trigger is the boolean. Missing metadata becomes None fields.
        let node = node_with_annotations(
            "worker-1",
            &[(crate::constants::RECLAIM_REQUESTED_ANNOTATION, "true")],
        );
        let req = node_reclaim_request(&node).expect("trigger alone is enough");
        assert!(req.reason.is_none());
        assert!(req.requested_at.is_none());
    }

    // ========================================================================
    // build_clear_reclaim_patch — wipe all three annotations in one PATCH
    //   Run as the last step of Phase::EmergencyRemove so that a rejoined
    //   Node does not immediately re-fire on the stale annotation.
    // ========================================================================

    #[test]
    fn test_build_clear_reclaim_patch_sets_three_annotations_to_null() {
        let patch = build_clear_reclaim_patch();
        let annotations = patch
            .pointer("/metadata/annotations")
            .expect("patch must write /metadata/annotations")
            .as_object()
            .expect("must be object");
        for key in [
            crate::constants::RECLAIM_REQUESTED_ANNOTATION,
            crate::constants::RECLAIM_REASON_ANNOTATION,
            crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION,
        ] {
            let v = annotations
                .get(key)
                .unwrap_or_else(|| panic!("clear patch must address {key}"));
            assert!(
                v.is_null(),
                "merge-patch convention: null deletes the key, got {v:?} for {key}"
            );
        }
    }

    #[test]
    fn test_build_clear_reclaim_patch_does_not_touch_other_metadata() {
        // Same strategic-merge safety contract as the agent-side patch:
        // only metadata.annotations is addressed.
        let patch = build_clear_reclaim_patch();
        let obj = patch.as_object().expect("is object");
        assert_eq!(obj.len(), 1, "top level must be only {{metadata}}");
        let meta = obj
            .get("metadata")
            .and_then(|v| v.as_object())
            .expect("metadata object");
        assert_eq!(meta.len(), 1, "metadata must contain only annotations");
    }

    // ========================================================================
    // build_disable_schedule_patch — flip spec.schedule.enabled=false
    //   Part of Phase::EmergencyRemove. Rationale: without this flip, the
    //   next schedule window re-adds the node, the agent sees the user's
    //   still-running JVM, and the eject→re-add→re-eject loop repeats every
    //   schedule boundary. Setting enabled=false makes the user's explicit
    //   re-enable the signal to return the node to service.
    //   See docs/roadmaps/5spot-emergency-reclaim-by-process-match.md (Q6).
    // ========================================================================

    #[test]
    fn test_build_disable_schedule_patch_sets_enabled_false() {
        let patch = build_disable_schedule_patch();
        let enabled = patch
            .pointer("/spec/schedule/enabled")
            .expect("patch must write /spec/schedule/enabled");
        assert_eq!(
            enabled,
            &serde_json::Value::Bool(false),
            "enabled must be the JSON literal false (merge-patch replaces, does not delete)"
        );
    }

    #[test]
    fn test_build_disable_schedule_patch_only_touches_schedule_enabled() {
        // Strategic-merge safety contract: we must not accidentally blow
        // away siblings of `enabled` under spec.schedule (daysOfWeek,
        // hoursOfDay, timezone) nor siblings of `schedule` under spec
        // (killSwitch, killIfCommands, bootstrapSpec, ...). Merge-patch
        // semantics mean any key we include at a given depth replaces
        // only that key — so the exact structure of the patch body is
        // the load-bearing invariant.
        let patch = build_disable_schedule_patch();
        let top = patch.as_object().expect("top must be object");
        assert_eq!(top.len(), 1, "top level must be only {{spec}}");

        let spec = top
            .get("spec")
            .and_then(|v| v.as_object())
            .expect("spec object");
        assert_eq!(spec.len(), 1, "spec must contain only {{schedule}}");

        let schedule = spec
            .get("schedule")
            .and_then(|v| v.as_object())
            .expect("schedule object");
        assert_eq!(
            schedule.len(),
            1,
            "schedule must contain only {{enabled}} — other fields \
             (daysOfWeek, hoursOfDay, timezone) must be untouched"
        );
        assert!(
            schedule.contains_key("enabled"),
            "schedule must contain the enabled key"
        );
    }

    #[test]
    fn test_build_disable_schedule_patch_never_writes_true() {
        // Belt-and-braces: the function is pure and hardcoded, so this
        // test mostly guards against a future refactor accidentally
        // parameterising the value. Re-enable must be an explicit human
        // action, never a controller decision.
        let patch = build_disable_schedule_patch();
        let enabled = patch
            .pointer("/spec/schedule/enabled")
            .expect("must have /spec/schedule/enabled");
        assert_eq!(
            enabled.as_bool(),
            Some(false),
            "build_disable_schedule_patch must never emit enabled=true; \
             re-enable is a user-driven action"
        );
    }

    // ========================================================================
    // build_emergency_reclaim_event / build_emergency_disable_schedule_event /
    // emergency_reclaim_message — event + message builders for Phase 3 dispatch
    // ========================================================================

    #[test]
    fn test_build_emergency_reclaim_event_is_warning() {
        use kube::runtime::events::EventType;
        let req = ReclaimRequest {
            reason: Some("process-match: java".to_string()),
            requested_at: Some("2026-04-20T22:00:00Z".to_string()),
        };
        let ev = build_emergency_reclaim_event(&req);
        assert!(
            matches!(ev.type_, EventType::Warning),
            "emergency reclaim must emit a Warning event so it paints red in \
             kubectl describe and wakes oncall dashboards"
        );
    }

    #[test]
    fn test_build_emergency_reclaim_event_reason_is_camelcase_constant() {
        use crate::constants::REASON_EMERGENCY_RECLAIM;
        let req = ReclaimRequest {
            reason: None,
            requested_at: None,
        };
        let ev = build_emergency_reclaim_event(&req);
        assert_eq!(
            ev.reason, REASON_EMERGENCY_RECLAIM,
            "Event.reason must be the REASON_EMERGENCY_RECLAIM constant so \
             operators can filter `kubectl get events` on it"
        );
    }

    #[test]
    fn test_build_emergency_reclaim_event_note_includes_reason() {
        let req = ReclaimRequest {
            reason: Some("process-match: java".to_string()),
            requested_at: Some("2026-04-20T22:00:00Z".to_string()),
        };
        let ev = build_emergency_reclaim_event(&req);
        let note = ev.note.as_deref().unwrap_or_default();
        assert!(
            note.contains("process-match: java"),
            "Event.note must surface the agent-supplied reason verbatim so \
             the operator can correlate without reading node logs — got: {note}"
        );
    }

    #[test]
    fn test_build_emergency_reclaim_event_note_handles_missing_reason() {
        // Reason annotation is best-effort on the agent side; a trigger
        // with no reason must not produce an empty event body.
        let req = ReclaimRequest {
            reason: None,
            requested_at: None,
        };
        let ev = build_emergency_reclaim_event(&req);
        let note = ev.note.as_deref().unwrap_or_default();
        assert!(
            !note.trim().is_empty(),
            "Event.note must never be empty — a missing reason should fall \
             back to a descriptive default, not a blank string"
        );
    }

    #[test]
    fn test_build_emergency_disable_schedule_event_is_warning() {
        use kube::runtime::events::EventType;
        let ev = build_emergency_disable_schedule_event();
        assert!(
            matches!(ev.type_, EventType::Warning),
            "disabling the schedule as part of emergency reclaim must be a \
             Warning: the user did not ask for it, the controller did, and \
             they must notice before the next schedule window"
        );
    }

    #[test]
    fn test_build_emergency_disable_schedule_event_reason_constant() {
        use crate::constants::REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE;
        let ev = build_emergency_disable_schedule_event();
        assert_eq!(
            ev.reason, REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE,
            "must use the dedicated reason constant so `kubectl get events \
             --field-selector reason=EmergencyReclaimDisabledSchedule` works"
        );
    }

    #[test]
    fn test_build_emergency_disable_schedule_event_note_mentions_reenable() {
        // The operator must know that re-enabling is a user action —
        // otherwise they'll wait for the controller to auto-resume.
        let ev = build_emergency_disable_schedule_event();
        let note = ev.note.as_deref().unwrap_or_default().to_lowercase();
        assert!(
            note.contains("enabled") || note.contains("schedule"),
            "note must reference the disabled schedule so an operator \
             reading `kubectl describe` understands why the node isn't \
             coming back — got: {note}"
        );
    }

    #[test]
    fn test_emergency_reclaim_message_includes_node_name() {
        let req = ReclaimRequest {
            reason: Some("process-match: java".to_string()),
            requested_at: Some("2026-04-20T22:00:00Z".to_string()),
        };
        let msg = emergency_reclaim_message("node-worker-03", &req);
        assert!(
            msg.contains("node-worker-03"),
            "message must include the node name so the status condition on \
             the SM is self-contained (no cross-resource lookup) — got: {msg}"
        );
    }

    #[test]
    fn test_emergency_reclaim_message_includes_reason_when_present() {
        let req = ReclaimRequest {
            reason: Some("process-match: java".to_string()),
            requested_at: None,
        };
        let msg = emergency_reclaim_message("node-x", &req);
        assert!(
            msg.contains("process-match: java"),
            "reason must be embedded verbatim when present — got: {msg}"
        );
    }

    #[test]
    fn test_emergency_reclaim_message_without_reason_still_meaningful() {
        let req = ReclaimRequest {
            reason: None,
            requested_at: None,
        };
        let msg = emergency_reclaim_message("node-x", &req);
        assert!(
            !msg.trim().is_empty() && msg.contains("node-x"),
            "missing reason must not blank the message; node name is the \
             minimum floor — got: {msg}"
        );
    }

    // ========================================================================
    // Phase 2.5 remainder — reclaim-agent provisioning helpers
    //
    // The controller stamps a label + projects a per-node ConfigMap onto
    // every Node backing a ScheduledMachine whose `spec.killIfCommands` is
    // non-empty. Tests below pin the pure builders (no I/O) that back the
    // async orchestrator.
    // ========================================================================

    #[test]
    fn test_render_reclaim_toml_parses_back_via_agent_config() {
        // End-to-end contract: whatever we render, the agent must parse.
        // This is the load-bearing invariant — a rendered ConfigMap that
        // the agent rejects at startup would leave a node in the opt-in
        // label-stamped state but without a working detector.
        let rendered = render_reclaim_toml(&["java".to_string(), "idea".to_string()]);
        let parsed = crate::reclaim_agent::parse_config(&rendered)
            .expect("rendered TOML must parse via reclaim_agent::parse_config");
        assert_eq!(
            parsed.match_commands,
            vec!["java".to_string(), "idea".to_string()],
            "match_commands must round-trip verbatim"
        );
        assert!(
            parsed.match_argv_substrings.is_empty(),
            "CRD surface is commands-only; argv list must render empty"
        );
        assert_eq!(
            parsed.poll_interval_ms,
            crate::reclaim_agent::DEFAULT_POLL_INTERVAL_MS,
            "poll interval must default to the agent constant so a spec \
             change cannot accidentally tune the poll loop"
        );
    }

    #[test]
    fn test_render_reclaim_toml_empty_commands_still_parses() {
        // An empty-list render should not produce malformed TOML — the
        // controller's orchestrator is responsible for not calling render
        // with an empty list (it deletes the ConfigMap instead) but the
        // renderer must never produce a document the agent rejects.
        let rendered = render_reclaim_toml(&[]);
        let parsed = crate::reclaim_agent::parse_config(&rendered)
            .expect("empty-commands render must still be valid TOML");
        assert!(parsed.match_commands.is_empty());
    }

    #[test]
    fn test_render_reclaim_toml_escapes_quote_in_command() {
        // Defensive: a `killIfCommands` entry containing `"` must not
        // break the TOML. Real-world pattern list shouldn't include
        // quotes, but the renderer must still produce parseable output
        // so a typo in the spec doesn't brick the agent.
        let rendered = render_reclaim_toml(&["weird\"name".to_string()]);
        let parsed = crate::reclaim_agent::parse_config(&rendered)
            .expect("quote in command must be escaped, not break the TOML");
        assert_eq!(parsed.match_commands, vec!["weird\"name".to_string()]);
    }

    #[test]
    fn test_per_node_configmap_name_uses_prefix_constant() {
        use crate::constants::RECLAIM_AGENT_CONFIGMAP_PREFIX;
        let got = per_node_configmap_name("worker-03");
        assert_eq!(got, format!("{RECLAIM_AGENT_CONFIGMAP_PREFIX}worker-03"));
    }

    #[test]
    fn test_per_node_configmap_name_preserves_node_name_verbatim() {
        // Node names are DNS-1123 by the time they reach us (kubelet
        // enforces it). We do not re-sanitise — lowering or hashing the
        // node name would make the projected ConfigMap unguessable from
        // the node's identity, and the DaemonSet's future per-node mount
        // contract depends on name == prefix+node-name.
        let got = per_node_configmap_name("dev-workstation-ebourgeo-01.lab");
        assert!(
            got.ends_with("dev-workstation-ebourgeo-01.lab"),
            "node name must survive verbatim — got: {got}"
        );
    }

    #[test]
    fn test_build_reclaim_agent_configmap_name_and_namespace() {
        use crate::constants::RECLAIM_AGENT_NAMESPACE;
        let cm = build_reclaim_agent_configmap("node-a", &["java".to_string()]);
        assert_eq!(
            cm.metadata.namespace.as_deref(),
            Some(RECLAIM_AGENT_NAMESPACE),
            "ConfigMap must land in the agent's dedicated namespace"
        );
        assert_eq!(
            cm.metadata.name.as_deref(),
            Some(per_node_configmap_name("node-a").as_str()),
            "ConfigMap name must match per_node_configmap_name(node) so the \
             agent's volume mount can address it"
        );
    }

    #[test]
    fn test_build_reclaim_agent_configmap_data_key_is_reclaim_toml() {
        // The agent's kube watcher reads the TOML body from this exact
        // data key — this is the contract between controller (projects
        // the key via `RECLAIM_CONFIG_DATA_KEY`) and agent (pulls the
        // same key out of the observed ConfigMap in `configmap_to_config`).
        // Renaming here without updating both sides breaks arming silently.
        let cm = build_reclaim_agent_configmap("node-a", &["idea".to_string()]);
        let data = cm.data.expect("ConfigMap must have a data map");
        assert!(
            data.contains_key("reclaim.toml"),
            "data map must include the reclaim.toml key the agent watches"
        );
        // And the value must round-trip through parse_config.
        let toml_text = data.get("reclaim.toml").unwrap();
        let parsed =
            crate::reclaim_agent::parse_config(toml_text).expect("projected TOML must parse");
        assert_eq!(parsed.match_commands, vec!["idea".to_string()]);
    }

    #[test]
    fn test_build_reclaim_agent_configmap_carries_owner_labels() {
        // A stamped ConfigMap without identifying labels is invisible to
        // `kubectl get -l app.kubernetes.io/component=reclaim-agent` —
        // which is the operator's only way to list "all projections the
        // 5-spot controller has made". The component label is the
        // load-bearing one; the rest are nice-to-haves.
        let cm = build_reclaim_agent_configmap("node-a", &["java".to_string()]);
        let labels = cm.metadata.labels.expect("ConfigMap must carry labels");
        assert_eq!(
            labels
                .get("app.kubernetes.io/component")
                .map(String::as_str),
            Some("reclaim-agent"),
            "component label is the filter key used by operator tooling"
        );
    }

    #[test]
    fn test_build_reclaim_agent_label_patch_enable_writes_enabled_value() {
        use crate::constants::{RECLAIM_AGENT_LABEL, RECLAIM_AGENT_LABEL_ENABLED};
        let patch = build_reclaim_agent_label_patch(true);
        let labels = patch
            .pointer("/metadata/labels")
            .and_then(|v| v.as_object())
            .expect("patch must write metadata.labels object");
        assert_eq!(
            labels.get(RECLAIM_AGENT_LABEL).and_then(|v| v.as_str()),
            Some(RECLAIM_AGENT_LABEL_ENABLED),
            "enable patch must set the reclaim-agent label to the 'enabled' \
             constant so the DaemonSet nodeSelector matches"
        );
    }

    #[test]
    fn test_build_reclaim_agent_label_patch_disable_writes_null() {
        use crate::constants::RECLAIM_AGENT_LABEL;
        // Merge-patch semantics: null deletes the key. Using null (rather
        // than an empty string) is what lets `kubectl get node -l
        // 5spot.finos.org/reclaim-agent` return zero nodes after tear-down.
        let patch = build_reclaim_agent_label_patch(false);
        let labels = patch
            .pointer("/metadata/labels")
            .and_then(|v| v.as_object())
            .expect("patch must write metadata.labels object");
        assert!(
            labels.get(RECLAIM_AGENT_LABEL).is_some_and(|v| v.is_null()),
            "disable patch must set the reclaim-agent label to JSON null \
             so merge-patch deletes it (empty string would leave it set \
             and keep the DaemonSet pod scheduled)"
        );
    }

    #[test]
    fn test_build_reclaim_agent_label_patch_only_touches_reclaim_label() {
        use crate::constants::RECLAIM_AGENT_LABEL;
        // Strategic-merge safety: the patch must not clobber siblings
        // under metadata.labels (kata-runtime, topology labels, etc.) or
        // fields outside metadata. Without this assert a refactor could
        // accidentally blow away cluster-critical node labels.
        let patch = build_reclaim_agent_label_patch(true);
        let top = patch.as_object().expect("top must be object");
        assert_eq!(top.len(), 1, "top level must be only {{metadata}}");
        let meta = top
            .get("metadata")
            .and_then(|v| v.as_object())
            .expect("metadata object");
        assert_eq!(meta.len(), 1, "metadata must contain only {{labels}}");
        let labels = meta.get("labels").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            labels.len(),
            1,
            "labels must contain exactly one key ({RECLAIM_AGENT_LABEL}) — \
             other labels on the Node must survive untouched"
        );
    }

    // ========================================================================
    // reconcile_reclaim_agent_provision — mock-API orchestrator tests
    //
    // The pure-helper tests above pin the request-body shape; these tests
    // pin the async request *sequence* against a mock kube client. Together
    // they give end-to-end coverage of the Phase-2.5 projection contract
    // without needing a real cluster:
    //   1. Always: cluster-scoped Node merge-patch for the reclaim-agent label.
    //   2a. Commands non-empty: server-side apply of the per-node ConfigMap
    //       in 5spot-system with fieldManager=5spot-controller-reclaim-agent
    //       and force=true.
    //   2b. Commands empty: DELETE of the same per-node ConfigMap — a 404
    //       is benign so a re-run after partial tear-down completes cleanly.
    //   3. Error propagation: a 5xx on the Node label PATCH must short-
    //       circuit before the ConfigMap work (no second request issued).
    // ========================================================================

    #[tokio::test]
    async fn test_reconcile_reclaim_agent_provision_non_empty_patches_label_then_applies_cm() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);

            let (req, send) = h.next_request().await.expect("expected Node PATCH");
            assert_eq!(req.method(), http::Method::PATCH);
            assert!(
                req.uri().path().ends_with("/api/v1/nodes/node-a"),
                "must PATCH the cluster-scoped Node, got: {}",
                req.uri().path()
            );
            let body = collect_json_body(req.into_body()).await;
            assert_eq!(
                body.pointer("/metadata/labels/5spot.finos.org~1reclaim-agent")
                    .and_then(|v| v.as_str()),
                Some("enabled"),
                "label PATCH must set the reclaim-agent label to 'enabled'"
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "Node",
                            "metadata": { "name": "node-a", "resourceVersion": "2" }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );

            let (req, send) = h.next_request().await.expect("expected ConfigMap apply");
            assert_eq!(req.method(), http::Method::PATCH);
            let path = req.uri().path().to_string();
            assert!(
                path.contains("/api/v1/namespaces/5spot-system/configmaps/reclaim-agent-node-a"),
                "apply must target the per-node CM in 5spot-system, got: {path}"
            );
            let query = req.uri().query().unwrap_or("");
            assert!(
                query.contains("fieldManager=5spot-controller-reclaim-agent"),
                "SSA must use the dedicated field manager (distinct from the \
                 main reconciler) so audit logs attribute projection writes \
                 separately — got query: {query}"
            );
            assert!(
                query.contains("force=true"),
                "SSA must force so hand-edited ConfigMap fields snap back to \
                 the controller's view — got query: {query}"
            );
            let body = collect_json_body(req.into_body()).await;
            assert_eq!(
                body.pointer("/metadata/name").and_then(|v| v.as_str()),
                Some("reclaim-agent-node-a"),
                "applied CM body must carry the per-node name"
            );
            assert!(
                body.pointer("/data/reclaim.toml").is_some(),
                "applied CM body must carry the reclaim.toml key the agent watches"
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "ConfigMap",
                            "metadata": {
                                "name": "reclaim-agent-node-a",
                                "namespace": "5spot-system",
                                "resourceVersion": "3"
                            },
                            "data": { "reclaim.toml": "" }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );
        });

        let result = reconcile_reclaim_agent_provision(
            &client,
            "node-a",
            &["java".to_string(), "idea".to_string()],
        )
        .await;

        assert!(
            result.is_ok(),
            "happy path must return Ok(()), got: {result:?}"
        );
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_reclaim_agent_provision_empty_patches_label_then_deletes_cm() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);

            let (req, send) = h.next_request().await.expect("expected Node PATCH");
            assert_eq!(req.method(), http::Method::PATCH);
            assert!(req.uri().path().ends_with("/api/v1/nodes/node-b"));
            let body = collect_json_body(req.into_body()).await;
            assert!(
                body.pointer("/metadata/labels/5spot.finos.org~1reclaim-agent")
                    .is_some_and(serde_json::Value::is_null),
                "empty-commands label PATCH must set the label to JSON null \
                 (merge-patch delete sentinel) so kubectl get -l stops \
                 matching the node"
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "Node",
                            "metadata": { "name": "node-b", "resourceVersion": "2" }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );

            let (req, send) = h.next_request().await.expect("expected ConfigMap DELETE");
            assert_eq!(req.method(), http::Method::DELETE);
            assert!(
                req.uri()
                    .path()
                    .contains("/api/v1/namespaces/5spot-system/configmaps/reclaim-agent-node-b"),
                "DELETE must target the per-node CM in 5spot-system, got: {}",
                req.uri().path()
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "kind": "Status",
                            "apiVersion": "v1",
                            "status": "Success"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );
        });

        let result = reconcile_reclaim_agent_provision(&client, "node-b", &[]).await;
        assert!(
            result.is_ok(),
            "empty-commands path must return Ok(()), got: {result:?}"
        );
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_reclaim_agent_provision_empty_404_on_delete_is_benign_ok() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);

            let (_req, send) = h.next_request().await.expect("expected Node PATCH");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "Node",
                            "metadata": { "name": "node-c", "resourceVersion": "2" }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );

            let (_req, send) = h.next_request().await.expect("expected ConfigMap DELETE");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(
                        404,
                        "configmaps \"reclaim-agent-node-c\" not found",
                    )))
                    .unwrap(),
            );
        });

        let result = reconcile_reclaim_agent_provision(&client, "node-c", &[]).await;
        assert!(
            result.is_ok(),
            "404 on tear-down delete must be benign Ok(()) so a re-run after \
             a partial prior tear-down completes cleanly, got: {result:?}"
        );
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_reclaim_agent_provision_label_patch_500_propagates_and_short_circuits()
    {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);

            let (_req, send) = h.next_request().await.expect("expected Node PATCH");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "internal server error")))
                    .unwrap(),
            );
        });

        let result =
            reconcile_reclaim_agent_provision(&client, "node-d", &["java".to_string()]).await;

        assert!(
            matches!(
                result,
                Err(crate::reconcilers::ReconcilerError::KubeError(_))
            ),
            "Node label PATCH 500 must propagate as KubeError (no silent \
             degradation — best-effort is at the caller level, not here), \
             got: {result:?}"
        );
        srv.await.unwrap();
    }

    // ========================================================================
    // Phase 3 — diff_node_taints pure helper
    // ========================================================================

    use crate::crd::{NodeTaint, TaintEffect};
    use k8s_openapi::api::core::v1::Taint as CoreTaint;

    fn desired(key: &str, value: Option<&str>, effect: TaintEffect) -> NodeTaint {
        NodeTaint {
            key: key.to_string(),
            value: value.map(str::to_string),
            effect,
        }
    }

    fn core_taint(key: &str, value: Option<&str>, effect: &str) -> CoreTaint {
        CoreTaint {
            key: key.to_string(),
            value: value.map(str::to_string),
            effect: effect.to_string(),
            time_added: None,
        }
    }

    #[test]
    fn test_diff_node_taints_all_empty() {
        let plan = diff_node_taints(&[], &[], &[]);
        assert!(plan.to_add.is_empty());
        assert!(plan.to_update.is_empty());
        assert!(plan.to_remove.is_empty());
        assert!(plan.unchanged.is_empty());
        assert!(plan.conflicts.is_empty());
        assert!(
            plan.is_noop(),
            "fully-empty plan must report is_noop() == true"
        );
    }

    #[test]
    fn test_diff_node_taints_add_only() {
        let desired_list = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let plan = diff_node_taints(&[], &desired_list, &[]);
        assert_eq!(plan.to_add, desired_list);
        assert!(plan.to_update.is_empty());
        assert!(plan.to_remove.is_empty());
        assert!(plan.unchanged.is_empty());
        assert!(plan.conflicts.is_empty());
        assert!(!plan.is_noop());
    }

    #[test]
    fn test_diff_node_taints_unchanged_when_already_applied() {
        let desired_list = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let current = vec![core_taint("workload", Some("batch"), "NoSchedule")];
        let previously = desired_list.clone();
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert!(plan.to_add.is_empty());
        assert!(plan.to_update.is_empty());
        assert!(plan.to_remove.is_empty());
        assert_eq!(plan.unchanged, desired_list);
        assert!(plan.is_noop(), "already-applied state must be a no-op");
    }

    #[test]
    fn test_diff_node_taints_update_when_value_changes_and_we_own_it() {
        let desired_list = vec![desired("workload", Some("newval"), TaintEffect::NoSchedule)];
        let current = vec![core_taint("workload", Some("oldval"), "NoSchedule")];
        let previously = vec![desired("workload", Some("oldval"), TaintEffect::NoSchedule)];
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert_eq!(plan.to_update, desired_list);
        assert!(plan.to_add.is_empty());
        assert!(plan.to_remove.is_empty());
        assert!(plan.unchanged.is_empty());
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn test_diff_node_taints_conflict_when_value_changes_but_we_do_not_own_it() {
        // Admin independently added (workload, NoSchedule) with a different value.
        // We do NOT own it (not in previously_applied) — surface as conflict, do NOT overwrite.
        let desired_list = vec![desired("workload", Some("ours"), TaintEffect::NoSchedule)];
        let current = vec![core_taint("workload", Some("admin"), "NoSchedule")];
        let previously = vec![]; // we have not applied anything yet
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert!(
            plan.to_add.is_empty(),
            "must not add (admin already has the taint)"
        );
        assert!(
            plan.to_update.is_empty(),
            "must not overwrite admin's value"
        );
        assert!(plan.to_remove.is_empty());
        assert_eq!(plan.conflicts, desired_list, "must surface as conflict");
    }

    #[test]
    fn test_diff_node_taints_remove_when_previously_applied_and_still_on_node() {
        let desired_list = vec![];
        let current = vec![core_taint("workload", Some("batch"), "NoSchedule")];
        let previously = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert_eq!(plan.to_remove, previously);
        assert!(plan.to_add.is_empty());
        assert!(plan.to_update.is_empty());
        assert!(plan.unchanged.is_empty());
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn test_diff_node_taints_does_not_remove_admin_taint_not_previously_applied() {
        // We applied nothing previously. Admin-added (workload, NoSchedule) is on
        // current, but we never owned it — do NOT remove, do NOT conflict (we're
        // not trying to claim it).
        let desired_list = vec![];
        let current = vec![core_taint("workload", Some("admin"), "NoSchedule")];
        let previously = vec![];
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert!(
            plan.to_remove.is_empty(),
            "admin-added taint must be preserved"
        );
        assert!(plan.to_add.is_empty());
        assert!(plan.to_update.is_empty());
        assert!(plan.unchanged.is_empty());
        assert!(plan.conflicts.is_empty());
        assert!(plan.is_noop());
    }

    #[test]
    fn test_diff_node_taints_does_not_remove_when_previously_owned_but_admin_re_added() {
        // We applied it; admin then re-added with a different value. Removing would
        // stomp admin's intent. Leave it alone, surface as conflict.
        let desired_list = vec![];
        let current = vec![core_taint("workload", Some("admin"), "NoSchedule")];
        let previously = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert!(
            plan.to_remove.is_empty(),
            "value mismatch with admin means we do not own current — must not remove"
        );
        assert_eq!(plan.conflicts.len(), 1, "ownership conflict must surface");
        assert_eq!(plan.conflicts[0].key, "workload");
    }

    #[test]
    fn test_diff_node_taints_same_key_different_effect_treated_independently() {
        let desired_list = vec![
            desired("workload", Some("batch"), TaintEffect::NoSchedule),
            desired("workload", Some("batch"), TaintEffect::NoExecute),
        ];
        let current = vec![core_taint("workload", Some("batch"), "NoSchedule")];
        let previously = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert_eq!(
            plan.unchanged,
            vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)]
        );
        assert_eq!(
            plan.to_add,
            vec![desired("workload", Some("batch"), TaintEffect::NoExecute)]
        );
    }

    #[test]
    fn test_diff_node_taints_mixed_plan() {
        let desired_list = vec![
            desired("workload", Some("batch"), TaintEffect::NoSchedule), // unchanged
            desired("dedicated", Some("ml-new"), TaintEffect::NoExecute), // update
            desired("priority", Some("high"), TaintEffect::PreferNoSchedule), // add
        ];
        let current = vec![
            core_taint("workload", Some("batch"), "NoSchedule"),
            core_taint("dedicated", Some("ml-old"), "NoExecute"),
            core_taint("legacy", Some("gone"), "NoSchedule"), // will be removed
        ];
        let previously = vec![
            desired("workload", Some("batch"), TaintEffect::NoSchedule),
            desired("dedicated", Some("ml-old"), TaintEffect::NoExecute),
            desired("legacy", Some("gone"), TaintEffect::NoSchedule),
        ];
        let plan = diff_node_taints(&current, &desired_list, &previously);
        assert_eq!(plan.unchanged.len(), 1);
        assert_eq!(plan.unchanged[0].key, "workload");
        assert_eq!(plan.to_update.len(), 1);
        assert_eq!(plan.to_update[0].key, "dedicated");
        assert_eq!(plan.to_update[0].value.as_deref(), Some("ml-new"));
        assert_eq!(plan.to_add.len(), 1);
        assert_eq!(plan.to_add[0].key, "priority");
        assert_eq!(plan.to_remove.len(), 1);
        assert_eq!(plan.to_remove[0].key, "legacy");
        assert!(plan.conflicts.is_empty());
        assert!(!plan.is_noop());
    }

    #[test]
    fn test_diff_node_taints_ignores_unknown_effect_on_current() {
        // A taint on the Node with an effect string we don't recognise is
        // treated as an admin taint and must not cause a panic.
        let desired_list = vec![];
        let current = vec![core_taint("odd", Some("v"), "NotAnEffectWeKnow")];
        let plan = diff_node_taints(&current, &desired_list, &[]);
        assert!(plan.is_noop());
    }

    // ========================================================================
    // Phase 3 — apply_node_taints IO (mocked kube::Client)
    // ========================================================================

    fn node_response_body(name: &str, existing_taints: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": { "name": name, "resourceVersion": "1" },
            "spec": { "taints": existing_taints },
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn test_apply_node_taints_noop_makes_no_http_call() {
        let (client, handle) = mock_client_pair();
        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // If apply_node_taints tries to make a call, this will observe it.
            let maybe = h.next_request().await;
            assert!(
                maybe.is_none(),
                "no-op plan must not make any HTTP request, got: {maybe:?}"
            );
        });
        drop(srv);

        let plan = NodeTaintPlan::default();
        apply_node_taints(&client, "worker-1", &plan)
            .await
            .expect("no-op apply must return Ok");
    }

    #[tokio::test]
    async fn test_apply_node_taints_add_patches_node_and_sets_annotation() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // 1. GET Node (we read current taints before patching)
            let (req, send) = h.next_request().await.expect("expected Node GET");
            assert_eq!(req.method(), http::Method::GET);
            assert!(
                req.uri().path().ends_with("/api/v1/nodes/worker-1"),
                "unexpected GET path: {}",
                req.uri().path()
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(node_response_body(
                        "worker-1",
                        serde_json::json!([]),
                    )))
                    .unwrap(),
            );

            // 2. PATCH Node (merge or apply)
            let (req, send) = h.next_request().await.expect("expected Node PATCH");
            assert_eq!(req.method(), http::Method::PATCH);
            assert!(
                req.uri().path().ends_with("/api/v1/nodes/worker-1"),
                "unexpected PATCH path: {}",
                req.uri().path()
            );
            let body = http_body_util::BodyExt::collect(req.into_body())
                .await
                .unwrap()
                .to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            // spec.taints present with workload=batch:NoSchedule
            let taints = json.pointer("/spec/taints").expect("spec.taints in body");
            assert_eq!(
                taints[0]["key"], "workload",
                "patch body must include the workload taint: {taints}"
            );
            assert_eq!(taints[0]["effect"], "NoSchedule");
            // annotation written
            let annotations = json
                .pointer("/metadata/annotations")
                .expect("annotations in body");
            assert!(
                annotations.get("5spot.finos.org/applied-taints").is_some(),
                "ownership annotation must be set: {annotations}"
            );
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(node_response_body(
                        "worker-1",
                        serde_json::json!([{"key":"workload","value":"batch","effect":"NoSchedule"}]),
                    )))
                    .unwrap(),
            );
        });

        let plan = NodeTaintPlan {
            to_add: vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)],
            ..Default::default()
        };
        apply_node_taints(&client, "worker-1", &plan)
            .await
            .expect("apply should succeed");
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_apply_node_taints_propagates_patch_error() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // GET Node
            let (_req, send) = h.next_request().await.expect("expected Node GET");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(node_response_body(
                        "worker-1",
                        serde_json::json!([]),
                    )))
                    .unwrap(),
            );
            // PATCH fails 500
            let (_req, send) = h.next_request().await.expect("expected Node PATCH");
            send.send_response(
                Response::builder()
                    .status(500)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(500, "boom")))
                    .unwrap(),
            );
        });

        let plan = NodeTaintPlan {
            to_add: vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)],
            ..Default::default()
        };
        let result = apply_node_taints(&client, "worker-1", &plan).await;
        assert!(result.is_err(), "patch 500 must propagate, got: {result:?}");
        srv.await.unwrap();
    }

    // ========================================================================
    // Phase 4 — reconcile_node_taints orchestration
    // ========================================================================

    fn ready_node_body(name: &str, taints: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": { "name": name, "resourceVersion": "1" },
            "spec": { "taints": taints },
            "status": {
                "conditions": [
                    {"type": "Ready", "status": "True", "lastHeartbeatTime": "2026-04-19T00:00:00Z", "lastTransitionTime": "2026-04-19T00:00:00Z"}
                ]
            }
        }))
        .unwrap()
    }

    fn not_ready_node_body(name: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": { "name": name, "resourceVersion": "1" },
            "spec": { "taints": [] },
            "status": {
                "conditions": [
                    {"type": "Ready", "status": "False", "lastHeartbeatTime": "2026-04-19T00:00:00Z", "lastTransitionTime": "2026-04-19T00:00:00Z"}
                ]
            }
        }))
        .unwrap()
    }

    use crate::reconcilers::helpers::{NodeTaintReconcileOutcome, ReconcileNodeTaintsInput};

    #[tokio::test]
    async fn test_reconcile_node_taints_node_not_found_returns_no_node_yet() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected Node GET");
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(k8s_error_body(404, "not found")))
                    .unwrap(),
            );
        });

        let outcome = reconcile_node_taints(
            &client,
            ReconcileNodeTaintsInput {
                node_name: "missing",
                desired: &[],
                previously_applied: &[],
            },
        )
        .await
        .expect("404 must surface as outcome, not error");
        assert!(
            matches!(outcome, NodeTaintReconcileOutcome::NoNodeYet),
            "expected NoNodeYet, got: {outcome:?}"
        );
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_node_taints_not_ready_returns_node_not_ready() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected Node GET");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(not_ready_node_body("worker-1")))
                    .unwrap(),
            );
        });

        let desired_list = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let outcome = reconcile_node_taints(
            &client,
            ReconcileNodeTaintsInput {
                node_name: "worker-1",
                desired: &desired_list,
                previously_applied: &[],
            },
        )
        .await
        .expect("not-ready must surface as outcome, not error");
        assert!(
            matches!(outcome, NodeTaintReconcileOutcome::NodeNotReady),
            "expected NodeNotReady, got: {outcome:?}"
        );
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_node_taints_ready_and_noop_returns_applied() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected Node GET");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(ready_node_body(
                        "worker-1",
                        serde_json::json!([{"key": "workload", "value": "batch", "effect": "NoSchedule"}]),
                    )))
                    .unwrap(),
            );
            // No PATCH expected (noop).
        });

        let desired_list = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let previously_list = desired_list.clone();
        let outcome = reconcile_node_taints(
            &client,
            ReconcileNodeTaintsInput {
                node_name: "worker-1",
                desired: &desired_list,
                previously_applied: &previously_list,
            },
        )
        .await
        .expect("noop must succeed");
        match outcome {
            NodeTaintReconcileOutcome::Applied { applied } => {
                assert_eq!(applied.len(), 1);
                assert_eq!(applied[0].key, "workload");
            }
            other => panic!("expected Applied, got: {other:?}"),
        }
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_node_taints_ready_and_needs_add_patches_and_returns_applied() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            // 1. GET node (from reconcile_node_taints — check Ready)
            let (_req, send) = h.next_request().await.expect("expected Node GET #1");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(ready_node_body(
                        "worker-1",
                        serde_json::json!([]),
                    )))
                    .unwrap(),
            );
            // 2. GET node (from apply_node_taints — read current spec)
            let (_req, send) = h.next_request().await.expect("expected Node GET #2");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(ready_node_body(
                        "worker-1",
                        serde_json::json!([]),
                    )))
                    .unwrap(),
            );
            // 3. PATCH node
            let (req, send) = h.next_request().await.expect("expected Node PATCH");
            assert_eq!(req.method(), http::Method::PATCH);
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(ready_node_body(
                        "worker-1",
                        serde_json::json!([{"key": "workload", "value": "batch", "effect": "NoSchedule"}]),
                    )))
                    .unwrap(),
            );
        });

        let desired_list = vec![desired("workload", Some("batch"), TaintEffect::NoSchedule)];
        let outcome = reconcile_node_taints(
            &client,
            ReconcileNodeTaintsInput {
                node_name: "worker-1",
                desired: &desired_list,
                previously_applied: &[],
            },
        )
        .await
        .expect("apply should succeed");
        match outcome {
            NodeTaintReconcileOutcome::Applied { applied } => {
                assert_eq!(applied.len(), 1);
                assert_eq!(applied[0].key, "workload");
            }
            other => panic!("expected Applied, got: {other:?}"),
        }
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn test_reconcile_node_taints_conflict_returns_conflict_variant() {
        let (client, handle) = mock_client_pair();

        let srv = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.expect("expected Node GET");
            send.send_response(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(ready_node_body(
                        "worker-1",
                        serde_json::json!([{"key": "workload", "value": "admin", "effect": "NoSchedule"}]),
                    )))
                    .unwrap(),
            );
        });

        let desired_list = vec![desired("workload", Some("ours"), TaintEffect::NoSchedule)];
        let outcome = reconcile_node_taints(
            &client,
            ReconcileNodeTaintsInput {
                node_name: "worker-1",
                desired: &desired_list,
                previously_applied: &[],
            },
        )
        .await
        .expect("conflict must surface as outcome, not error");
        match outcome {
            NodeTaintReconcileOutcome::Conflict { conflicts } => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].key, "workload");
            }
            other => panic!("expected Conflict, got: {other:?}"),
        }
        srv.await.unwrap();
    }
}
