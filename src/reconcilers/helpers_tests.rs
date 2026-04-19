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
}
