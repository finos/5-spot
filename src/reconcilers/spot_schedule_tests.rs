// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants;
    use crate::crd::SpotScheduleRef;
    use serde_json::json;

    // ========================================================================
    // verdict_from_status — pure duck-typed status extraction (ADR 0006)
    // ========================================================================

    #[test]
    fn test_verdict_active_when_status_active_true() {
        let status = json!({ "active": true, "observedGeneration": 7 });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict,
            SpotScheduleVerdict::Active {
                provider_generation: Some(7)
            }
        );
        assert!(verdict.is_resolved());
        assert_eq!(verdict.active(), Some(true));
        assert_eq!(verdict.provider_generation(), Some(7));
        assert_eq!(verdict.reason(), constants::REASON_SPOT_SCHEDULE_RESOLVED);
    }

    #[test]
    fn test_verdict_inactive_when_status_active_false() {
        let status = json!({ "active": false });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict,
            SpotScheduleVerdict::Inactive {
                provider_generation: None
            }
        );
        assert_eq!(verdict.active(), Some(false));
    }

    #[test]
    fn test_verdict_unresolved_when_status_absent() {
        let verdict = verdict_from_status(None);
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_STATUS_ACTIVE_MISSING
        );
        assert!(!verdict.is_resolved());
        assert_eq!(verdict.active(), None);
    }

    #[test]
    fn test_verdict_unresolved_when_active_missing() {
        let status = json!({ "observedGeneration": 1 });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_STATUS_ACTIVE_MISSING
        );
    }

    #[test]
    fn test_verdict_unresolved_when_active_not_boolean() {
        let status = json!({ "active": "yes" });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_STATUS_ACTIVE_MISSING
        );
    }

    #[test]
    fn test_verdict_ready_true_is_resolved() {
        let status = json!({
            "active": true,
            "conditions": [{ "type": "Ready", "status": "True" }]
        });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(verdict.active(), Some(true));
        assert!(verdict.is_resolved());
    }

    #[test]
    fn test_verdict_ready_false_is_unresolved_even_when_active() {
        // ADR 0006 §4: Ready=False ⇒ unresolved, NOT inactive.
        let status = json!({
            "active": true,
            "conditions": [{ "type": "Ready", "status": "False" }]
        });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_READY
        );
        assert!(!verdict.is_resolved());
    }

    #[test]
    fn test_verdict_ready_unknown_is_unresolved() {
        let status = json!({
            "active": false,
            "conditions": [{ "type": "Ready", "status": "Unknown" }]
        });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_READY
        );
    }

    #[test]
    fn test_verdict_ready_absent_trusts_active() {
        // Ready is recommended, not required: absent Ready ⇒ active authoritative.
        let status = json!({
            "active": true,
            "conditions": [{ "type": "Synced", "status": "True" }]
        });
        let verdict = verdict_from_status(Some(&status));
        assert_eq!(
            verdict,
            SpotScheduleVerdict::Active {
                provider_generation: None
            }
        );
    }

    #[test]
    fn test_verdict_empty_conditions_trusts_active() {
        let status = json!({ "active": true, "conditions": [] });
        assert!(verdict_from_status(Some(&status)).is_resolved());
    }

    // ========================================================================
    // resolve_spot_schedule — async wrapper over discovery + GET
    // ========================================================================

    use http::{Request, Response};
    use kube::client::Body;
    use tower_test::mock;

    fn reference() -> SpotScheduleRef {
        SpotScheduleRef {
            api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
            kind: "CapitalMarketsSchedule".to_string(),
            name: "nyse-equities".to_string(),
        }
    }

    /// Body that `pinned_kind` discovery expects: an `APIResourceList` for the
    /// group/version naming the kind's plural and namespaced scope.
    fn discovery_body() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "kind": "APIResourceList",
            "apiVersion": "v1",
            "groupVersion": "spotschedules.5spot.finos.org/v1alpha1",
            "resources": [{
                "name": "capitalmarketsschedules",
                "singularName": "capitalmarketsschedule",
                "namespaced": true,
                "kind": "CapitalMarketsSchedule",
                "verbs": ["get", "list", "watch"]
            }]
        }))
        .unwrap()
    }

    fn provider_object_body(active: bool) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
            "kind": "CapitalMarketsSchedule",
            "metadata": { "name": "nyse-equities", "namespace": "capital-markets" },
            "status": {
                "active": active,
                "observedGeneration": 3,
                "conditions": [{ "type": "Ready", "status": "True" }]
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn test_resolve_crd_not_installed_is_unresolved() {
        let (svc, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(svc, "capital-markets");

        let server = tokio::spawn(async move {
            // Discovery GET fails (CRD/group not served) → 404.
            let (_req, send) = handle.next_request().await.expect("discovery request");
            send.send_response(
                Response::builder()
                    .status(404)
                    .body(Body::from(Vec::new()))
                    .unwrap(),
            );
        });

        let verdict = resolve_spot_schedule(&client, "capital-markets", &reference())
            .await
            .expect("unresolved is Ok, not Err");
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_PROVIDER_CRD_NOT_INSTALLED
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_resolve_object_not_found_is_unresolved() {
        let (svc, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(svc, "capital-markets");

        let server = tokio::spawn(async move {
            let (_disc, send) = handle.next_request().await.expect("discovery request");
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(Body::from(discovery_body()))
                    .unwrap(),
            );
            // Object GET → 404 with a parseable Status body so `get_opt`
            // converts it to `None` (CRD installed, object absent).
            let (_get, send) = handle.next_request().await.expect("object get");
            let not_found = serde_json::to_vec(&json!({
                "kind": "Status",
                "apiVersion": "v1",
                "status": "Failure",
                "code": 404,
                "reason": "NotFound",
                "message": "capitalmarketsschedules.spotschedules.5spot.finos.org \"nyse-equities\" not found"
            }))
            .unwrap();
            send.send_response(
                Response::builder()
                    .status(404)
                    .body(Body::from(not_found))
                    .unwrap(),
            );
        });

        let verdict = resolve_spot_schedule(&client, "capital-markets", &reference())
            .await
            .expect("unresolved is Ok");
        assert_eq!(
            verdict.reason(),
            constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_FOUND
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_resolve_active_object_resolves_active() {
        let (svc, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(svc, "capital-markets");

        let server = tokio::spawn(async move {
            let (_disc, send) = handle.next_request().await.expect("discovery request");
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(Body::from(discovery_body()))
                    .unwrap(),
            );
            let (_get, send) = handle.next_request().await.expect("object get");
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(Body::from(provider_object_body(true)))
                    .unwrap(),
            );
        });

        let verdict = resolve_spot_schedule(&client, "capital-markets", &reference())
            .await
            .expect("resolve ok");
        assert_eq!(
            verdict,
            SpotScheduleVerdict::Active {
                provider_generation: Some(3)
            }
        );
        server.await.unwrap();
    }
}
