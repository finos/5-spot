// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! Tests for [`super::ChildNodeWatchManager`]. The actual `kube::runtime`
//! `watcher` stream is hard to drive deterministically from a unit test,
//! so we focus on what we own: the lifecycle invariants of the manager
//! itself (spawn-on-resolve, abort-on-evict, idempotent restart, mpsc
//! plumbing into `emit_refs_for_node`).
//!
//! The pure `node_to_scheduled_machines_via_machine` mapper is exercised
//! in `helpers_tests.rs`; the integration of that mapper with a live
//! Node watch stream is left to the Phase 1.10 dual-cluster integration
//! test under `tests/`.

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::reconcilers::child_client::{CacheKey, ChildWatchHook};
    use http::{Request, Response};
    use k8s_openapi::api::core::v1::Node;
    use kube::api::{ApiResource, GroupVersionKind, ObjectMeta};
    use kube::client::Body;
    use kube::core::DynamicObject;
    use kube::runtime::reflector;
    use kube::runtime::reflector::ObjectRef;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tower_test::mock;

    fn make_mock_client() -> kube::Client {
        // We never drive the mock — the test asserts manager state, not
        // watcher I/O. The kube::Client just needs to be constructible
        // so the manager can call Api::all on it.
        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        kube::Client::new(svc, "default")
    }

    fn empty_machine_store() -> reflector::Store<DynamicObject> {
        let ar = ApiResource::from_gvk_with_plural(
            &GroupVersionKind::gvk("cluster.x-k8s.io", "v1beta1", "Machine"),
            "machines",
        );
        let writer: reflector::store::Writer<DynamicObject> = reflector::store::Writer::new(ar);
        writer.as_reader()
    }

    fn machine_store_with(machines: Vec<DynamicObject>) -> reflector::Store<DynamicObject> {
        let ar = ApiResource::from_gvk_with_plural(
            &GroupVersionKind::gvk("cluster.x-k8s.io", "v1beta1", "Machine"),
            "machines",
        );
        let mut writer: reflector::store::Writer<DynamicObject> = reflector::store::Writer::new(ar);
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Init);
        for m in &machines {
            writer.apply_watcher_event(&kube::runtime::watcher::Event::InitApply(m.clone()));
        }
        writer.apply_watcher_event(&kube::runtime::watcher::Event::InitDone);
        writer.as_reader()
    }

    /// Synthesise a CAPI Machine whose `status.nodeRef.name` points at
    /// `node_name` and whose `metadata.labels[LABEL_SCHEDULED_MACHINE]`
    /// is `sm_name` in `namespace`. Matches the shape the mapper
    /// `node_to_scheduled_machines_via_machine` expects.
    fn machine_for_sm(namespace: &str, sm_name: &str, node_name: &str) -> DynamicObject {
        let ar = ApiResource::from_gvk_with_plural(
            &GroupVersionKind::gvk("cluster.x-k8s.io", "v1beta1", "Machine"),
            "machines",
        );
        let mut labels = BTreeMap::new();
        labels.insert(
            crate::labels::LABEL_SCHEDULED_MACHINE.to_string(),
            sm_name.to_string(),
        );
        DynamicObject {
            types: Some(kube::api::TypeMeta {
                api_version: "cluster.x-k8s.io/v1beta1".to_string(),
                kind: "Machine".to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(format!("{sm_name}-machine")),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            data: serde_json::json!({
                "status": { "nodeRef": { "name": node_name } }
            }),
        }
        // `ar` is unused in the field set but documents intent; the
        // mapper reads from `metadata` + `data` only.
        .tap(|_| {
            let _ = ar;
        })
    }

    /// Tiny no-op `Tap` helper so we can keep the trailing `let _ = ar;`
    /// readable. Avoids pulling in `tap` as a dep.
    trait Tap: Sized {
        fn tap<F: FnOnce(&Self)>(self, f: F) -> Self {
            f(&self);
            self
        }
    }
    impl<T> Tap for T {}

    // ========================================================================
    // Lifecycle: spawn on resolve, abort on evict
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_spawns_watcher_task() {
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
        let key = CacheKey::new("team-a", "alpha-kubeconfig");

        assert_eq!(manager.task_count(), 0);
        manager.on_child_resolved(&key, make_mock_client());
        assert!(manager.has_watcher(&key));
        assert_eq!(manager.task_count(), 1);
    }

    #[tokio::test]
    async fn test_evict_aborts_watcher_task() {
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
        let key = CacheKey::new("team-a", "alpha-kubeconfig");

        manager.on_child_resolved(&key, make_mock_client());
        assert_eq!(manager.task_count(), 1);

        manager.on_child_evicted(&key);
        assert!(!manager.has_watcher(&key));
        assert_eq!(manager.task_count(), 0);
    }

    #[tokio::test]
    async fn test_evict_of_unknown_key_is_noop() {
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
        // Evicting before any resolve must not panic and must leave the
        // task map empty.
        manager.on_child_evicted(&CacheKey::new("nope", "missing"));
        assert_eq!(manager.task_count(), 0);
    }

    #[tokio::test]
    async fn test_resolve_then_evict_then_resolve_restarts_cleanly() {
        // Token rotation simulation: resolve, evict (RV change), resolve
        // again. Final state must have exactly one task running for the
        // key — no zombie tasks left behind by the first invocation.
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
        let key = CacheKey::new("team-a", "alpha-kubeconfig");

        manager.on_child_resolved(&key, make_mock_client());
        manager.on_child_evicted(&key);
        manager.on_child_resolved(&key, make_mock_client());

        assert_eq!(manager.task_count(), 1);
        assert!(manager.has_watcher(&key));
    }

    #[tokio::test]
    async fn test_double_resolve_same_key_replaces_old_task() {
        // Defensive invariant: even if the cache misses a cancel-then-resolve
        // ordering bug, calling on_child_resolved twice for the same key
        // must abort the previous task before installing a new one.
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
        let key = CacheKey::new("team-a", "alpha-kubeconfig");

        manager.on_child_resolved(&key, make_mock_client());
        manager.on_child_resolved(&key, make_mock_client());

        assert_eq!(
            manager.task_count(),
            1,
            "back-to-back resolves must not leak watcher tasks"
        );
    }

    #[tokio::test]
    async fn test_multiple_keys_run_independent_watchers() {
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let manager = ChildNodeWatchManager::new(tx, empty_machine_store());
        let k1 = CacheKey::new("team-a", "alpha-kubeconfig");
        let k2 = CacheKey::new("team-b", "beta-kubeconfig");

        manager.on_child_resolved(&k1, make_mock_client());
        manager.on_child_resolved(&k2, make_mock_client());
        assert_eq!(manager.task_count(), 2);

        manager.on_child_evicted(&k1);
        assert!(!manager.has_watcher(&k1));
        assert!(manager.has_watcher(&k2));
        assert_eq!(manager.task_count(), 1);
    }

    // ========================================================================
    // Node→SM mapping plumbed through mpsc
    // ========================================================================

    #[tokio::test]
    async fn test_emit_refs_pushes_owning_sm_on_channel() {
        // Direct test of the pure helper that runs inside the watcher
        // loop: given a Node and a Machine store with one matching
        // entry, the corresponding ObjectRef must arrive on the mpsc.
        let (tx, mut rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let store = machine_store_with(vec![machine_for_sm("team-a", "gpu-worker", "worker-01")]);
        let node = Node {
            metadata: ObjectMeta {
                name: Some("worker-01".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        super::super::emit_refs_for_node(&node, &store, &tx).await;
        let received = rx
            .try_recv()
            .expect("expected one ObjectRef on the channel");
        assert_eq!(received.name, "gpu-worker");
        assert_eq!(received.namespace.as_deref(), Some("team-a"));
    }

    #[tokio::test]
    async fn test_emit_refs_no_matching_machine_pushes_nothing() {
        let (tx, mut rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(8);
        let store = machine_store_with(vec![machine_for_sm(
            "team-a",
            "gpu-worker",
            "different-node",
        )]);
        let node = Node {
            metadata: ObjectMeta {
                name: Some("worker-01".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        super::super::emit_refs_for_node(&node, &store, &tx).await;
        assert!(rx.try_recv().is_err(), "no matching Machine ⇒ no Ref");
    }

    #[tokio::test]
    async fn test_emit_refs_silently_drops_when_receiver_closed() {
        // Controller shutdown sequence: rx is dropped while the watcher
        // is still running. Send must not panic — the watcher's job is
        // best-effort, the controller's shutdown signal will tear it
        // down separately.
        let (tx, rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(1);
        drop(rx);
        let store = machine_store_with(vec![machine_for_sm("team-a", "gpu-worker", "worker-01")]);
        let node = Node {
            metadata: ObjectMeta {
                name: Some("worker-01".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        // No panic, no hang.
        super::super::emit_refs_for_node(&node, &store, &tx).await;
    }

    // ========================================================================
    // ChildClientCache integration: installing the manager as a hook
    // ========================================================================

    #[test]
    fn test_manager_implements_child_watch_hook() {
        // Compile-time assertion: ChildNodeWatchManager can be stored
        // as Arc<dyn ChildWatchHook>, which is what ChildClientCache::set_hook
        // takes.
        fn _accept(_h: Arc<dyn ChildWatchHook>) {}
        let (tx, _rx) = mpsc::channel::<ObjectRef<crate::crd::ScheduledMachine>>(1);
        let manager: Arc<dyn ChildWatchHook> =
            Arc::new(ChildNodeWatchManager::new(tx, empty_machine_store()));
        _accept(manager);
    }
}
