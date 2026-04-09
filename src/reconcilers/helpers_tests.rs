// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
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
            "5spot.io/scheduled-machine".to_string(),
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
            "apiVersion": "5spot.io/v1alpha1",
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
                "apiVersion": "5spot.io/v1alpha1",
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
        update_phase(&ctx, "default", "test-sm", None, "Inactive", None, None)
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
}
