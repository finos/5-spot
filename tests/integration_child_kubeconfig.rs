// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Integration test: child-cluster kubeconfig wiring
//!
//! End-to-end exercise of the Phase 1 multi-cluster work — the resolver,
//! the cache, the watcher manager, and the `Context` plumbing — through
//! their public interfaces. Validates that the four moving parts (CRD
//! field → cache resolve → hook fired → manager spawns task) compose
//! correctly without requiring a live Kubernetes cluster.
//!
//! Why hermetic, not a real `kind` cluster: the per-cluster connection
//! semantics are already exercised by the existing real-cluster
//! integration tests (`integration_node_taints.rs`,
//! `integration_emergency_reclaim.rs`); what this test adds is the
//! *integration of the new modules with each other and with Context*,
//! which a unit test inside the modules can't validate. `tower_test`
//! gives us deterministic control over Secret GET ordering + RV bumps
//! without paying the kind-cluster startup cost.
//!
//! A complementary real-cluster integration test (full Active →
//! ShuttingDown cycle with two real clusters) is left as future work
//! once the dev environment standardises on a two-cluster harness.

use std::pin::pin;
use std::sync::Arc;

use http::{Request, Response};
use kube::api::ObjectMeta;
use kube::client::Body;
use kube::runtime::reflector;
use kube::runtime::reflector::ObjectRef;
use kube::{api::ApiResource, api::GroupVersionKind, core::DynamicObject};
use tokio::sync::mpsc;
use tower_test::mock;

use five_spot::constants::CHILD_NODE_EVENT_CHANNEL_CAP;
use five_spot::crd::{
    EmbeddedResource, KubeconfigSecretRef, ScheduleSpec, ScheduledMachine, ScheduledMachineSpec,
};
use five_spot::reconcilers::{
    CacheKey, ChildClientCache, ChildNodeWatchManager, Context, ResolvedClient,
};

// Minimal valid kubeconfig. `insecure-skip-tls-verify` keeps the parse
// hermetic — no CA bundle or DNS lookup.
const FIXTURE_KUBECONFIG_YAML: &str = r#"
apiVersion: v1
kind: Config
clusters:
- name: child
  cluster:
    server: https://child.example.test
    insecure-skip-tls-verify: true
contexts:
- name: child
  context:
    cluster: child
    user: child
users:
- name: child
  user:
    token: dummy-token
current-context: child
"#;

fn mock_pair() -> (kube::Client, mock::Handle<Request<Body>, Response<Body>>) {
    let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
    (kube::Client::new(svc, "default"), handle)
}

fn empty_machine_store() -> reflector::Store<DynamicObject> {
    let ar = ApiResource::from_gvk_with_plural(
        &GroupVersionKind::gvk("cluster.x-k8s.io", "v1beta1", "Machine"),
        "machines",
    );
    let writer: reflector::store::Writer<DynamicObject> = reflector::store::Writer::new(ar);
    writer.as_reader()
}

fn make_sm(
    namespace: &str,
    name: &str,
    cluster_name: &str,
    ref_: Option<KubeconfigSecretRef>,
) -> Arc<ScheduledMachine> {
    Arc::new(ScheduledMachine {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: ScheduledMachineSpec {
            schedule: ScheduleSpec {
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-17".to_string()],
                timezone: "UTC".to_string(),
                enabled: true,
            },
            cluster_name: cluster_name.to_string(),
            bootstrap_spec: EmbeddedResource(serde_json::json!({
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            })),
            infrastructure_spec: EmbeddedResource(serde_json::json!({
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {}
            })),
            machine_template: None,
            priority: 50,
            graceful_shutdown_timeout: "5m".to_string(),
            node_drain_timeout: "5m".to_string(),
            kill_switch: false,
            node_taints: vec![],
            kill_if_commands: None,
            kubeconfig_secret_ref: ref_,
            kata: None,
        },
        status: None,
    })
}

