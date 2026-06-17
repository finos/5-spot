// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::crd::{ScheduledMachine, ScheduledMachineSpec, SpotScheduleRef};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::GroupVersionKind;
    use kube::runtime::reflector::ObjectRef;
    use serde_json::json;

    fn gvk(kind: &str) -> GroupVersionKind {
        GroupVersionKind::gvk("spotschedules.5spot.finos.org", "v1alpha1", kind)
    }

    fn provider_key(kind: &str, namespace: &str, name: &str) -> ProviderKey {
        ProviderKey {
            gvk: gvk(kind),
            namespace: namespace.to_string(),
            name: name.to_string(),
        }
    }

    fn sm_ref(namespace: &str, name: &str) -> ObjectRef<ScheduledMachine> {
        ObjectRef::new(name).within(namespace)
    }

    /// Build a `ScheduledMachine` whose required `spec.schedule` provider
    /// reference is `(kind, name)` in `namespace` (ADR 0009 — the schedule ref
    /// is what `provider_key_for` keys off).
    fn scheduled_machine(namespace: &str, name: &str, spot: (&str, &str)) -> ScheduledMachine {
        let (kind, provider_name) = spot;
        let schedule = SpotScheduleRef {
            api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
            kind: kind.to_string(),
            name: provider_name.to_string(),
        };
        ScheduledMachine {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            spec: ScheduledMachineSpec {
                schedule,
                enabled: true,
                cluster_name: "c".to_string(),
                bootstrap_spec: crate::crd::EmbeddedResource(json!({
                    "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                    "kind": "K0sWorkerConfig", "spec": {}
                })),
                infrastructure_spec: crate::crd::EmbeddedResource(json!({
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

    // ========================================================================
    // ReverseIndex — register / replace / deregister / lookup / gvks
    // ========================================================================

    #[test]
    fn test_register_then_lookup_returns_sm() {
        let mut index = ReverseIndex::default();
        let key = provider_key("CapitalMarketsSchedule", "cm", "nyse");
        index.register(sm_ref("cm", "sm-a"), key.clone());

        assert_eq!(index.lookup(&key), vec![sm_ref("cm", "sm-a")]);
        assert_eq!(index.key_count(), 1);
    }

    #[test]
    fn test_two_sms_share_one_provider_key() {
        let mut index = ReverseIndex::default();
        let key = provider_key("CapitalMarketsSchedule", "cm", "nyse");
        index.register(sm_ref("cm", "sm-a"), key.clone());
        index.register(sm_ref("cm", "sm-b"), key.clone());

        let mut found = index.lookup(&key);
        found.sort_by_key(kube::runtime::reflector::ObjectRef::to_string);
        assert_eq!(found, vec![sm_ref("cm", "sm-a"), sm_ref("cm", "sm-b")]);
        // One provider key, one GVK.
        assert_eq!(index.key_count(), 1);
        assert_eq!(index.referenced_gvks().len(), 1);
    }

    #[test]
    fn test_register_replaces_previous_key_for_same_sm() {
        let mut index = ReverseIndex::default();
        let old = provider_key("CapitalMarketsSchedule", "cm", "nyse");
        let new = provider_key("CapitalMarketsSchedule", "cm", "tsx");
        index.register(sm_ref("cm", "sm-a"), old.clone());
        index.register(sm_ref("cm", "sm-a"), new.clone());

        // No longer indexed under the old provider name…
        assert!(index.lookup(&old).is_empty());
        // …and present under the new one.
        assert_eq!(index.lookup(&new), vec![sm_ref("cm", "sm-a")]);
        assert_eq!(index.key_count(), 1);
    }

    #[test]
    fn test_deregister_removes_sm_and_empties_key() {
        let mut index = ReverseIndex::default();
        let key = provider_key("CapitalMarketsSchedule", "cm", "nyse");
        index.register(sm_ref("cm", "sm-a"), key.clone());
        index.deregister(&sm_ref("cm", "sm-a"));

        assert!(index.lookup(&key).is_empty());
        assert_eq!(index.key_count(), 0);
        assert!(index.referenced_gvks().is_empty());
    }

    #[test]
    fn test_deregister_one_of_two_keeps_the_other() {
        let mut index = ReverseIndex::default();
        let key = provider_key("CapitalMarketsSchedule", "cm", "nyse");
        index.register(sm_ref("cm", "sm-a"), key.clone());
        index.register(sm_ref("cm", "sm-b"), key.clone());
        index.deregister(&sm_ref("cm", "sm-a"));

        assert_eq!(index.lookup(&key), vec![sm_ref("cm", "sm-b")]);
        assert_eq!(index.key_count(), 1);
    }

    #[test]
    fn test_deregister_unknown_sm_is_noop() {
        let mut index = ReverseIndex::default();
        index.deregister(&sm_ref("cm", "ghost"));
        assert_eq!(index.key_count(), 0);
    }

    #[test]
    fn test_referenced_gvks_dedupes_across_providers_of_same_kind() {
        let mut index = ReverseIndex::default();
        index.register(
            sm_ref("cm", "sm-a"),
            provider_key("CapitalMarketsSchedule", "cm", "nyse"),
        );
        index.register(
            sm_ref("cm", "sm-b"),
            provider_key("CapitalMarketsSchedule", "cm", "tsx"),
        );
        index.register(
            sm_ref("ops", "sm-c"),
            provider_key("PrometheusSchedule", "ops", "queue"),
        );

        // Two distinct provider objects of one kind + one of another ⇒ 2 GVKs.
        assert_eq!(index.key_count(), 3);
        assert_eq!(index.referenced_gvks().len(), 2);
    }

    // ========================================================================
    // provider_key_for — extraction from a ScheduledMachine
    // ========================================================================

    /// An SM with a valid schedule ref but no namespace — `provider_key_for`
    /// returns `None` because a provider object is always resolved in the SM's
    /// own namespace, which is unknowable here.
    fn namespaceless_scheduled_machine(name: &str) -> ScheduledMachine {
        let mut sm = scheduled_machine("cm", name, ("CapitalMarketsSchedule", "nyse"));
        sm.metadata.namespace = None;
        sm
    }

    #[test]
    fn test_provider_key_for_extracts_gvk_ns_name() {
        let sm = scheduled_machine("cm", "sm-a", ("CapitalMarketsSchedule", "nyse"));
        let key = provider_key_for(&sm).expect("has schedule ref");
        assert_eq!(key, provider_key("CapitalMarketsSchedule", "cm", "nyse"));
    }

    #[test]
    fn test_provider_key_for_none_without_namespace() {
        // The schedule ref is required, so the only way to get no key is a
        // namespace-less SM (the provider is resolved in the SM's namespace).
        let sm = namespaceless_scheduled_machine("sm-a");
        assert!(provider_key_for(&sm).is_none());
    }

    // ========================================================================
    // SpotScheduleWatchManager — index + watcher lifecycle
    // ========================================================================

    use http::{Request, Response};
    use kube::client::Body;
    use tower_test::mock;

    fn manager() -> SpotScheduleWatchManager {
        // The watcher tasks attempt discovery against this mock client and fail
        // (no responder), which is fine: we assert on tracked-handle counts, not
        // on successful discovery.
        let (svc, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(svc, "default");
        let (tx, _rx) = mpsc::channel(8);
        SpotScheduleWatchManager::new(client, tx)
    }

    #[tokio::test]
    async fn test_observe_starts_one_watcher_and_indexes() {
        let mgr = manager();
        mgr.observe_scheduled_machine(&scheduled_machine(
            "cm",
            "sm-a",
            ("CapitalMarketsSchedule", "nyse"),
        ));
        assert_eq!(mgr.indexed_key_count(), 1);
        assert_eq!(mgr.watcher_count(), 1);
    }

    #[tokio::test]
    async fn test_two_sms_same_gvk_share_one_watcher() {
        let mgr = manager();
        mgr.observe_scheduled_machine(&scheduled_machine(
            "cm",
            "sm-a",
            ("CapitalMarketsSchedule", "nyse"),
        ));
        mgr.observe_scheduled_machine(&scheduled_machine(
            "cm",
            "sm-b",
            ("CapitalMarketsSchedule", "tsx"),
        ));
        // Two provider objects, same GVK ⇒ one watcher.
        assert_eq!(mgr.indexed_key_count(), 2);
        assert_eq!(mgr.watcher_count(), 1);
    }

    #[tokio::test]
    async fn test_forget_last_referencing_sm_stops_watcher() {
        let mgr = manager();
        mgr.observe_scheduled_machine(&scheduled_machine(
            "cm",
            "sm-a",
            ("CapitalMarketsSchedule", "nyse"),
        ));
        assert_eq!(mgr.watcher_count(), 1);

        mgr.forget_scheduled_machine(&scheduled_machine(
            "cm",
            "sm-a",
            ("CapitalMarketsSchedule", "nyse"),
        ));
        assert_eq!(mgr.indexed_key_count(), 0);
        assert_eq!(mgr.watcher_count(), 0);
    }

    // NOTE: the former `test_observe_without_spot_schedule_deregisters` was
    // deleted with ADR 0009: `spec.schedule` is now a required ref, so an SM can
    // never lose its provider reference on a re-apply. The forget/stop-watcher
    // path is still covered by `test_forget_last_referencing_sm_stops_watcher`.
}
