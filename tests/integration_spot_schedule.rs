// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Integration test: spot-schedule provider contract (ADR 0006)
//!
//! Exercises the Phase 2 resolver and Phase 3 watch-index through their public
//! interfaces, hermetically (no live cluster) via `tower_test`:
//!
//! 1. **create → active**: a provider object reporting `status.active: true`
//!    (and `Ready`) resolves to `Active`, and composition yields
//!    should-be-active = `true`.
//! 2. **flip → inactive**: the same object flipped to `status.active: false`
//!    resolves to `Inactive`, composition yields `false`.
//! 3. **delete → unresolved + fallback**: a `404` on the object resolves to
//!    `Unresolved(ProviderNotFound)`; composition then *holds last state*
//!    (stays `true` if last-known was active) and *fails inactive* when never
//!    resolved.
//! 4. **watch index → reconcile mapping**: a provider event for `(gvk, ns,
//!    name)` maps back to exactly the `ScheduledMachine`s that reference it.
//!
//! The controller's `reconcile_on` wiring itself (manager → mpsc → Controller)
//! is validated structurally by the unit tests in `spot_schedule_watch_tests`;
//! a full Active→ShuttingDown cycle against a real provider controller is left
//! to a future two-resource cluster harness (the provider controller lands in
//! roadmap Phase 5).

use http::{Request, Response};
use kube::client::Body;
use kube::runtime::reflector::ObjectRef;
use serde_json::json;
use tower_test::mock;

use five_spot::constants;
use five_spot::crd::{EmbeddedResource, ScheduledMachine, ScheduledMachineSpec, SpotScheduleRef};
use five_spot::reconcilers::compose_should_be_active;
use five_spot::reconcilers::spot_schedule::{resolve_spot_schedule, SpotScheduleVerdict};
use five_spot::reconcilers::spot_schedule_watch::{provider_key_for, ProviderKey, ReverseIndex};

const NS: &str = "capital-markets";

fn reference() -> SpotScheduleRef {
    SpotScheduleRef {
        api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
        kind: "CapitalMarketsSchedule".to_string(),
        name: "nyse-equities".to_string(),
    }
}

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

fn provider_body(active: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
        "kind": "CapitalMarketsSchedule",
        "metadata": { "name": "nyse-equities", "namespace": NS },
        "status": {
            "active": active,
            "observedGeneration": 5,
            "conditions": [{ "type": "Ready", "status": "True" }]
        }
    }))
    .unwrap()
}

fn not_found_body() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "kind": "Status", "apiVersion": "v1", "status": "Failure",
        "code": 404, "reason": "NotFound", "message": "not found"
    }))
    .unwrap()
}

/// Drive one resolve against a mock that serves a discovery response then one
/// object response. Returns the verdict.
async fn resolve_with(object_status: u16, object_body: Vec<u8>) -> SpotScheduleVerdict {
    let (svc, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = kube::Client::new(svc, NS);

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
                .status(object_status)
                .body(Body::from(object_body))
                .unwrap(),
        );
    });

    let verdict = resolve_spot_schedule(&client, NS, &reference())
        .await
        .expect("resolve returns Ok (unresolved is a verdict, not an error)");
    server.await.unwrap();
    verdict
}

#[tokio::test]
async fn test_create_active_provider_makes_machine_active() {
    let verdict = resolve_with(200, provider_body(true)).await;
    assert_eq!(
        verdict,
        SpotScheduleVerdict::Active {
            provider_generation: Some(5)
        }
    );
    // spotSchedule-only machine (no inline schedule): provider decides.
    assert!(compose_should_be_active(None, Some(&verdict), None));
}

#[tokio::test]
async fn test_flip_to_inactive_tears_machine_down() {
    let verdict = resolve_with(200, provider_body(false)).await;
    assert_eq!(
        verdict,
        SpotScheduleVerdict::Inactive {
            provider_generation: Some(5)
        }
    );
    assert!(!compose_should_be_active(None, Some(&verdict), Some(true)));
}

#[tokio::test]
async fn test_delete_provider_is_unresolved_and_holds_last_state() {
    let verdict = resolve_with(404, not_found_body()).await;
    assert_eq!(
        verdict.reason(),
        constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_FOUND
    );

    // Hold-last-state: was active ⇒ stays active despite the provider vanishing.
    assert!(compose_should_be_active(None, Some(&verdict), Some(true)));
    // Never resolved (no last-known) ⇒ fail-inactive.
    assert!(!compose_should_be_active(None, Some(&verdict), None));
    // An inline schedule still closes the machine even while holding state.
    assert!(!compose_should_be_active(
        Some(false),
        Some(&verdict),
        Some(true)
    ));
}

#[test]
fn test_watch_index_maps_provider_event_to_referencing_machines() {
    // Two SMs reference the same provider object; a third references a different
    // one. A provider event for the shared object must map to exactly the first
    // two — the mapping that drives the Phase 3 event-driven reconcile.
    let sm_a = scheduled_machine("sm-a", "nyse-equities");
    let sm_b = scheduled_machine("sm-b", "nyse-equities");
    let sm_c = scheduled_machine("sm-c", "tsx-equities");

    let mut index = ReverseIndex::default();
    index.register(ObjectRef::from_obj(&sm_a), provider_key_for(&sm_a).unwrap());
    index.register(ObjectRef::from_obj(&sm_b), provider_key_for(&sm_b).unwrap());
    index.register(ObjectRef::from_obj(&sm_c), provider_key_for(&sm_c).unwrap());

    let shared = ProviderKey {
        gvk: kube::core::GroupVersionKind::gvk(
            "spotschedules.5spot.finos.org",
            "v1alpha1",
            "CapitalMarketsSchedule",
        ),
        namespace: NS.to_string(),
        name: "nyse-equities".to_string(),
    };

    let mut mapped: Vec<String> = index
        .lookup(&shared)
        .iter()
        .map(ToString::to_string)
        .collect();
    mapped.sort();
    let mut expected: Vec<String> = vec![
        ObjectRef::from_obj(&sm_a).to_string(),
        ObjectRef::from_obj(&sm_b).to_string(),
    ];
    expected.sort();
    assert_eq!(mapped, expected);
    // Two distinct provider objects ⇒ a single GVK watcher would cover both.
    assert_eq!(index.referenced_gvks().len(), 1);
}

fn scheduled_machine(name: &str, provider_name: &str) -> ScheduledMachine {
    use kube::api::ObjectMeta;
    ScheduledMachine {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(NS.to_string()),
            ..Default::default()
        },
        spec: ScheduledMachineSpec {
            schedule: None,
            spot_schedule: Some(SpotScheduleRef {
                api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
                kind: "CapitalMarketsSchedule".to_string(),
                name: provider_name.to_string(),
            }),
            cluster_name: "c".to_string(),
            bootstrap_spec: EmbeddedResource(json!({
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig", "spec": {}
            })),
            infrastructure_spec: EmbeddedResource(json!({
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine", "spec": {}
            })),
            machine_template: None,
            priority: 50,
            graceful_shutdown_timeout: "5m".to_string(),
            node_drain_timeout: "5m".to_string(),
            kill_switch: false,
            node_taints: vec![],
            kill_if_commands: None,
            kubeconfig_secret_ref: None,
            kata: None,
        },
        status: None,
    }
}