fn secret_response(
    namespace: &str,
    name: &str,
    key: &str,
    resource_version: &str,
    yaml: &str,
) -> Response<Body> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(yaml.as_bytes());
    let body = serde_json::to_vec(&serde_json::json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "resourceVersion": resource_version
        },
        "type": "Opaque",
        "data": { key: b64 }
    }))
    .unwrap();
    Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn not_found_response(name: &str) -> Response<Body> {
    let body = serde_json::to_vec(&serde_json::json!({
        "kind": "Status",
        "apiVersion": "v1",
        "status": "Failure",
        "message": format!("secrets \"{name}\" not found"),
        "reason": "NotFound",
        "code": 404
    }))
    .unwrap();
    Response::builder()
        .status(404)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Wire a full Context with cache + watch manager, exactly as main.rs
/// does, and exercise the integration through the `resolve()` entry
/// point. Asserts that:
/// - The cache GETs the right Secret
/// - A child client is built and returned
/// - The watcher manager spawns exactly one task for the resulting key
/// - The mpsc receiver is wired (we don't drain it; we only verify the
///   channel pair exists and the manager holds the sender end)
#[tokio::test]
async fn explicit_ref_resolves_and_starts_watcher() {
    let (mgmt, handle) = mock_pair();

    // Build Context exactly like main.rs: with the same Arc-cache and
    // watch manager wiring.
    let context = Arc::new(Context::new(mgmt.clone(), 0, 1));
    let (tx, _rx) = mpsc::channel::<ObjectRef<ScheduledMachine>>(CHILD_NODE_EVENT_CHANNEL_CAP);
    let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
    let manager_for_assert = manager.clone();
    context.child_clients.set_hook(Arc::new(manager));

    let sm = make_sm(
        "team-a",
        "gpu-worker",
        "alpha",
        Some(KubeconfigSecretRef {
            name: "custom-kubeconfig".to_string(),
            key: "value".to_string(),
        }),
    );

    let server = tokio::spawn(async move {
        let mut h = pin!(handle);
        let (req, send) = h.next_request().await.expect("expected Secret GET");
        assert!(
            req.uri()
                .path()
                .ends_with("/api/v1/namespaces/team-a/secrets/custom-kubeconfig"),
            "expected GET on the explicitly referenced Secret, got {}",
            req.uri()
        );
        send.send_response(secret_response(
            "team-a",
            "custom-kubeconfig",
            "value",
            "1",
            FIXTURE_KUBECONFIG_YAML,
        ));
    });

    let resolved = context
        .child_clients
        .resolve(&context.client, &sm)
        .await
        .expect("resolve must succeed for a valid explicit ref");
    server.await.unwrap();

    assert!(
        resolved.is_child(),
        "explicit ref must yield a Child client, got {resolved:?}"
    );
    let key = CacheKey::new("team-a", "custom-kubeconfig");
    assert!(
        manager_for_assert.has_watcher(&key),
        "watch manager must have spawned a watcher for the resolved CacheKey"
    );
    assert_eq!(manager_for_assert.task_count(), 1);
}

#[tokio::test]
async fn auto_discovery_falls_back_to_management_with_no_watcher() {
    let (mgmt, handle) = mock_pair();
    let context = Arc::new(Context::new(mgmt.clone(), 0, 1));
    let (tx, _rx) = mpsc::channel::<ObjectRef<ScheduledMachine>>(CHILD_NODE_EVENT_CHANNEL_CAP);
    let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
    let manager_for_assert = manager.clone();
    context.child_clients.set_hook(Arc::new(manager));

    let sm = make_sm("team-a", "gpu-worker", "alpha", None);

    let server = tokio::spawn(async move {
        let mut h = pin!(handle);
        let (req, send) = h.next_request().await.unwrap();
        // Auto-discovery: alpha → alpha-kubeconfig
        assert!(req
            .uri()
            .path()
            .ends_with("/api/v1/namespaces/team-a/secrets/alpha-kubeconfig"));
        send.send_response(not_found_response("alpha-kubeconfig"));
    });

    let resolved = context
        .child_clients
        .resolve(&context.client, &sm)
        .await
        .expect("auto-discovery 404 must fall through to management");
    server.await.unwrap();

    assert!(
        !resolved.is_child(),
        "no Secret → Management variant, got {resolved:?}"
    );
    assert_eq!(
        manager_for_assert.task_count(),
        0,
        "management fallback must NOT spawn a watcher"
    );
}

#[tokio::test]
async fn rv_rotation_restarts_watcher_cleanly() {
    // Token rotation: same Secret, new resourceVersion. The watcher
    // running with stale credentials must be cancelled and a fresh one
    // started before the resolve returns.
    let (mgmt, handle) = mock_pair();
    let context = Arc::new(Context::new(mgmt.clone(), 0, 1));
    let (tx, _rx) = mpsc::channel::<ObjectRef<ScheduledMachine>>(CHILD_NODE_EVENT_CHANNEL_CAP);
    let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
    let manager_for_assert = manager.clone();
    context.child_clients.set_hook(Arc::new(manager));

    let sm = make_sm("team-a", "gpu-worker", "alpha", None);

    let server = tokio::spawn(async move {
        let mut h = pin!(handle);
        // First resolve: RV "1"
        let (_, send) = h.next_request().await.unwrap();
        send.send_response(secret_response(
            "team-a",
            "alpha-kubeconfig",
            "value",
            "1",
            FIXTURE_KUBECONFIG_YAML,
        ));
        // Second resolve: RV "2" (rotation)
        let (_, send) = h.next_request().await.unwrap();
        send.send_response(secret_response(
            "team-a",
            "alpha-kubeconfig",
            "value",
            "2",
            FIXTURE_KUBECONFIG_YAML,
        ));
    });

    let _ = context
        .child_clients
        .resolve(&context.client, &sm)
        .await
        .unwrap();
    assert_eq!(manager_for_assert.task_count(), 1);

    let _ = context
        .child_clients
        .resolve(&context.client, &sm)
        .await
        .unwrap();
    server.await.unwrap();
    // Still exactly one watcher — the old one was aborted, a new one started.
    assert_eq!(
        manager_for_assert.task_count(),
        1,
        "RV rotation must keep exactly one running watcher per CacheKey"
    );
}

#[tokio::test]
async fn cache_evict_cancels_watcher() {
    let (mgmt, handle) = mock_pair();
    let cache = ChildClientCache::new();
    let (tx, _rx) = mpsc::channel::<ObjectRef<ScheduledMachine>>(CHILD_NODE_EVENT_CHANNEL_CAP);
    let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
    let manager_for_assert = manager.clone();
    cache.set_hook(Arc::new(manager));

    let sm = make_sm(
        "team-a",
        "gpu-worker",
        "alpha",
        Some(KubeconfigSecretRef {
            name: "kc".to_string(),
            key: "value".to_string(),
        }),
    );

    let server = tokio::spawn(async move {
        let mut h = pin!(handle);
        let (_, send) = h.next_request().await.unwrap();
        send.send_response(secret_response(
            "team-a",
            "kc",
            "value",
            "1",
            FIXTURE_KUBECONFIG_YAML,
        ));
    });

    let _ = cache.resolve(&mgmt, &sm).await.unwrap();
    server.await.unwrap();
    assert_eq!(manager_for_assert.task_count(), 1);

    cache.evict(&CacheKey::new("team-a", "kc"));
    assert_eq!(
        manager_for_assert.task_count(),
        0,
        "explicit cache evict must propagate to the watch manager"
    );
}

#[tokio::test]
async fn explicit_ref_404_fails_closed_and_does_not_spawn_watcher() {
    // Fail-closed contract: misconfigured kubeconfigSecretRef must NOT
    // silently route Node operations to the management cluster. The
    // resolver returns an error AND no watcher gets spawned for the
    // non-existent Secret.
    let (mgmt, handle) = mock_pair();
    let cache = ChildClientCache::new();
    let (tx, _rx) = mpsc::channel::<ObjectRef<ScheduledMachine>>(CHILD_NODE_EVENT_CHANNEL_CAP);
    let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
    let manager_for_assert = manager.clone();
    cache.set_hook(Arc::new(manager));

    let sm = make_sm(
        "team-a",
        "gpu-worker",
        "alpha",
        Some(KubeconfigSecretRef {
            name: "missing".to_string(),
            key: "value".to_string(),
        }),
    );

    let server = tokio::spawn(async move {
        let mut h = pin!(handle);
        let (_, send) = h.next_request().await.unwrap();
        send.send_response(not_found_response("missing"));
    });

    let result = cache.resolve(&mgmt, &sm).await;
    server.await.unwrap();

    assert!(
        result.is_err(),
        "explicit ref + 404 must error, got {:?}",
        result
            .ok()
            .map(|r| matches!(r, ResolvedClient::Management(_)))
    );
    assert_eq!(manager_for_assert.task_count(), 0);
}
