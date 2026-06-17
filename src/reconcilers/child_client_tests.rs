// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! Tests for the [`ChildClientCache`] resolver. Lock the contract for:
//! - Resolution order (explicit → auto-discover → management fallback)
//! - Cache hit (same RV) vs miss (changed RV) vs auto-discovery 404 fallthrough
//! - Error mapping for the three new `ReconcilerError` kubeconfig variants
//! - Bounded-LRU eviction at capacity
//!
//! Uses `tower_test::mock` to drive the management-cluster client; the
//! child client is constructed from a fixture kubeconfig YAML — kube-rs
//! parses it eagerly but the child Client itself only contacts its server
//! when something downstream actually USES the client. The tests never
//! exercise the child Client, so they remain hermetic.

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::crd::{
        EmbeddedResource, KubeconfigSecretRef, ScheduledMachine, ScheduledMachineSpec,
        SpotScheduleRef,
    };
    use http::{Request, Response};
    use kube::api::ObjectMeta;
    use kube::client::Body;
    use std::pin::pin;
    use std::sync::Arc;
    use tower_test::mock;

    // ========================================================================
    // Fixtures
    // ========================================================================

    /// Minimal valid kubeconfig that `kube::Config::from_custom_kubeconfig`
    /// accepts. `insecure-skip-tls-verify` keeps the parse hermetic — no
    /// CA bundle parsing, no DNS resolution.
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

    fn mock_client_pair() -> (kube::Client, mock::Handle<Request<Body>, Response<Body>>) {
        let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
        (kube::Client::new(svc, "default"), handle)
    }

    fn make_sm(
        namespace: &str,
        name: &str,
        cluster_name: &str,
        ref_: Option<KubeconfigSecretRef>,
    ) -> Arc<ScheduledMachine> {
        let sm = ScheduledMachine {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            spec: ScheduledMachineSpec {
                schedule: SpotScheduleRef {
                    api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
                    kind: "TimeBasedSpotSchedule".to_string(),
                    name: "weekdays-9-5".to_string(),
                },
                enabled: true,
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
        };
        Arc::new(sm)
    }

    /// JSON body for a Secret GET response containing the fixture kubeconfig.
    fn secret_response_body(
        namespace: &str,
        name: &str,
        key: &str,
        resource_version: &str,
        kubeconfig_yaml: &str,
    ) -> Vec<u8> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(kubeconfig_yaml.as_bytes());
        serde_json::to_vec(&serde_json::json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {
                "name": name,
                "namespace": namespace,
                "resourceVersion": resource_version,
            },
            "type": "Opaque",
            "data": { key: b64 }
        }))
        .unwrap()
    }

    fn k8s_404_body(name: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "kind": "Status",
            "apiVersion": "v1",
            "status": "Failure",
            "message": format!("secrets \"{name}\" not found"),
            "reason": "NotFound",
            "code": 404
        }))
        .unwrap()
    }

    fn k8s_403_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "kind": "Status",
            "apiVersion": "v1",
            "status": "Failure",
            "message": "forbidden",
            "reason": "Forbidden",
            "code": 403
        }))
        .unwrap()
    }

    fn respond_with(status: u16, body: Vec<u8>) -> Response<Body> {
        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap()
    }

    // ========================================================================
    // Resolution order: management fallback when neither ref nor auto-Secret
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_falls_back_to_management_when_auto_discovery_404s() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm("team-a", "sm1", "alpha", None);

        // Server: a single Secret GET that 404s.
        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected one Secret GET");
            assert!(
                req.uri()
                    .path()
                    .ends_with("/api/v1/namespaces/team-a/secrets/alpha-kubeconfig"),
                "expected auto-discovery GET, got: {}",
                req.uri()
            );
            send.send_response(respond_with(404, k8s_404_body("alpha-kubeconfig")));
        });

        let resolved = cache
            .resolve(&mgmt, &sm)
            .await
            .expect("auto-discovery 404 must fall through to management");
        assert!(
            !resolved.is_child(),
            "expected Management variant on auto-discovery 404"
        );
        server.await.unwrap();
        assert_eq!(cache.len(), 0, "no entry should be cached after 404");
    }

    // ========================================================================
    // Resolution: explicit ref takes precedence and builds a child client
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_uses_explicit_ref_when_present() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm(
            "team-a",
            "sm1",
            "alpha",
            Some(KubeconfigSecretRef {
                name: "custom-kubeconfig".to_string(),
                key: "value".to_string(),
            }),
        );

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected one Secret GET");
            assert!(
                req.uri()
                    .path()
                    .ends_with("/api/v1/namespaces/team-a/secrets/custom-kubeconfig"),
                "expected explicit-ref GET to custom-kubeconfig, got: {}",
                req.uri()
            );
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "custom-kubeconfig",
                    "value",
                    "42",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
        });

        let resolved = cache
            .resolve(&mgmt, &sm)
            .await
            .expect("explicit ref must resolve");
        assert!(resolved.is_child(), "expected Child variant");
        if let ResolvedClient::Child {
            key,
            resource_version,
            ..
        } = resolved
        {
            assert_eq!(key, CacheKey::new("team-a", "custom-kubeconfig"));
            assert_eq!(resource_version, "42");
        }
        server.await.unwrap();
        assert_eq!(cache.len(), 1);
    }

    // ========================================================================
    // Explicit ref takes priority over auto-discovery
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_prefers_explicit_ref_over_auto_discovery() {
        // With both an explicit ref AND an `<clusterName>-kubeconfig` Secret
        // available, the resolver MUST GET the explicit name only — not the
        // auto-discovery name. This is asserted by the mock seeing exactly
        // one request, and it's the explicit one.
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm(
            "team-a",
            "sm1",
            "alpha",
            Some(KubeconfigSecretRef {
                name: "override".to_string(),
                key: "value".to_string(),
            }),
        );

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected one Secret GET");
            let path = req.uri().path().to_string();
            assert!(
                path.ends_with("/api/v1/namespaces/team-a/secrets/override"),
                "expected explicit `override` Secret GET, got: {path}"
            );
            assert!(
                !path.contains("alpha-kubeconfig"),
                "explicit ref MUST suppress auto-discovery GET: {path}"
            );
            send.send_response(respond_with(
                200,
                secret_response_body("team-a", "override", "value", "1", FIXTURE_KUBECONFIG_YAML),
            ));
        });

        let _ = cache.resolve(&mgmt, &sm).await.expect("must resolve");
        server.await.unwrap();
    }

    // ========================================================================
    // Auto-discovery: <clusterName>-kubeconfig Secret found → child client
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_auto_discovers_capi_naming_when_secret_exists() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm("team-a", "sm1", "alpha", None);

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (req, send) = h.next_request().await.expect("expected Secret GET");
            assert!(req
                .uri()
                .path()
                .ends_with("/api/v1/namespaces/team-a/secrets/alpha-kubeconfig"));
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "7",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
        });

        let resolved = cache.resolve(&mgmt, &sm).await.expect("must resolve");
        assert!(
            resolved.is_child(),
            "auto-discovered Secret → Child variant"
        );
        server.await.unwrap();
    }

    // ========================================================================
    // Cache: hit on same RV returns cached client (still GETs Secret to check)
    // ========================================================================

    #[tokio::test]
    async fn test_cache_hit_on_same_resource_version_does_not_rebuild() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm("team-a", "sm1", "alpha", None);

        // Two resolves: server responds with the same RV both times.
        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            for _ in 0..2 {
                let (_req, send) = h.next_request().await.expect("expected Secret GET");
                send.send_response(respond_with(
                    200,
                    secret_response_body(
                        "team-a",
                        "alpha-kubeconfig",
                        "value",
                        "100",
                        FIXTURE_KUBECONFIG_YAML,
                    ),
                ));
            }
        });

        let first = cache.resolve(&mgmt, &sm).await.expect("first resolve");
        let second = cache.resolve(&mgmt, &sm).await.expect("second resolve");
        server.await.unwrap();

        let rv_first = match first {
            ResolvedClient::Child {
                resource_version, ..
            } => resource_version,
            _ => panic!("expected child"),
        };
        let rv_second = match second {
            ResolvedClient::Child {
                resource_version, ..
            } => resource_version,
            _ => panic!("expected child"),
        };
        assert_eq!(rv_first, "100");
        assert_eq!(rv_second, "100");
        assert_eq!(cache.len(), 1, "cache must hold exactly one entry");
    }

    // ========================================================================
    // Cache: RV change forces a rebuild
    // ========================================================================

    #[tokio::test]
    async fn test_cache_rebuilds_when_resource_version_changes() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm("team-a", "sm1", "alpha", None);

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            // First resolve: RV "1"
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "1",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
            // Second resolve: RV "2" — rotation event
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "2",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
        });

        let r1 = cache.resolve(&mgmt, &sm).await.unwrap();
        let r2 = cache.resolve(&mgmt, &sm).await.unwrap();
        server.await.unwrap();

        let rv1 = match r1 {
            ResolvedClient::Child {
                resource_version, ..
            } => resource_version,
            _ => panic!(),
        };
        let rv2 = match r2 {
            ResolvedClient::Child {
                resource_version, ..
            } => resource_version,
            _ => panic!(),
        };
        assert_eq!(rv1, "1");
        assert_eq!(
            rv2, "2",
            "RV change must yield the new RV on subsequent resolve"
        );
        assert_eq!(cache.len(), 1, "cache stays at one entry (key unchanged)");
    }

    // ========================================================================
    // Error mapping: explicit ref + 404 → ChildClusterUnreachable (fail-closed)
    // ========================================================================

    #[tokio::test]
    async fn test_explicit_ref_404_returns_child_cluster_unreachable() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm(
            "team-a",
            "sm1",
            "alpha",
            Some(KubeconfigSecretRef {
                name: "does-not-exist".to_string(),
                key: "value".to_string(),
            }),
        );

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(404, k8s_404_body("does-not-exist")));
        });

        let err = cache
            .resolve(&mgmt, &sm)
            .await
            .expect_err("explicit ref + 404 must error, not fall back");
        assert!(
            matches!(err, ReconcilerError::ChildClusterUnreachable { .. }),
            "expected ChildClusterUnreachable, got: {err:?}"
        );
        server.await.unwrap();
    }

    // ========================================================================
    // Error mapping: missing data key → KubeconfigSecretMissingKey
    // ========================================================================

    #[tokio::test]
    async fn test_missing_data_key_returns_kubeconfig_secret_missing_key() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm(
            "team-a",
            "sm1",
            "alpha",
            Some(KubeconfigSecretRef {
                name: "kc".to_string(),
                key: "missing-key".to_string(),
            }),
        );

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            // Secret exists but only has `value`, not `missing-key`.
            send.send_response(respond_with(
                200,
                secret_response_body("team-a", "kc", "value", "1", FIXTURE_KUBECONFIG_YAML),
            ));
        });

        let err = cache.resolve(&mgmt, &sm).await.expect_err("must error");
        assert!(
            matches!(
                err,
                ReconcilerError::KubeconfigSecretMissingKey { ref key, .. } if key == "missing-key"
            ),
            "expected KubeconfigSecretMissingKey, got: {err:?}"
        );
        server.await.unwrap();
    }

    // ========================================================================
    // Error mapping: invalid YAML → KubeconfigInvalid
    // ========================================================================

    #[tokio::test]
    async fn test_invalid_kubeconfig_yaml_returns_kubeconfig_invalid() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm(
            "team-a",
            "sm1",
            "alpha",
            Some(KubeconfigSecretRef {
                name: "kc".to_string(),
                key: "value".to_string(),
            }),
        );

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            // Garbage YAML — clearly not a kubeconfig.
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "kc",
                    "value",
                    "1",
                    "this is\n  : not [valid: yaml: :",
                ),
            ));
        });

        let err = cache.resolve(&mgmt, &sm).await.expect_err("must error");
        assert!(
            matches!(err, ReconcilerError::KubeconfigInvalid { .. }),
            "expected KubeconfigInvalid, got: {err:?}"
        );
        server.await.unwrap();
    }

    // ========================================================================
    // Error mapping: non-404 auto-discovery error propagates
    // ========================================================================

    #[tokio::test]
    async fn test_auto_discovery_403_propagates_as_kube_error() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let sm = make_sm("team-a", "sm1", "alpha", None);

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(403, k8s_403_body()));
        });

        let err = cache
            .resolve(&mgmt, &sm)
            .await
            .expect_err("403 must propagate, not silently fall back");
        // Non-404 auto-discovery is wrapped as KubeError.
        assert!(
            matches!(err, ReconcilerError::KubeError(_)),
            "expected KubeError, got: {err:?}"
        );
        server.await.unwrap();
        assert_eq!(cache.len(), 0);
    }

    // ========================================================================
    // CacheKey / ResolvedClient helpers
    // ========================================================================

    #[test]
    fn test_cache_key_equality() {
        let a = CacheKey::new("ns", "name");
        let b = CacheKey::new("ns", "name");
        let c = CacheKey::new("ns", "other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_default_kubeconfig_secret_key_constant() {
        assert_eq!(
            DEFAULT_KUBECONFIG_SECRET_KEY, "value",
            "default key must be `value` per CAPI convention; flips here will break auto-discovery"
        );
    }

    // ========================================================================
    // ChildWatchHook plumbing — verifies the cache fires lifecycle callbacks
    // in the right order. The real hook (ChildNodeWatchManager) starts and
    // cancels per-child Node watchers in lock-step with these events.
    // ========================================================================

    /// Test double for `ChildWatchHook` — records every call so tests can
    /// assert exact lifecycle sequences.
    #[derive(Default)]
    struct RecordingHook {
        events: std::sync::Mutex<Vec<HookEvent>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum HookEvent {
        Resolved(CacheKey),
        Evicted(CacheKey),
    }

    impl super::super::ChildWatchHook for RecordingHook {
        fn on_child_resolved(&self, key: &CacheKey, _client: kube::Client) {
            self.events
                .lock()
                .unwrap()
                .push(HookEvent::Resolved(key.clone()));
        }
        fn on_child_evicted(&self, key: &CacheKey) {
            self.events
                .lock()
                .unwrap()
                .push(HookEvent::Evicted(key.clone()));
        }
    }

    impl RecordingHook {
        fn events(&self) -> Vec<HookEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn test_hook_fires_resolved_once_on_initial_build() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let hook = Arc::new(RecordingHook::default());
        cache.set_hook(hook.clone());

        let sm = make_sm("team-a", "sm1", "alpha", None);
        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "1",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
        });
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        server.await.unwrap();
        assert_eq!(
            hook.events(),
            vec![HookEvent::Resolved(CacheKey::new(
                "team-a",
                "alpha-kubeconfig"
            ))]
        );
    }

    #[tokio::test]
    async fn test_hook_does_not_fire_on_cache_hit_same_rv() {
        // Same RV both times — the cache short-circuits without rebuilding
        // and MUST NOT fire on_child_resolved a second time (it would
        // re-start a watcher that is already running).
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let hook = Arc::new(RecordingHook::default());
        cache.set_hook(hook.clone());
        let sm = make_sm("team-a", "sm1", "alpha", None);
        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            for _ in 0..2 {
                let (_req, send) = h.next_request().await.unwrap();
                send.send_response(respond_with(
                    200,
                    secret_response_body(
                        "team-a",
                        "alpha-kubeconfig",
                        "value",
                        "1",
                        FIXTURE_KUBECONFIG_YAML,
                    ),
                ));
            }
        });
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        server.await.unwrap();
        assert_eq!(
            hook.events(),
            vec![HookEvent::Resolved(CacheKey::new(
                "team-a",
                "alpha-kubeconfig"
            ))],
            "cache hits on identical RV must not re-fire on_child_resolved"
        );
    }

    #[tokio::test]
    async fn test_hook_fires_evicted_then_resolved_on_rv_change() {
        // Token / cert rotation: Secret RV bumps. The watcher running
        // with stale credentials must be cancelled before a fresh one
        // is started with the rebuilt Client — assert the exact event
        // ordering.
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let hook = Arc::new(RecordingHook::default());
        cache.set_hook(hook.clone());
        let sm = make_sm("team-a", "sm1", "alpha", None);

        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            // First resolve: RV "1"
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "1",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
            // Second: RV "2"
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "2",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
        });
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        server.await.unwrap();

        let key = CacheKey::new("team-a", "alpha-kubeconfig");
        assert_eq!(
            hook.events(),
            vec![
                HookEvent::Resolved(key.clone()),
                HookEvent::Evicted(key.clone()),
                HookEvent::Resolved(key),
            ]
        );
    }

    #[tokio::test]
    async fn test_explicit_evict_fires_hook_when_entry_present() {
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let hook = Arc::new(RecordingHook::default());
        cache.set_hook(hook.clone());
        let sm = make_sm("team-a", "sm1", "alpha", None);
        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(
                200,
                secret_response_body(
                    "team-a",
                    "alpha-kubeconfig",
                    "value",
                    "1",
                    FIXTURE_KUBECONFIG_YAML,
                ),
            ));
        });
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        server.await.unwrap();
        let key = CacheKey::new("team-a", "alpha-kubeconfig");
        cache.evict(&key);
        cache.evict(&key); // second evict on missing key — must not re-fire
        assert_eq!(
            hook.events(),
            vec![HookEvent::Resolved(key.clone()), HookEvent::Evicted(key),]
        );
    }

    #[tokio::test]
    async fn test_hook_does_not_fire_for_management_fallback() {
        // Auto-discovery 404 path — no child client was built, hook stays
        // silent. (Watchers are only meaningful when there's a child
        // Client to watch with.)
        let (mgmt, handle) = mock_client_pair();
        let cache = ChildClientCache::new();
        let hook = Arc::new(RecordingHook::default());
        cache.set_hook(hook.clone());
        let sm = make_sm("team-a", "sm1", "alpha", None);
        let server = tokio::spawn(async move {
            let mut h = pin!(handle);
            let (_req, send) = h.next_request().await.unwrap();
            send.send_response(respond_with(404, k8s_404_body("alpha-kubeconfig")));
        });
        let _ = cache.resolve(&mgmt, &sm).await.unwrap();
        server.await.unwrap();
        assert!(
            hook.events().is_empty(),
            "management fallback must not invoke the watch hook: {:?}",
            hook.events()
        );
    }
}
