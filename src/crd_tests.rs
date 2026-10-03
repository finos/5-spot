// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants;
    use std::collections::HashSet;

    // ========================================================================
    // Day range parsing tests
    // ========================================================================

    #[test]
    fn test_parse_single_day() {
        let days = vec!["mon".to_string()];
        let result = parse_day_ranges(&days).unwrap();
        assert_eq!(result, HashSet::from([0]));
    }

    #[test]
    fn test_parse_day_range() {
        let days = vec!["mon-fri".to_string()];
        let result = parse_day_ranges(&days).unwrap();
        assert_eq!(result, HashSet::from([0, 1, 2, 3, 4]));
    }

    #[test]
    fn test_parse_day_range_wrapping() {
        let days = vec!["fri-mon".to_string()];
        let result = parse_day_ranges(&days).unwrap();
        assert_eq!(result, HashSet::from([0, 4, 5, 6]));
    }

    #[test]
    fn test_parse_day_combinations() {
        let days = vec!["mon-wed,fri-sun".to_string()];
        let result = parse_day_ranges(&days).unwrap();
        assert_eq!(result, HashSet::from([0, 1, 2, 4, 5, 6]));
    }

    #[test]
    fn test_parse_multiple_day_specs() {
        let days = vec!["mon".to_string(), "wed".to_string(), "fri".to_string()];
        let result = parse_day_ranges(&days).unwrap();
        assert_eq!(result, HashSet::from([0, 2, 4]));
    }

    #[test]
    fn test_parse_invalid_day() {
        let days = vec!["monday".to_string()];
        let result = parse_day_ranges(&days);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid day"));
    }

    #[test]
    fn test_parse_invalid_day_range() {
        let days = vec!["mon-tuesday".to_string()];
        let result = parse_day_ranges(&days);
        assert!(result.is_err());
    }

    // ========================================================================
    // Hour range parsing tests
    // ========================================================================

    #[test]
    fn test_parse_single_hour() {
        let hours = vec!["9".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        assert_eq!(result, HashSet::from([9]));
    }

    #[test]
    fn test_parse_hour_range() {
        let hours = vec!["9-17".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        let expected: HashSet<u8> = (9..=17).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_hour_range_wrapping() {
        let hours = vec!["22-6".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        let expected: HashSet<u8> = (22..=23).chain(0..=6).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_hour_combinations() {
        let hours = vec!["0-9,18-23".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        let expected: HashSet<u8> = (0..=9).chain(18..=23).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_multiple_hour_specs() {
        let hours = vec!["8".to_string(), "12".to_string(), "18".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        assert_eq!(result, HashSet::from([8, 12, 18]));
    }

    #[test]
    fn test_parse_invalid_hour() {
        let hours = vec!["25".to_string()];
        let result = parse_hour_ranges(&hours);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be 0-23"));
    }

    #[test]
    fn test_parse_invalid_hour_range() {
        let hours = vec!["9-25".to_string()];
        let result = parse_hour_ranges(&hours);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_zero_hour() {
        let hours = vec!["0".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        assert_eq!(result, HashSet::from([0]));
    }

    #[test]
    fn test_parse_max_hour() {
        let hours = vec!["23".to_string()];
        let result = parse_hour_ranges(&hours).unwrap();
        assert_eq!(result, HashSet::from([23]));
    }

    // ========================================================================
    // TimeBasedSpotScheduleSpec tests
    //
    // The day/hour/timezone window moved from the (removed) inline
    // ScheduledMachine.spec.schedule ScheduleSpec into the
    // TimeBasedSpotSchedule provider CRD (ADR 0009). The window-evaluation
    // behaviour itself is owned and tested by the provider in
    // src/providers/time_based_tests.rs; here we only pin the accessor
    // helpers that survived the move.
    // ========================================================================

    #[test]
    fn test_time_based_spot_schedule_spec_get_active_weekdays() {
        let spec = TimeBasedSpotScheduleSpec {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "UTC".to_string(),
            enabled: true,
        };

        let weekdays = spec.get_active_weekdays().unwrap();
        assert_eq!(weekdays, Some(HashSet::from([0, 1, 2, 3, 4])));
    }

    #[test]
    fn test_time_based_spot_schedule_spec_get_active_hours() {
        let spec = TimeBasedSpotScheduleSpec {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "UTC".to_string(),
            enabled: true,
        };

        let hours = spec.get_active_hours().unwrap();
        let expected: HashSet<u8> = (9..=17).collect();
        assert_eq!(hours, Some(expected));
    }

    // ========================================================================
    // Condition tests
    // ========================================================================

    #[test]
    fn test_condition_creation() {
        let condition = Condition::new(
            "Ready",
            "True",
            "ReconcileSucceeded",
            "Resource reconciled successfully",
        );

        assert_eq!(condition.r#type, "Ready");
        assert_eq!(condition.status, "True");
        assert_eq!(condition.reason, "ReconcileSucceeded");
        assert_eq!(condition.message, "Resource reconciled successfully");
        assert!(!condition.last_transition_time.is_empty());
    }

    // ========================================================================
    // Phase string constants tests
    // ========================================================================

    #[test]
    fn test_phase_constants() {
        use crate::constants::*;
        assert_eq!(PHASE_PENDING, "Pending");
        assert_eq!(PHASE_ACTIVE, "Active");
        assert_eq!(PHASE_INACTIVE, "Inactive");
        assert_eq!(PHASE_SHUTTING_DOWN, "ShuttingDown");
        assert_eq!(PHASE_DISABLED, "Disabled");
        assert_eq!(PHASE_TERMINATED, "Terminated");
        assert_eq!(PHASE_ERROR, "Error");
        assert_eq!(PHASE_EMERGENCY_REMOVE, "EmergencyRemove");
    }

    #[test]
    fn test_reason_emergency_reclaim_disabled_schedule_is_camelcase() {
        use crate::constants::REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE;
        assert_eq!(
            REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE,
            "EmergencyReclaimDisabledSchedule"
        );
    }

    #[test]
    fn test_emergency_drain_timeout_bounded() {
        use crate::constants::{EMERGENCY_DRAIN_TIMEOUT_SECS, MAX_DURATION_SECS};
        // const block so the assertion is resolved at compile time —
        // guards against a future refactor that sets the timeout to 0
        // or overflows past the 24h cap.
        const _: () = assert!(
            EMERGENCY_DRAIN_TIMEOUT_SECS > 0 && EMERGENCY_DRAIN_TIMEOUT_SECS <= MAX_DURATION_SECS,
            "EMERGENCY_DRAIN_TIMEOUT_SECS must be within (0, MAX_DURATION_SECS]"
        );
    }

    // ========================================================================
    // Emergency reclaim annotation / label constants (roadmap Phase 1 / 2.5)
    // ========================================================================

    #[test]
    fn test_reclaim_annotation_constants_under_5spot_namespace() {
        use crate::constants::*;
        assert_eq!(
            RECLAIM_REQUESTED_ANNOTATION,
            "5spot.finos.org/reclaim-requested"
        );
        assert_eq!(RECLAIM_REASON_ANNOTATION, "5spot.finos.org/reclaim-reason");
        assert_eq!(
            RECLAIM_REQUESTED_AT_ANNOTATION,
            "5spot.finos.org/reclaim-requested-at"
        );
        assert_eq!(RECLAIM_REQUESTED_VALUE, "true");
    }

    #[test]
    fn test_reclaim_agent_label_constants() {
        use crate::constants::*;
        assert_eq!(RECLAIM_AGENT_LABEL, "5spot.finos.org/reclaim-agent");
        assert_eq!(RECLAIM_AGENT_LABEL_ENABLED, "enabled");
    }

    #[test]
    fn test_reclaim_agent_configmap_and_namespace() {
        use crate::constants::*;
        assert_eq!(RECLAIM_AGENT_NAMESPACE, "5spot-system");
        assert_eq!(RECLAIM_AGENT_CONFIGMAP_PREFIX, "reclaim-agent-");
    }

    #[test]
    fn test_reason_emergency_reclaim_is_camelcase() {
        use crate::constants::REASON_EMERGENCY_RECLAIM;
        assert_eq!(REASON_EMERGENCY_RECLAIM, "EmergencyReclaim");
    }

    #[test]
    fn test_reclaim_annotations_covered_by_reserved_prefixes() {
        // Reserved prefixes on user-supplied labels/annotations must include
        // 5spot.finos.org/ so operators can't inject these keys via the
        // ScheduledMachine.spec.machineTemplate surface.
        use crate::constants::{
            RECLAIM_AGENT_LABEL, RECLAIM_REASON_ANNOTATION, RECLAIM_REQUESTED_ANNOTATION,
            RECLAIM_REQUESTED_AT_ANNOTATION, RESERVED_LABEL_PREFIXES,
        };
        for key in [
            RECLAIM_REQUESTED_ANNOTATION,
            RECLAIM_REASON_ANNOTATION,
            RECLAIM_REQUESTED_AT_ANNOTATION,
            RECLAIM_AGENT_LABEL,
        ] {
            assert!(
                RESERVED_LABEL_PREFIXES.iter().any(|p| key.starts_with(p)),
                "{key} must be covered by a RESERVED_LABEL_PREFIXES entry"
            );
        }
    }

    // ========================================================================
    // Serialization tests
    // ========================================================================

    /// Canonical `spec.schedule` reference used across the ScheduledMachine
    /// spec-construction tests. `schedule` is required since ADR 0009, so every
    /// spec needs one; reuse this helper so a contract change touches one place.
    fn sample_schedule_ref() -> SpotScheduleRef {
        SpotScheduleRef {
            api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
            kind: "TimeBasedSpotSchedule".to_string(),
            name: "weekdays-9-5".to_string(),
        }
    }

    #[test]
    fn test_scheduled_machine_spec_serialization() {
        use serde_json::json;

        let spec = ScheduledMachineSpec {
            schedule: sample_schedule_ref(),
            enabled: true,
            cluster_name: "test-cluster".to_string(),
            bootstrap_spec: EmbeddedResource(json!({
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {"args": []}
            })),
            infrastructure_spec: EmbeddedResource(json!({
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "192.168.1.100", "port": 22}
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
        };

        // Test that it serializes without errors
        let json_output = serde_json::to_string(&spec).unwrap();
        assert!(json_output.contains("weekdays-9-5"));
        assert!(json_output.contains("192.168.1.100"));
        assert!(json_output.contains("bootstrap"));
        // skip_serializing_if = Option::is_none must elide the field entirely.
        assert!(
            !json_output.contains("kubeconfigSecretRef"),
            "kubeconfigSecretRef must be omitted when None to preserve clean YAML for single-cluster SMs"
        );
    }

    #[test]
    fn test_scheduled_machine_status_default() {
        let status = ScheduledMachineStatus::default();
        assert_eq!(status.phase, None);
        assert!(status.conditions.is_empty());
        assert_eq!(status.observed_generation, None);
        assert!(!status.in_schedule);
    }

    // ========================================================================
    // Condition.status schema — P2-7 enum constraint tests (TDD)
    // ========================================================================

    fn condition_schema_json() -> serde_json::Value {
        let schema = schemars::schema_for!(Condition);
        serde_json::to_value(schema).expect("schema should serialise")
    }

    // ---- Positive: valid enum values are present in the schema ----

    #[test]
    fn test_condition_status_schema_has_enum_constraint() {
        let schema = condition_schema_json();
        // Navigate to properties.status.enum
        let enum_vals = schema
            .pointer("/definitions/Condition/properties/status/enum")
            .or_else(|| schema.pointer("/properties/status/enum"))
            .expect("Condition.status schema must have an 'enum' constraint for NIST CM-5");
        let arr = enum_vals.as_array().expect("enum must be an array");
        assert_eq!(
            arr.len(),
            3,
            "exactly 3 enum values expected: True, False, Unknown"
        );
    }

    #[test]
    fn test_condition_status_schema_contains_true() {
        let schema = condition_schema_json();
        let enum_vals = schema
            .pointer("/definitions/Condition/properties/status/enum")
            .or_else(|| schema.pointer("/properties/status/enum"))
            .expect("enum must exist");
        assert!(
            enum_vals
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("True")),
            "enum must contain 'True'"
        );
    }

    #[test]
    fn test_condition_status_schema_contains_false() {
        let schema = condition_schema_json();
        let enum_vals = schema
            .pointer("/definitions/Condition/properties/status/enum")
            .or_else(|| schema.pointer("/properties/status/enum"))
            .expect("enum must exist");
        assert!(
            enum_vals
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("False")),
            "enum must contain 'False'"
        );
    }

    #[test]
    fn test_condition_status_schema_contains_unknown() {
        let schema = condition_schema_json();
        let enum_vals = schema
            .pointer("/definitions/Condition/properties/status/enum")
            .or_else(|| schema.pointer("/properties/status/enum"))
            .expect("enum must exist");
        assert!(
            enum_vals
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("Unknown")),
            "enum must contain 'Unknown'"
        );
    }

    // ---- Negative: the Condition type itself still works as a plain String ----

    #[test]
    fn test_condition_new_still_accepts_string_status() {
        // Runtime behaviour unchanged — only the CRD schema gains the constraint
        let c = Condition::new("Ready", "True", "ReconcileSucceeded", "ok");
        assert_eq!(c.status, "True");
    }

    // ========================================================================
    // Status enrichment — providerID + full NodeRef (roadmap Phase 1, TDD RED)
    // ========================================================================

    #[test]
    fn test_status_deserializes_provider_id() {
        let json = serde_json::json!({
            "providerID": "libvirt:///uuid-abc-123",
        });
        let status: ScheduledMachineStatus =
            serde_json::from_value(json).expect("status with providerID must deserialize");
        assert_eq!(
            status.provider_id.as_deref(),
            Some("libvirt:///uuid-abc-123"),
            "providerID must round-trip into ScheduledMachineStatus.provider_id"
        );
    }

    #[test]
    fn test_status_provider_id_omitted_when_none() {
        let status = ScheduledMachineStatus::default();
        let json = serde_json::to_value(&status).expect("serialize default status");
        assert!(
            json.get("providerID").is_none(),
            "providerID must be omitted when None (skip_serializing_if)"
        );
    }

    #[test]
    fn test_status_deserializes_full_node_ref() {
        let json = serde_json::json!({
            "nodeRef": {
                "apiVersion": "v1",
                "kind": "Node",
                "name": "worker-01",
                "uid": "11111111-2222-3333-4444-555555555555",
            }
        });
        let status: ScheduledMachineStatus =
            serde_json::from_value(json).expect("status with full nodeRef must deserialize");
        let node_ref = status.node_ref.expect("nodeRef must be present");
        assert_eq!(node_ref.api_version, "v1");
        assert_eq!(node_ref.kind, "Node");
        assert_eq!(node_ref.name, "worker-01");
        assert_eq!(
            node_ref.uid.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
    }

    #[test]
    fn test_status_node_ref_uid_optional() {
        let json = serde_json::json!({
            "nodeRef": {
                "apiVersion": "v1",
                "kind": "Node",
                "name": "worker-02",
            }
        });
        let status: ScheduledMachineStatus =
            serde_json::from_value(json).expect("nodeRef without uid must still deserialize");
        let node_ref = status.node_ref.expect("nodeRef must be present");
        assert_eq!(node_ref.name, "worker-02");
        assert!(node_ref.uid.is_none());
    }

    #[test]
    fn test_status_rejects_old_shape_node_ref() {
        // Old shape was LocalObjectReference { name }. Deserializing that into
        // the new NodeRef struct must fail loudly so operators see the migration
        // requirement — silent data loss is unacceptable.
        let json = serde_json::json!({
            "nodeRef": { "name": "worker-legacy" }
        });
        let err = serde_json::from_value::<ScheduledMachineStatus>(json)
            .expect_err("old-shape nodeRef must NOT silently succeed");
        let msg = err.to_string();
        assert!(
            msg.contains("apiVersion") || msg.contains("kind"),
            "error must name a missing field so operators know what changed, got: {msg}"
        );
    }

    // ========================================================================
    // killIfCommands — emergency reclaim opt-in (roadmap Phase 2.5, TDD RED)
    // ========================================================================

    fn base_spec() -> ScheduledMachineSpec {
        use serde_json::json;
        ScheduledMachineSpec {
            schedule: sample_schedule_ref(),
            enabled: true,
            cluster_name: "test-cluster".to_string(),
            bootstrap_spec: EmbeddedResource(json!({
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            })),
            infrastructure_spec: EmbeddedResource(json!({
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "192.168.1.100", "port": 22}
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
        }
    }

    #[test]
    fn test_kill_if_commands_absent_deserializes_as_none() {
        let json = serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "weekdays-9-5"
            },
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            }
        });
        let spec: ScheduledMachineSpec =
            serde_json::from_value(json).expect("spec without killIfCommands must deserialize");
        assert!(
            spec.kill_if_commands.is_none(),
            "absent killIfCommands must be None so no agent is installed"
        );
    }

    #[test]
    fn test_kill_if_commands_omitted_from_serialized_output_when_none() {
        let spec = base_spec();
        let json = serde_json::to_value(&spec).expect("serialize spec");
        assert!(
            json.get("killIfCommands").is_none(),
            "killIfCommands must be omitted when None (skip_serializing_if)"
        );
    }

    #[test]
    fn test_kill_if_commands_non_empty_round_trips() {
        let mut spec = base_spec();
        spec.kill_if_commands = Some(vec![
            "java".to_string(),
            "idea".to_string(),
            "steam".to_string(),
        ]);
        let json = serde_json::to_value(&spec).expect("serialize");
        assert_eq!(
            json["killIfCommands"],
            serde_json::json!(["java", "idea", "steam"]),
            "non-empty list must serialize as camelCase killIfCommands"
        );
        let round: ScheduledMachineSpec = serde_json::from_value(json).expect("round-trip");
        assert_eq!(
            round.kill_if_commands.as_deref(),
            Some(["java".to_string(), "idea".to_string(), "steam".to_string()].as_slice())
        );
    }

    #[test]
    fn test_kill_if_commands_empty_list_deserializes_as_some_empty() {
        // Empty list is a valid but meaningless configuration. Preserve the
        // distinction between "absent" (no opt-in) and "present but empty" so
        // the controller can surface a condition warning on empty lists rather
        // than silently treating them as opt-out.
        let json = serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "weekdays-9-5"
            },
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            },
            "killIfCommands": []
        });
        let spec: ScheduledMachineSpec =
            serde_json::from_value(json).expect("empty killIfCommands must deserialize");
        assert_eq!(
            spec.kill_if_commands.as_deref(),
            Some([].as_slice()),
            "empty list must round-trip as Some(vec![]), not None"
        );
    }

    #[test]
    fn test_node_ref_round_trip_serialization() {
        let original = NodeRef {
            api_version: "v1".to_string(),
            kind: "Node".to_string(),
            name: "worker-03".to_string(),
            uid: Some("aaaa-bbbb".to_string()),
        };
        let json = serde_json::to_value(&original).expect("serialize NodeRef");
        assert_eq!(json["apiVersion"], "v1");
        assert_eq!(json["kind"], "Node");
        assert_eq!(json["name"], "worker-03");
        assert_eq!(json["uid"], "aaaa-bbbb");

        let round: NodeRef = serde_json::from_value(json).expect("round-trip NodeRef");
        assert_eq!(round.api_version, "v1");
        assert_eq!(round.uid.as_deref(), Some("aaaa-bbbb"));
    }

    // ========================================================================
    // NodeTaint / TaintEffect tests
    // ========================================================================

    #[test]
    fn test_node_taint_parse_with_value() {
        let json = serde_json::json!({
            "key": "workload",
            "value": "batch",
            "effect": "NoSchedule"
        });
        let taint: NodeTaint = serde_json::from_value(json).expect("parse NodeTaint with value");
        assert_eq!(taint.key, "workload");
        assert_eq!(taint.value.as_deref(), Some("batch"));
        assert_eq!(taint.effect, TaintEffect::NoSchedule);
    }

    #[test]
    fn test_node_taint_parse_without_value() {
        let json = serde_json::json!({
            "key": "dedicated",
            "effect": "NoExecute"
        });
        let taint: NodeTaint = serde_json::from_value(json).expect("parse NodeTaint no value");
        assert_eq!(taint.key, "dedicated");
        assert!(taint.value.is_none());
        assert_eq!(taint.effect, TaintEffect::NoExecute);
    }

    #[test]
    fn test_node_taint_round_trip_without_value_omits_field() {
        let taint = NodeTaint {
            key: "dedicated".to_string(),
            value: None,
            effect: TaintEffect::PreferNoSchedule,
        };
        let json = serde_json::to_value(&taint).expect("serialize");
        assert_eq!(json["key"], "dedicated");
        assert_eq!(json["effect"], "PreferNoSchedule");
        assert!(
            json.get("value").is_none(),
            "value=None must be omitted, got: {json}"
        );
    }

    #[test]
    fn test_taint_effect_rejects_invalid_variant() {
        let json = serde_json::json!({
            "key": "k",
            "effect": "Invalid"
        });
        let result: Result<NodeTaint, _> = serde_json::from_value(json);
        assert!(result.is_err(), "Invalid effect must fail to parse");
    }

    #[test]
    fn test_taint_effect_all_three_variants_round_trip() {
        for (variant, name) in [
            (TaintEffect::NoSchedule, "NoSchedule"),
            (TaintEffect::PreferNoSchedule, "PreferNoSchedule"),
            (TaintEffect::NoExecute, "NoExecute"),
        ] {
            let json = serde_json::to_value(&variant).expect("serialize");
            assert_eq!(json, serde_json::Value::String(name.to_string()));
            let round: TaintEffect = serde_json::from_value(json).expect("round-trip");
            assert_eq!(round, variant);
        }
    }

    #[test]
    fn test_node_taint_hash_eq_by_key_and_effect_and_value() {
        let a = NodeTaint {
            key: "k".to_string(),
            value: Some("v".to_string()),
            effect: TaintEffect::NoSchedule,
        };
        let b = a.clone();
        assert_eq!(a, b, "Clone must be Eq");
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b), "Hash must agree with Eq for clones");
    }

    #[test]
    fn test_scheduled_machine_spec_default_node_taints_is_empty() {
        let json = serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "weekdays-9-5"
            },
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            }
        });
        let spec: ScheduledMachineSpec =
            serde_json::from_value(json).expect("missing nodeTaints must default to empty");
        assert!(spec.node_taints.is_empty());
    }

    #[test]
    fn test_scheduled_machine_spec_node_taints_omitted_when_empty() {
        let json = serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "weekdays-9-5"
            },
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            }
        });
        let spec: ScheduledMachineSpec = serde_json::from_value(json).expect("parse");
        let back = serde_json::to_value(&spec).expect("serialize");
        assert!(
            back.get("nodeTaints").is_none(),
            "empty nodeTaints must serialize as omitted, got: {back}"
        );
    }

    #[test]
    fn test_scheduled_machine_spec_parses_node_taints() {
        let json = serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "weekdays-9-5"
            },
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            },
            "nodeTaints": [
                {"key": "workload", "value": "batch", "effect": "NoSchedule"},
                {"key": "dedicated", "effect": "NoExecute"}
            ]
        });
        let spec: ScheduledMachineSpec = serde_json::from_value(json).expect("parse");
        assert_eq!(spec.node_taints.len(), 2);
        assert_eq!(spec.node_taints[0].key, "workload");
        assert_eq!(spec.node_taints[0].value.as_deref(), Some("batch"));
        assert_eq!(spec.node_taints[0].effect, TaintEffect::NoSchedule);
        assert_eq!(spec.node_taints[1].key, "dedicated");
        assert!(spec.node_taints[1].value.is_none());
        assert_eq!(spec.node_taints[1].effect, TaintEffect::NoExecute);
    }

    // --- validate_node_taints: happy path ---

    #[test]
    fn test_validate_node_taints_empty_list_ok() {
        assert!(validate_node_taints(&[]).is_ok());
    }

    #[test]
    fn test_validate_node_taints_simple_valid_list_ok() {
        let taints = vec![
            NodeTaint {
                key: "workload".to_string(),
                value: Some("batch".to_string()),
                effect: TaintEffect::NoSchedule,
            },
            NodeTaint {
                key: "dedicated".to_string(),
                value: None,
                effect: TaintEffect::NoExecute,
            },
        ];
        assert!(validate_node_taints(&taints).is_ok());
    }

    #[test]
    fn test_validate_node_taints_key_with_prefix_ok() {
        let taints = vec![NodeTaint {
            key: "example.com/team".to_string(),
            value: Some("platform".to_string()),
            effect: TaintEffect::NoSchedule,
        }];
        assert!(validate_node_taints(&taints).is_ok());
    }

    #[test]
    fn test_validate_node_taints_same_key_different_effect_ok() {
        // core/v1 allows same key with different effects.
        let taints = vec![
            NodeTaint {
                key: "workload".to_string(),
                value: Some("batch".to_string()),
                effect: TaintEffect::NoSchedule,
            },
            NodeTaint {
                key: "workload".to_string(),
                value: Some("batch".to_string()),
                effect: TaintEffect::NoExecute,
            },
        ];
        assert!(validate_node_taints(&taints).is_ok());
    }

    // --- validate_node_taints: rejection paths ---

    #[test]
    fn test_validate_node_taints_rejects_empty_key() {
        let taints = vec![NodeTaint {
            key: String::new(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints).expect_err("empty key must be rejected");
        assert!(err.contains("key"), "error should mention key: {err}");
    }

    #[test]
    fn test_validate_node_taints_rejects_leading_hyphen_key() {
        let taints = vec![NodeTaint {
            key: "-bad".to_string(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints).expect_err("leading hyphen key must be rejected");
        assert!(err.contains("key"), "error should mention key: {err}");
    }

    #[test]
    fn test_validate_node_taints_rejects_trailing_hyphen_key() {
        let taints = vec![NodeTaint {
            key: "bad-".to_string(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints).expect_err("trailing hyphen key must be rejected");
        assert!(err.contains("key"), "error should mention key: {err}");
    }

    #[test]
    fn test_validate_node_taints_rejects_key_with_invalid_char() {
        let taints = vec![NodeTaint {
            key: "bad$key".to_string(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints).expect_err("invalid char must be rejected");
        assert!(err.contains("key"), "error should mention key: {err}");
    }

    #[test]
    fn test_validate_node_taints_rejects_key_over_63_chars() {
        let long_key = "a".repeat(64);
        let taints = vec![NodeTaint {
            key: long_key,
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints).expect_err("long key must be rejected");
        assert!(err.contains("63"), "error should mention limit: {err}");
    }

    #[test]
    fn test_validate_node_taints_rejects_value_over_63_chars() {
        let long_value = "v".repeat(64);
        let taints = vec![NodeTaint {
            key: "workload".to_string(),
            value: Some(long_value),
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints).expect_err("long value must be rejected");
        assert!(err.contains("63"), "error should mention limit: {err}");
    }

    #[test]
    fn test_validate_node_taints_rejects_duplicate_key_and_effect() {
        let taints = vec![
            NodeTaint {
                key: "workload".to_string(),
                value: Some("a".to_string()),
                effect: TaintEffect::NoSchedule,
            },
            NodeTaint {
                key: "workload".to_string(),
                value: Some("b".to_string()),
                effect: TaintEffect::NoSchedule,
            },
        ];
        let err = validate_node_taints(&taints).expect_err("duplicate must be rejected");
        assert!(
            err.contains("duplicate"),
            "error should mention duplicate: {err}"
        );
    }

    #[test]
    fn test_validate_node_taints_rejects_reserved_5spot_prefix() {
        let taints = vec![NodeTaint {
            key: "5spot.finos.org/reserved".to_string(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err =
            validate_node_taints(&taints).expect_err("5spot.finos.org/ prefix must be rejected");
        assert!(
            err.contains("5spot.finos.org"),
            "error should mention reserved prefix: {err}"
        );
    }

    #[test]
    fn test_validate_node_taints_rejects_kubernetes_io_prefix() {
        let taints = vec![NodeTaint {
            key: "kubernetes.io/role".to_string(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints)
            .expect_err("kubernetes.io/ prefix must be rejected as reserved");
        assert!(
            err.contains("reserved"),
            "error should mention reserved: {err}"
        );
    }

    #[test]
    fn test_validate_node_taints_rejects_node_kubernetes_io_prefix() {
        let taints = vec![NodeTaint {
            key: "node.kubernetes.io/unreachable".to_string(),
            value: None,
            effect: TaintEffect::NoExecute,
        }];
        let err = validate_node_taints(&taints)
            .expect_err("node.kubernetes.io/ prefix must be rejected as reserved");
        assert!(
            err.contains("reserved"),
            "error should mention reserved: {err}"
        );
    }

    // ========================================================================
    // Phase 2 — status.appliedNodeTaints + NodeTainted condition constants
    // ========================================================================

    #[test]
    fn test_status_applied_node_taints_defaults_empty() {
        let status = ScheduledMachineStatus::default();
        assert!(status.applied_node_taints.is_empty());
    }

    #[test]
    fn test_status_applied_node_taints_omitted_when_empty() {
        let status = ScheduledMachineStatus::default();
        let json = serde_json::to_value(&status).expect("serialize");
        assert!(
            json.get("appliedNodeTaints").is_none(),
            "empty appliedNodeTaints must be omitted, got: {json}"
        );
    }

    #[test]
    fn test_status_applied_node_taints_round_trip() {
        let status = ScheduledMachineStatus {
            applied_node_taints: vec![
                NodeTaint {
                    key: "workload".to_string(),
                    value: Some("batch".to_string()),
                    effect: TaintEffect::NoSchedule,
                },
                NodeTaint {
                    key: "dedicated".to_string(),
                    value: None,
                    effect: TaintEffect::NoExecute,
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_value(&status).expect("serialize");
        assert_eq!(json["appliedNodeTaints"][0]["key"], "workload");
        assert_eq!(json["appliedNodeTaints"][0]["effect"], "NoSchedule");
        assert_eq!(json["appliedNodeTaints"][1]["key"], "dedicated");
        assert_eq!(json["appliedNodeTaints"][1]["effect"], "NoExecute");
        let round: ScheduledMachineStatus =
            serde_json::from_value(json).expect("deserialize status");
        assert_eq!(round.applied_node_taints.len(), 2);
        assert_eq!(round.applied_node_taints[0], status.applied_node_taints[0]);
        assert_eq!(round.applied_node_taints[1], status.applied_node_taints[1]);
    }

    #[test]
    fn test_status_missing_applied_node_taints_deserializes_as_empty() {
        let json = serde_json::json!({});
        let status: ScheduledMachineStatus =
            serde_json::from_value(json).expect("deserialize defaulted status");
        assert!(status.applied_node_taints.is_empty());
    }

    #[test]
    fn test_node_tainted_condition_constants() {
        use crate::constants::{
            CONDITION_TYPE_NODE_TAINTED, REASON_NODE_NOT_READY, REASON_NODE_TAINTS_APPLIED,
            REASON_NODE_TAINT_PATCH_FAILED, REASON_NO_NODE_YET, REASON_TAINT_OWNERSHIP_CONFLICT,
        };
        assert_eq!(CONDITION_TYPE_NODE_TAINTED, "NodeTainted");
        assert_eq!(REASON_NODE_TAINTS_APPLIED, "Applied");
        assert_eq!(REASON_NODE_NOT_READY, "NodeNotReady");
        assert_eq!(REASON_NODE_TAINT_PATCH_FAILED, "PatchFailed");
        assert_eq!(REASON_NO_NODE_YET, "NoNodeYet");
        assert_eq!(REASON_TAINT_OWNERSHIP_CONFLICT, "TaintOwnershipConflict");
    }

    #[test]
    fn test_validate_node_taints_rejects_node_role_kubernetes_io_prefix() {
        let taints = vec![NodeTaint {
            key: "node-role.kubernetes.io/control-plane".to_string(),
            value: None,
            effect: TaintEffect::NoSchedule,
        }];
        let err = validate_node_taints(&taints)
            .expect_err("node-role.kubernetes.io/ prefix must be rejected as reserved");
        assert!(
            err.contains("reserved") || err.contains("machineTemplate"),
            "error should explain: {err}"
        );
    }

    // ========================================================================
    // KubeconfigSecretRef — child-cluster kubeconfig reference tests (TDD)
    //
    // These tests pin the contract for the new optional field that points a
    // ScheduledMachine at a child-cluster kubeconfig Secret in its own
    // namespace. The CRD type is the source of truth; YAML schema bounds and
    // serde defaults are asserted here so a regression flips a test.
    // ========================================================================

    #[test]
    fn test_kubeconfig_secret_ref_default_key_is_value() {
        // Posit: when `key` is omitted in JSON, serde fills in the CAPI
        // convention default ("value"). This keeps the common case zero-config.
        let json = serde_json::json!({ "name": "alpha-kubeconfig" });
        let parsed: KubeconfigSecretRef = serde_json::from_value(json)
            .expect("KubeconfigSecretRef must accept a JSON object with only `name`");
        assert_eq!(parsed.name, "alpha-kubeconfig");
        assert_eq!(
            parsed.key, "value",
            "default key must be `value` per CAPI's <clusterName>-kubeconfig convention"
        );
    }

    #[test]
    fn test_kubeconfig_secret_ref_explicit_key_round_trips() {
        let original = KubeconfigSecretRef {
            name: "team-a-kubeconfig".to_string(),
            key: "kubeconfig".to_string(),
        };
        let serialized = serde_json::to_value(&original).expect("must serialize");
        let parsed: KubeconfigSecretRef =
            serde_json::from_value(serialized.clone()).expect("must round-trip");
        assert_eq!(parsed.name, "team-a-kubeconfig");
        assert_eq!(parsed.key, "kubeconfig");
        // camelCase JSON field names are required for kube-rs / OpenAPI conformance.
        let s = serialized.to_string();
        assert!(s.contains("\"name\""), "expected camelCase `name`: {s}");
        assert!(s.contains("\"key\""), "expected camelCase `key`: {s}");
    }

    #[test]
    fn test_kubeconfig_secret_ref_rejects_unknown_fields() {
        // `deny_unknown_fields` prevents typo-driven silent failures, e.g.
        // `nameSpace` (capital S) or `Name` (capital N) being ignored.
        let json = serde_json::json!({
            "name": "alpha-kubeconfig",
            "key": "value",
            "namespace": "team-a"   // not allowed — cross-namespace refs are forbidden
        });
        let result: Result<KubeconfigSecretRef, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "KubeconfigSecretRef must reject unknown fields (no cross-namespace, no typos)"
        );
    }

    #[test]
    fn test_kubeconfig_secret_ref_name_required() {
        // `name` has no default — omitting it must error.
        let json = serde_json::json!({ "key": "value" });
        let result: Result<KubeconfigSecretRef, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "KubeconfigSecretRef must require `name` (no default)"
        );
    }

    #[test]
    fn test_spec_with_kubeconfig_secret_ref_round_trips() {
        use serde_json::json;

        let spec = ScheduledMachineSpec {
            schedule: sample_schedule_ref(),
            enabled: true,
            cluster_name: "alpha".to_string(),
            bootstrap_spec: EmbeddedResource(json!({
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            })),
            infrastructure_spec: EmbeddedResource(json!({
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            })),
            machine_template: None,
            priority: 50,
            graceful_shutdown_timeout: "5m".to_string(),
            node_drain_timeout: "5m".to_string(),
            kill_switch: false,
            node_taints: vec![],
            kill_if_commands: None,
            kubeconfig_secret_ref: Some(KubeconfigSecretRef {
                name: "alpha-kubeconfig".to_string(),
                key: "value".to_string(),
            }),
            kata: None,
        };
        let s = serde_json::to_string(&spec).expect("must serialize");
        assert!(s.contains("kubeconfigSecretRef"));
        assert!(s.contains("alpha-kubeconfig"));
        let parsed: ScheduledMachineSpec =
            serde_json::from_str(&s).expect("must round-trip with kubeconfigSecretRef");
        let r = parsed
            .kubeconfig_secret_ref
            .expect("ref must survive round-trip");
        assert_eq!(r.name, "alpha-kubeconfig");
        assert_eq!(r.key, "value");
    }

    // ---- Schema-bound assertions ----

    #[test]
    fn test_kubeconfig_secret_ref_name_schema_is_bounded() {
        // Schema must enforce RFC-1123 DNS subdomain bounds on `name` so a
        // malicious / typo'd CR can't trigger an unbounded Secret GET.
        let schema = serde_json::to_value(schemars::schema_for!(KubeconfigSecretRef))
            .expect("schema serializes");
        let name_schema = schema
            .pointer("/properties/name")
            .or_else(|| schema.pointer("/definitions/KubeconfigSecretRef/properties/name"))
            .expect("KubeconfigSecretRef.name property must exist in schema");
        assert!(
            name_schema.get("maxLength").is_some(),
            "name must have maxLength bound (RFC-1123 DNS subdomain = 253): {name_schema}"
        );
        assert!(
            name_schema.get("pattern").is_some(),
            "name must constrain charset via pattern: {name_schema}"
        );
    }

    #[test]
    fn test_kubeconfig_secret_ref_key_schema_is_bounded() {
        let schema = serde_json::to_value(schemars::schema_for!(KubeconfigSecretRef))
            .expect("schema serializes");
        let key_schema = schema
            .pointer("/properties/key")
            .or_else(|| schema.pointer("/definitions/KubeconfigSecretRef/properties/key"))
            .expect("KubeconfigSecretRef.key property must exist in schema");
        assert!(
            key_schema.get("maxLength").is_some(),
            "key must have maxLength bound: {key_schema}"
        );
        assert!(
            key_schema.get("minLength").is_some(),
            "key must have minLength bound to forbid empty-string keys: {key_schema}"
        );
    }

    #[test]
    fn test_spec_kubeconfig_secret_ref_is_optional_in_schema() {
        // Backward compat: the new field must NOT be in the `required` list.
        let schema =
            serde_json::to_value(schemars::schema_for!(ScheduledMachineSpec)).expect("serializes");
        let required = schema
            .pointer("/required")
            .or_else(|| schema.pointer("/definitions/ScheduledMachineSpec/required"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !required.iter().any(|v| v == "kubeconfigSecretRef"),
            "kubeconfigSecretRef MUST be optional to preserve backward compatibility with existing SMs: required={required:?}"
        );
    }

    // ========================================================================
    // KataConfig — Kata config delivery reference tests (ADR 0002, TDD)
    //
    // These tests pin the contract for the optional `spec.kata` field that points
    // a ScheduledMachine at a Secret or ConfigMap **on the workload cluster**
    // holding a Kata containerd drop-in. The CRD type is the source of truth;
    // serde defaults (incl. `namespace` → 5spot-system) and YAML schema bounds
    // are asserted here so a regression flips a test.
    // ========================================================================

    #[test]
    fn test_kata_defaults_when_optional_fields_omitted() {
        // Posit: with only `kind` and `name`, serde fills the defaulted fields
        // (namespace, key, destPath, restartService) so the common case is
        // zero-config.
        let json = serde_json::json!({ "kind": "ConfigMap", "name": "kata-drop-in" });
        let parsed: KataConfig = serde_json::from_value(json)
            .expect("KataConfig must accept a JSON object with only kind + name");
        assert_eq!(parsed.kind, KataConfigSourceKind::ConfigMap);
        assert_eq!(parsed.name, "kata-drop-in");
        assert_eq!(
            parsed.namespace, "5spot-system",
            "default namespace must be the agent's own namespace (5spot-system)"
        );
        assert_eq!(
            parsed.key, "kata-containers.toml",
            "default key must be the containerd drop-in filename"
        );
        assert_eq!(
            parsed.restart_service, "k0sworker.service",
            "default restartService must be the k0s worker unit"
        );
    }

    #[test]
    fn test_kata_explicit_round_trips() {
        let original = KataConfig {
            kind: KataConfigSourceKind::Secret,
            name: "kata-secret".to_string(),
            namespace: "team-alpha".to_string(),
            key: "custom.toml".to_string(),
            restart_service: "k0scontroller.service".to_string(),
        };
        let serialized = serde_json::to_value(&original).expect("must serialize");
        let parsed: KataConfig =
            serde_json::from_value(serialized.clone()).expect("must round-trip");
        assert_eq!(parsed, original);
        // camelCase JSON field names are required for kube-rs / OpenAPI conformance.
        let s = serialized.to_string();
        assert!(s.contains("\"kind\""), "expected camelCase `kind`: {s}");
        assert!(s.contains("\"name\""), "expected camelCase `name`: {s}");
        assert!(s.contains("\"key\""), "expected camelCase `key`: {s}");
        assert!(
            s.contains("\"restartService\""),
            "expected camelCase `restartService`: {s}"
        );
        assert!(
            !s.contains("destPath"),
            "destPath must not serialize — removed per ADR 0005: {s}"
        );
    }

    #[test]
    fn test_kata_config_source_kind_serializes_pascalcase() {
        // The two variants must serialize verbatim so they line up with the
        // Kubernetes object kinds "ConfigMap" / "Secret".
        assert_eq!(
            serde_json::to_value(KataConfigSourceKind::ConfigMap).unwrap(),
            serde_json::json!("ConfigMap")
        );
        assert_eq!(
            serde_json::to_value(KataConfigSourceKind::Secret).unwrap(),
            serde_json::json!("Secret")
        );
    }

    #[test]
    fn test_kata_rejects_invalid_kind() {
        // Only the two PascalCase kinds are valid; a lowercase typo must error.
        let json = serde_json::json!({ "kind": "configmap", "name": "x" });
        let result: Result<KataConfig, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "KataConfig must reject a kind outside {{ConfigMap, Secret}}"
        );
    }

    #[test]
    fn test_kata_namespace_overrides_default() {
        // `namespace` is a first-class field (the workload-cluster namespace the
        // agent reads from); an explicit value must override the 5spot-system
        // default and round-trip.
        let json = serde_json::json!({
            "kind": "ConfigMap",
            "name": "kata-drop-in",
            "namespace": "team-alpha"
        });
        let parsed: KataConfig =
            serde_json::from_value(json).expect("KataConfig must accept an explicit namespace");
        assert_eq!(parsed.namespace, "team-alpha");
    }

    #[test]
    fn test_kata_rejects_unknown_fields() {
        // `deny_unknown_fields` blocks typos — an unrecognised field is a hard
        // error, not a silent miss (ADR 0002).
        let json = serde_json::json!({
            "kind": "ConfigMap",
            "name": "kata-drop-in",
            "bogusField": "x"   // not a KataConfig field
        });
        let result: Result<KataConfig, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "KataConfig must reject unknown fields (typos)"
        );
    }

    #[test]
    fn test_kata_kind_and_name_required() {
        // Neither `kind` nor `name` has a default — omitting either must error.
        let missing_kind = serde_json::json!({ "name": "x" });
        assert!(
            serde_json::from_value::<KataConfig>(missing_kind).is_err(),
            "KataConfig must require `kind`"
        );
        let missing_name = serde_json::json!({ "kind": "Secret" });
        assert!(
            serde_json::from_value::<KataConfig>(missing_name).is_err(),
            "KataConfig must require `name`"
        );
    }

    #[test]
    fn test_spec_with_kata_round_trips() {
        let mut spec = base_spec();
        spec.kata = Some(KataConfig {
            kind: KataConfigSourceKind::ConfigMap,
            name: "kata-drop-in".to_string(),
            namespace: "5spot-system".to_string(),
            key: "kata-containers.toml".to_string(),
            restart_service: "k0sworker.service".to_string(),
        });
        let s = serde_json::to_string(&spec).expect("must serialize");
        assert!(s.contains("\"kata\""), "expected the `kata` field key: {s}");
        assert!(s.contains("kata-drop-in"));
        let parsed: ScheduledMachineSpec =
            serde_json::from_str(&s).expect("must round-trip with spec.kata");
        let r = parsed.kata.expect("ref must survive round-trip");
        assert_eq!(r.kind, KataConfigSourceKind::ConfigMap);
        assert_eq!(r.name, "kata-drop-in");
    }

    #[test]
    fn test_spec_without_kata_omits_field() {
        // skip_serializing_if = Option::is_none must elide the field entirely so
        // existing single-runtime SMs keep clean YAML.
        let spec = base_spec();
        let s = serde_json::to_string(&spec).expect("must serialize");
        assert!(
            !s.contains("\"kata\""),
            "kata must be omitted when None: {s}"
        );
    }

    // ---- Schema-bound assertions ----

    #[test]
    fn test_kata_name_schema_is_bounded() {
        let schema =
            serde_json::to_value(schemars::schema_for!(KataConfig)).expect("schema serializes");
        let name_schema = schema
            .pointer("/properties/name")
            .or_else(|| schema.pointer("/definitions/KataConfig/properties/name"))
            .expect("KataConfig.name property must exist in schema");
        assert!(
            name_schema.get("maxLength").is_some(),
            "name must have maxLength bound (RFC-1123 DNS subdomain = 253): {name_schema}"
        );
        assert!(
            name_schema.get("pattern").is_some(),
            "name must constrain charset via pattern: {name_schema}"
        );
    }

    #[test]
    fn test_kata_schema_has_no_dest_path_property() {
        // ADR 0005: destPath is removed from the contract entirely — the host
        // path is the compile-time constant KATA_CONFIG_DEST_PATH, so no
        // CR author can steer where the privileged agent writes.
        let schema =
            serde_json::to_value(schemars::schema_for!(KataConfig)).expect("schema serializes");
        assert!(
            schema.pointer("/properties/destPath").is_none()
                && schema
                    .pointer("/definitions/KataConfig/properties/destPath")
                    .is_none(),
            "destPath must not exist in the KataConfig schema (ADR 0005)"
        );
    }

    #[test]
    fn test_kata_rejects_dest_path_field() {
        // deny_unknown_fields makes a lingering destPath a hard admission
        // error rather than a silently-ignored no-op — pins the removal.
        let json = serde_json::json!({
            "kind": "ConfigMap",
            "name": "kata-drop-in",
            "destPath": "/etc/k0s/containerd.d/kata.toml"
        });
        assert!(
            serde_json::from_value::<KataConfig>(json).is_err(),
            "destPath must be rejected as an unknown field (ADR 0005)"
        );
    }

    #[test]
    fn test_kata_restart_service_schema_is_bounded() {
        let schema =
            serde_json::to_value(schemars::schema_for!(KataConfig)).expect("schema serializes");
        let svc = schema
            .pointer("/properties/restartService")
            .or_else(|| schema.pointer("/definitions/KataConfig/properties/restartService"))
            .expect("KataConfig.restartService property must exist in schema");
        assert!(
            svc.get("pattern").is_some(),
            "restartService must constrain to a systemd unit via pattern: {svc}"
        );
        assert!(
            svc.get("maxLength").is_some(),
            "restartService must have a maxLength bound: {svc}"
        );
    }

    #[test]
    fn test_kata_restart_service_pattern_excludes_leading_hyphen() {
        // ADR 0012: a hyphen is legal to systemd *inside* a unit name, and must
        // not be legal in first position, where `systemctl` reads it as an
        // option (-H connects to a remote host, -M targets a container). The
        // pre-ADR pattern put `-` in a single class applied to every position,
        // so `-Hbar.example.com.service` was admissible.
        //
        // `regex` is deliberately not a dependency (roadmap 04), so this pins
        // the pattern's shape; the behavioural check is in
        // kata_config_agent_tests, where parse_kata_ref rejects the same value.
        let schema =
            serde_json::to_value(schemars::schema_for!(KataConfig)).expect("schema serializes");
        let svc = schema
            .pointer("/properties/restartService")
            .or_else(|| schema.pointer("/definitions/KataConfig/properties/restartService"))
            .expect("KataConfig.restartService property must exist in schema");
        let pattern = svc
            .get("pattern")
            .and_then(serde_json::Value::as_str)
            .expect("restartService must carry a pattern");
        assert!(
            pattern.starts_with("^[A-Za-z0-9@._]["),
            "the first character class must exclude `-`: {pattern}"
        );
        assert!(
            pattern.ends_with("\\.service$"),
            "the pattern must still anchor on the .service suffix: {pattern}"
        );
    }

    #[test]
    fn test_spec_kata_is_optional_in_schema() {
        // Backward compat: the new field must NOT be in the `required` list.
        let schema =
            serde_json::to_value(schemars::schema_for!(ScheduledMachineSpec)).expect("serializes");
        let required = schema
            .pointer("/required")
            .or_else(|| schema.pointer("/definitions/ScheduledMachineSpec/required"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !required.iter().any(|v| v == "kata"),
            "kata MUST be optional to preserve backward compatibility: required={required:?}"
        );
    }

    // ========================================================================
    // EmbeddedResource — metadata accessors
    // ========================================================================

    #[test]
    fn test_embedded_metadata_namespace_present() {
        use serde_json::json;
        let e = EmbeddedResource(json!({
            "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
            "kind": "K0sWorkerConfig",
            "metadata": { "namespace": "kube-system" },
            "spec": {}
        }));
        assert_eq!(e.metadata_namespace(), Some("kube-system"));
    }

    #[test]
    fn test_embedded_metadata_namespace_absent() {
        use serde_json::json;
        let e = EmbeddedResource(json!({
            "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
            "kind": "K0sWorkerConfig",
            "spec": {}
        }));
        assert_eq!(e.metadata_namespace(), None);
    }

    #[test]
    fn test_embedded_metadata_name_present() {
        use serde_json::json;
        let e = EmbeddedResource(json!({
            "kind": "K0sWorkerConfig",
            "metadata": { "name": "evil-name" },
            "spec": {}
        }));
        assert_eq!(e.metadata_name(), Some("evil-name"));
    }

    #[test]
    fn test_embedded_metadata_labels_extracted() {
        use serde_json::json;
        let e = EmbeddedResource(json!({
            "kind": "K0sWorkerConfig",
            "metadata": { "labels": { "team": "payments", "env": "prod" } },
            "spec": {}
        }));
        let labels = e.metadata_labels();
        assert_eq!(labels.get("team").map(String::as_str), Some("payments"));
        assert_eq!(labels.get("env").map(String::as_str), Some("prod"));
        assert_eq!(labels.len(), 2);
    }

    #[test]
    fn test_embedded_metadata_labels_skips_non_string_values() {
        use serde_json::json;
        // Defensive: non-string values are narrowed out rather than panicking.
        let e = EmbeddedResource(json!({
            "kind": "K0sWorkerConfig",
            "metadata": { "labels": { "ok": "yes", "bad": 7 } },
            "spec": {}
        }));
        let labels = e.metadata_labels();
        assert_eq!(labels.get("ok").map(String::as_str), Some("yes"));
        assert!(
            !labels.contains_key("bad"),
            "non-string value must be skipped"
        );
    }

    #[test]
    fn test_embedded_metadata_annotations_empty_when_absent() {
        use serde_json::json;
        let e = EmbeddedResource(json!({ "kind": "K0sWorkerConfig", "spec": {} }));
        assert!(e.metadata_annotations().is_empty());
    }

    // ========================================================================
    // EmbeddedResource — schema shape (metadata labels/annotations only)
    // ========================================================================

    fn embedded_schema_json() -> serde_json::Value {
        let schema = embedded_resource_schema(&mut schemars::SchemaGenerator::default());
        serde_json::to_value(schema).expect("schema should serialise")
    }

    #[test]
    fn test_embedded_schema_metadata_allows_labels_and_annotations() {
        let schema = embedded_schema_json();
        let meta_props = schema
            .pointer("/properties/metadata/properties")
            .and_then(|v| v.as_object())
            .expect("metadata must declare typed properties");
        assert!(
            meta_props.contains_key("labels"),
            "metadata.labels must be allowed"
        );
        assert!(
            meta_props.contains_key("annotations"),
            "metadata.annotations must be allowed"
        );
        // name/namespace are intentionally NOT declared — they are rejected at
        // admission/runtime, not silently accepted.
        assert!(!meta_props.contains_key("namespace"));
        assert!(!meta_props.contains_key("name"));
    }

    #[test]
    fn test_embedded_schema_metadata_preserves_unknown_fields() {
        // preserve-unknown is REQUIRED so the API server does not prune an
        // unknown metadata.namespace before admission policies can reject it.
        let schema = embedded_schema_json();
        let preserve = schema
            .pointer("/properties/metadata/x-kubernetes-preserve-unknown-fields")
            .and_then(serde_json::Value::as_bool);
        assert_eq!(
            preserve,
            Some(true),
            "metadata must preserve unknown fields so namespace/name can be rejected, not pruned"
        );
    }

    // ========================================================================
    // spec.spotSchedule + multi-version (ADR 0006 / ADR 0007)
    // ========================================================================

    fn spot_schedule_ref() -> SpotScheduleRef {
        SpotScheduleRef {
            api_version: "spotschedules.5spot.finos.org/v1alpha1".to_string(),
            kind: "CapitalMarketsSchedule".to_string(),
            name: "nyse-equities".to_string(),
        }
    }

    #[test]
    fn test_spot_schedule_ref_round_trips() {
        let original = spot_schedule_ref();
        let json = serde_json::to_value(&original).unwrap();
        // camelCase on the wire.
        assert_eq!(
            json.get("apiVersion").and_then(serde_json::Value::as_str),
            Some("spotschedules.5spot.finos.org/v1alpha1")
        );
        let parsed: SpotScheduleRef = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn test_spot_schedule_ref_group_extraction() {
        let r = spot_schedule_ref();
        assert_eq!(r.group(), "spotschedules.5spot.finos.org");
        assert!(r.is_spot_schedule_group());
    }

    #[test]
    fn test_spot_schedule_ref_group_rejects_foreign_group() {
        let r = SpotScheduleRef {
            api_version: "evil.example.com/v1".to_string(),
            kind: "Foo".to_string(),
            name: "bar".to_string(),
        };
        assert_eq!(r.group(), "evil.example.com");
        assert!(!r.is_spot_schedule_group());
    }

    #[test]
    fn test_spot_schedule_ref_group_no_slash_returns_whole() {
        // A version-less apiVersion (rejected by the schema CEL) degrades to the
        // whole string for group(), which then fails the group check.
        let r = SpotScheduleRef {
            api_version: "spotschedules.5spot.finos.org".to_string(),
            kind: "Foo".to_string(),
            name: "bar".to_string(),
        };
        assert_eq!(r.group(), "spotschedules.5spot.finos.org");
        assert!(r.is_spot_schedule_group());
    }

    #[test]
    fn test_spot_schedule_ref_denies_unknown_fields() {
        let json = serde_json::json!({
            "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
            "kind": "CapitalMarketsSchedule",
            "name": "x",
            "namespace": "other"
        });
        // deny_unknown_fields: a stray `namespace` is a hard error, not ignored.
        assert!(serde_json::from_value::<SpotScheduleRef>(json).is_err());
    }

    #[test]
    fn test_api_version_schema_pins_group_via_cel() {
        let schema = serde_json::to_value(spot_schedule_api_version_schema(
            &mut schemars::SchemaGenerator::default(),
        ))
        .unwrap();
        let rule = schema
            .pointer("/x-kubernetes-validations/0/rule")
            .and_then(serde_json::Value::as_str);
        assert_eq!(
            rule,
            Some("self.startsWith('spotschedules.5spot.finos.org/')")
        );
    }

    #[test]
    fn test_is_enabled_follows_spec_enabled() {
        // Since ADR 0009 `is_enabled()` reads the top-level `spec.enabled`
        // master switch, independent of the provider referenced by
        // `spec.schedule`.
        let mut spec = base_spec();
        spec.enabled = true;
        assert!(spec.is_enabled());

        spec.enabled = false;
        assert!(!spec.is_enabled());
    }

    #[test]
    fn test_spec_enabled_defaults_true_when_absent() {
        // `enabled` has serde default `true`, so a spec deserialized without it
        // is administratively enabled — backward compatible with pre-ADR-0009
        // SMs that never carried the field.
        let json = serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "weekdays-9-5"
            },
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            }
        });
        let spec: ScheduledMachineSpec =
            serde_json::from_value(json).expect("spec without enabled must deserialize");
        assert!(spec.enabled, "absent enabled must default to true");
        assert!(spec.is_enabled());
    }

    #[test]
    fn test_schedule_ref_is_required_on_the_wire() {
        // ADR 0009: `schedule` is a required SpotScheduleRef — it always
        // serializes, and a spec without it fails to deserialize.
        let spec = base_spec();
        let json = serde_json::to_value(&spec).unwrap();
        let sched = json
            .get("schedule")
            .expect("schedule must always be present");
        assert_eq!(
            sched.get("kind").and_then(serde_json::Value::as_str),
            Some("TimeBasedSpotSchedule")
        );

        let missing = serde_json::json!({
            "clusterName": "c",
            "bootstrapSpec": {
                "apiVersion": "bootstrap.cluster.x-k8s.io/v1beta1",
                "kind": "K0sWorkerConfig",
                "spec": {}
            },
            "infrastructureSpec": {
                "apiVersion": "infrastructure.cluster.x-k8s.io/v1beta1",
                "kind": "RemoteMachine",
                "spec": {"address": "10.0.0.1", "port": 22}
            }
        });
        assert!(
            serde_json::from_value::<ScheduledMachineSpec>(missing).is_err(),
            "spec without the required schedule ref must fail to deserialize"
        );
    }

    #[test]
    fn test_scheduled_machine_crd_is_single_served_v1beta1() {
        // ADR 0009: the ScheduledMachine CRD is now a single served version
        // (v1beta1); the frozen v1alpha1 ScheduledMachine was removed.
        use kube::CustomResourceExt;
        let crd = ScheduledMachine::crd();
        assert_eq!(
            crd.spec.versions.len(),
            1,
            "ScheduledMachine must have exactly one served version"
        );
        assert_eq!(crd.spec.versions[0].name, "v1beta1");
        assert!(crd.spec.versions[0].storage);
    }

    // ========================================================================
    // CapitalMarketsSchedule reference provider (ADR 0006)
    // ========================================================================

    #[test]
    fn test_capital_markets_schedule_spec_round_trips() {
        let spec = CapitalMarketsScheduleSpec {
            timezone: "America/New_York".to_string(),
            sessions: vec![TradingSession {
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-16".to_string()],
            }],
            holidays: vec!["2026-12-25".to_string()],
            early_closes: vec![EarlyClose {
                date: "2026-11-27".to_string(),
                close_hour: 13,
            }],
        };
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            json.pointer("/earlyCloses/0/closeHour")
                .and_then(serde_json::Value::as_u64),
            Some(13)
        );
        let parsed: CapitalMarketsScheduleSpec = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.sessions.len(), 1);
        assert_eq!(parsed.holidays, vec!["2026-12-25".to_string()]);
    }

    #[test]
    fn test_capital_markets_schedule_status_active_is_the_contract() {
        let status = CapitalMarketsScheduleStatus {
            active: true,
            ..Default::default()
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json.get("active").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn test_capital_markets_schedule_crd_group_and_version() {
        use kube::CustomResourceExt;
        let crd = CapitalMarketsSchedule::crd();
        assert_eq!(crd.spec.group, "spotschedules.5spot.finos.org");
        assert_eq!(crd.spec.names.kind, "CapitalMarketsSchedule");
        assert_eq!(crd.spec.versions[0].name, "v1alpha1");
    }

    #[test]
    fn test_spot_schedule_status_round_trips() {
        let status = SpotScheduleStatus {
            resolved: true,
            active: Some(true),
            reason: Some("Resolved".to_string()),
            message: None,
            provider_generation: Some(4),
            last_transition_time: Some("2026-06-13T13:30:00+00:00".to_string()),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json.get("providerGeneration")
                .and_then(serde_json::Value::as_i64),
            Some(4)
        );
        // message is None ⇒ omitted on the wire.
        assert!(json.get("message").is_none());
        let parsed: SpotScheduleStatus = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_scheduled_machine_status_spot_schedule_omitted_when_none() {
        let status = ScheduledMachineStatus::default();
        let json = serde_json::to_value(&status).unwrap();
        assert!(json.get("spotSchedule").is_none());
    }

    // ========================================================================
    // ScheduledCapacity (ADR 0011)
    // ========================================================================

    /// Canonical `ScheduledCapacity` spec JSON: the worked example from ADR
    /// 0011, gating a banlieue `VirtualMachinePool`'s `spec.warmReplicas` on a
    /// `CapitalMarketsSchedule`. Reused so a contract change touches one place.
    fn scheduled_capacity_json() -> serde_json::Value {
        serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "CapitalMarketsSchedule",
                "name": "nyse-trading-day"
            },
            "targetRef": {
                "apiVersion": "banlieue.io/v1alpha1",
                "kind": "VirtualMachinePool",
                "name": "agent-sandboxes"
            },
            "capacity": { "path": "spec.warmReplicas", "activeValue": 10 },
            "handback": { "drainedPath": "status.claimed", "timeout": "10m" },
            "nodeName": "worker-3"
        })
    }

    #[test]
    fn test_scheduled_capacity_spec_round_trips() {
        let spec: ScheduledCapacitySpec =
            serde_json::from_value(scheduled_capacity_json()).unwrap();

        assert_eq!(spec.schedule.kind, "CapitalMarketsSchedule");
        assert_eq!(spec.target_ref.api_version, "banlieue.io/v1alpha1");
        assert_eq!(spec.target_ref.kind, "VirtualMachinePool");
        assert_eq!(spec.capacity.path, "spec.warmReplicas");
        assert_eq!(spec.capacity.active_value, 10);
        let handback = spec.handback.as_ref().expect("handback present");
        assert_eq!(handback.drained_path.as_deref(), Some("status.claimed"));
        assert_eq!(handback.timeout, "10m");
        assert_eq!(spec.node_name.as_deref(), Some("worker-3"));

        // camelCase on the wire, both directions.
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            json.pointer("/capacity/activeValue")
                .and_then(serde_json::Value::as_i64),
            Some(10)
        );
        assert_eq!(
            json.pointer("/handback/drainedPath")
                .and_then(serde_json::Value::as_str),
            Some("status.claimed")
        );
        assert_eq!(
            json.pointer("/targetRef/apiVersion")
                .and_then(serde_json::Value::as_str),
            Some("banlieue.io/v1alpha1")
        );
        assert_eq!(
            json.get("nodeName").and_then(serde_json::Value::as_str),
            Some("worker-3")
        );
    }

    #[test]
    fn test_scheduled_capacity_spec_defaults() {
        // Only the four required fields; everything else defaults.
        let spec: ScheduledCapacitySpec = serde_json::from_value(serde_json::json!({
            "schedule": {
                "apiVersion": "spotschedules.5spot.finos.org/v1alpha1",
                "kind": "TimeBasedSpotSchedule",
                "name": "office-hours"
            },
            "targetRef": {
                "apiVersion": "banlieue.io/v1alpha1",
                "kind": "VirtualMachinePool",
                "name": "pool"
            },
            "capacity": { "path": "spec.warmReplicas", "activeValue": 4 }
        }))
        .unwrap();

        assert!(spec.enabled, "spec.enabled defaults to true");
        assert!(!spec.kill_switch, "spec.killSwitch defaults to false");
        assert!(
            spec.handback.is_none(),
            "spec.handback is optional (ADR 0011 decision 6): absent means no wait"
        );
        assert!(spec.node_name.is_none());
    }

    #[test]
    fn test_scheduled_capacity_handback_timeout_defaults() {
        // A handback block naming only the drain path still gets a bounded
        // timeout, so the wait can never be unbounded.
        let handback: HandbackPolicy =
            serde_json::from_value(serde_json::json!({ "drainedPath": "status.claimed" })).unwrap();
        assert_eq!(handback.timeout, constants::DEFAULT_HANDBACK_TIMEOUT);
    }

    #[test]
    fn test_scheduled_capacity_handback_drained_path_is_optional() {
        // A timeout with no drain path: nothing to observe, so handback
        // completes on the zero write. Still parses.
        let handback: HandbackPolicy =
            serde_json::from_value(serde_json::json!({ "timeout": "5m" })).unwrap();
        assert!(handback.drained_path.is_none());
        assert_eq!(handback.timeout, "5m");
    }

    #[test]
    fn test_scheduled_capacity_reuses_spot_schedule_ref_group_pin() {
        // ADR 0011 decision 1: the same SpotScheduleRef shape ADR 0009 pinned,
        // so the group helpers behave identically on both kinds.
        let spec: ScheduledCapacitySpec =
            serde_json::from_value(scheduled_capacity_json()).unwrap();
        assert_eq!(spec.schedule.group(), constants::SPOT_SCHEDULE_API_GROUP);
        assert!(spec.schedule.is_spot_schedule_group());
    }

    #[test]
    fn test_scheduled_capacity_status_defaults_and_omits_absent_fields() {
        let status = ScheduledCapacityStatus::default();
        let json = serde_json::to_value(&status).unwrap();
        for absent in [
            "phase",
            "message",
            "writtenValue",
            "lastWriteTime",
            "governedNode",
            "observedDrainedValue",
            "handbackDeadline",
            "targetRef",
            "observedGeneration",
            "spotSchedule",
        ] {
            assert!(
                json.get(absent).is_none(),
                "status.{absent} must be omitted when unset"
            );
        }
        assert!(!status.ready);
        assert!(status.conditions.is_empty());
    }

    #[test]
    fn test_scheduled_capacity_status_round_trips() {
        let status = ScheduledCapacityStatus {
            phase: Some(constants::PHASE_CAPACITY_HANDING_BACK.to_string()),
            written_value: Some(0),
            observed_drained_value: Some(3),
            governed_node: Some("worker-3".to_string()),
            handback_deadline: Some("2026-10-02T14:30:00+00:00".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json.get("writtenValue").and_then(serde_json::Value::as_i64),
            Some(0),
            "a written zero must survive serialisation, not be skipped as a default"
        );
        assert_eq!(
            json.get("observedDrainedValue")
                .and_then(serde_json::Value::as_i64),
            Some(3)
        );
        let parsed: ScheduledCapacityStatus = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.phase.as_deref(), Some("HandingBack"));
        assert_eq!(parsed.governed_node.as_deref(), Some("worker-3"));
    }

    #[test]
    fn test_scheduled_capacity_crd_group_version_kind() {
        use kube::CustomResourceExt;
        let crd = ScheduledCapacity::crd();
        assert_eq!(crd.spec.group, constants::API_GROUP);
        assert_eq!(crd.spec.names.kind, "ScheduledCapacity");
        assert_eq!(crd.spec.names.plural, "scheduledcapacities");
        assert_eq!(
            crd.spec.names.short_names.as_deref(),
            Some(&["scap".to_string()][..])
        );
        assert_eq!(crd.spec.scope, "Namespaced");
        assert_eq!(crd.spec.versions.len(), 1);
        assert_eq!(crd.spec.versions[0].name, "v1alpha1");
        assert!(crd.spec.versions[0].storage);
    }

    /// The write path is a security control (ADR 0011 decision 1), so the CRD
    /// must carry both the charset pattern and the `spec.`-prefix CEL rule.
    /// A schema that lost either would let `metadata.ownerReferences` through
    /// admission and leave only the reconciler guard standing.
    #[test]
    fn test_scheduled_capacity_crd_pins_write_path_to_spec_prefix() {
        use kube::CustomResourceExt;
        let crd = ScheduledCapacity::crd();
        let schema = crd.spec.versions[0]
            .schema
            .as_ref()
            .and_then(|s| s.open_api_v3_schema.as_ref())
            .expect("CRD has an OpenAPI schema");
        let json = serde_json::to_value(schema).unwrap();

        let path = json
            .pointer("/properties/spec/properties/capacity/properties/path")
            .expect("capacity.path is in the schema");
        assert_eq!(
            path.get("pattern").and_then(serde_json::Value::as_str),
            Some(constants::CAPACITY_PATH_PATTERN)
        );
        let rules = path
            .get("x-kubernetes-validations")
            .and_then(serde_json::Value::as_array)
            .expect("capacity.path carries a CEL rule");
        assert!(
            rules.iter().any(|r| r
                .get("rule")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|rule| rule.contains("startsWith('spec.')"))),
            "capacity.path must be CEL-pinned to the spec. prefix, got {rules:?}"
        );
    }

    #[test]
    fn test_scheduled_capacity_crd_pins_drained_path_to_status_prefix() {
        use kube::CustomResourceExt;
        let crd = ScheduledCapacity::crd();
        let json = serde_json::to_value(
            crd.spec.versions[0]
                .schema
                .as_ref()
                .and_then(|s| s.open_api_v3_schema.as_ref())
                .unwrap(),
        )
        .unwrap();
        let drained = json
            .pointer("/properties/spec/properties/handback/properties/drainedPath")
            .expect("handback.drainedPath is in the schema");
        let rules = drained
            .get("x-kubernetes-validations")
            .and_then(serde_json::Value::as_array)
            .expect("handback.drainedPath carries a CEL rule");
        assert!(
            rules.iter().any(|r| r
                .get("rule")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|rule| rule.contains("startsWith('status.')"))),
            "handback.drainedPath must be CEL-pinned to the status. prefix, got {rules:?}"
        );
    }

    /// `activeValue` is what gets written to a foreign object, so its floor
    /// matters: zero is the *inactive* value by construction (ADR 0011
    /// decision 1), which makes an active value of zero a schedule that does
    /// nothing, and a negative one meaningless.
    #[test]
    fn test_scheduled_capacity_crd_bounds_active_value() {
        use kube::CustomResourceExt;
        let crd = ScheduledCapacity::crd();
        let json = serde_json::to_value(
            crd.spec.versions[0]
                .schema
                .as_ref()
                .and_then(|s| s.open_api_v3_schema.as_ref())
                .unwrap(),
        )
        .unwrap();
        let active_value = json
            .pointer("/properties/spec/properties/capacity/properties/activeValue")
            .expect("capacity.activeValue is in the schema");
        assert_eq!(
            active_value
                .get("minimum")
                .and_then(serde_json::Value::as_f64),
            Some(1.0),
            "activeValue must be at least 1; 0 is the fixed inactive value"
        );
    }

    #[test]
    fn test_scheduled_capacity_crd_pins_target_api_version_charset() {
        use kube::CustomResourceExt;
        let crd = ScheduledCapacity::crd();
        let json = serde_json::to_value(
            crd.spec.versions[0]
                .schema
                .as_ref()
                .and_then(|s| s.open_api_v3_schema.as_ref())
                .unwrap(),
        )
        .unwrap();
        let api_version = json
            .pointer("/properties/spec/properties/targetRef/properties/apiVersion")
            .expect("targetRef.apiVersion is in the schema");
        assert!(
            api_version.get("pattern").is_some(),
            "targetRef.apiVersion must constrain its charset; the group allowlist \
             is enforced in the reconciler but the shape is enforced here"
        );
    }

    #[test]
    fn test_scheduled_capacity_crd_has_operator_printcolumns() {
        use kube::CustomResourceExt;
        let crd = ScheduledCapacity::crd();
        let names: Vec<&str> = crd.spec.versions[0]
            .additional_printer_columns
            .as_ref()
            .expect("printer columns")
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        for expected in ["Phase", "Written", "Target", "Schedule", "Enabled"] {
            assert!(
                names.contains(&expected),
                "printer column {expected} missing from {names:?}"
            );
        }
    }

    /// ADR 0011 decision 8: its own phase set, not `ScheduledMachine`'s. A
    /// borrowed enum is permanent under ADR 0007, so the two must stay
    /// distinct and each capacity phase must be unique.
    #[test]
    fn test_capacity_phases_are_distinct_and_not_borrowed() {
        let capacity_phases = [
            constants::PHASE_CAPACITY_PENDING,
            constants::PHASE_CAPACITY_ACTIVE,
            constants::PHASE_CAPACITY_HANDING_BACK,
            constants::PHASE_CAPACITY_HANDBACK_TIMED_OUT,
            constants::PHASE_CAPACITY_INACTIVE,
            constants::PHASE_CAPACITY_DISABLED,
            constants::PHASE_CAPACITY_TERMINATED,
            constants::PHASE_CAPACITY_ERROR,
        ];
        let unique: HashSet<&str> = capacity_phases.iter().copied().collect();
        assert_eq!(
            unique.len(),
            capacity_phases.len(),
            "capacity phases must be distinct"
        );
        // The two phases that exist only here are the whole point of a separate
        // phase set: a drain wait is not node membership.
        assert!(unique.contains("HandingBack"));
        assert!(unique.contains("HandbackTimedOut"));
    }

    #[test]
    fn test_capacity_condition_types_are_distinct() {
        let conditions = [
            constants::CONDITION_TYPE_TARGET_RESOLVED,
            constants::CONDITION_TYPE_CAPACITY_WRITTEN,
            constants::CONDITION_TYPE_HANDBACK_COMPLETE,
            constants::CONDITION_TYPE_HOST_GOVERNANCE_CONFLICT,
            // Reused unchanged from ADR 0006 so provider resolution reports
            // identically across both controllers.
            constants::CONDITION_TYPE_SPOT_SCHEDULE_RESOLVED,
        ];
        let unique: HashSet<&str> = conditions.iter().copied().collect();
        assert_eq!(unique.len(), conditions.len());
    }

    #[test]
    fn test_capacity_inactive_value_is_zero() {
        // ADR 0011 decision 1: the inactive value is fixed at the type's zero,
        // never configurable. A schedule that hands nothing back is not a
        // schedule.
        assert_eq!(constants::CAPACITY_INACTIVE_VALUE, 0);
    }

    /// The segment cap lives in two places that must agree: the schema pattern
    /// (enforced at admission) and `CAPACITY_PATH_MAX_SEGMENTS` (enforced in
    /// the reconciler). Bumping one without the other would let a path through
    /// admission that the reconciler then rejects, or worse the reverse.
    #[test]
    fn test_capacity_path_pattern_encodes_the_segment_cap() {
        let repetitions = constants::CAPACITY_PATH_MAX_SEGMENTS - 1;
        let expected = format!("{{0,{repetitions}}}");
        assert!(
            constants::CAPACITY_PATH_PATTERN.contains(&expected),
            "CAPACITY_PATH_PATTERN must bound repetitions to {expected} to match \
             CAPACITY_PATH_MAX_SEGMENTS = {}, got {}",
            constants::CAPACITY_PATH_MAX_SEGMENTS,
            constants::CAPACITY_PATH_PATTERN
        );
    }

    /// The pattern is a security control, so pin the one property that is
    /// checkable without a regex engine and that silently breaks everything
    /// else if lost: anchoring. An unanchored pattern would match a *substring*,
    /// so `metadata.labels` would pass by virtue of containing `data.labels`.
    ///
    /// Behavioural rejection (array indices, `..`, quotes, `/`) is proven
    /// against the reconciler's own validator, which uses no regex, and against
    /// a real API server in the kind e2e.
    #[test]
    fn test_capacity_path_pattern_is_anchored() {
        let pattern = constants::CAPACITY_PATH_PATTERN;
        assert!(
            pattern.starts_with('^'),
            "pattern must be start-anchored, else a prefix check is bypassable: {pattern}"
        );
        assert!(
            pattern.ends_with('$'),
            "pattern must be end-anchored, else trailing junk is accepted: {pattern}"
        );
        // The only literal dot allowed is the escaped separator: an unescaped
        // `.` would match any character and widen the charset to everything.
        assert!(
            !pattern.replace(r"\.", "").contains('.'),
            "pattern must contain no unescaped '.': {pattern}"
        );
    }

    #[test]
    fn test_allowed_capacity_target_api_groups_is_a_closed_allowlist() {
        // ADR 0011 decision 5: a third allowlist beside bootstrap and
        // infrastructure. It must be non-empty and must not have grown to
        // include a group the capacity controller has no business patching.
        let allowed = constants::ALLOWED_CAPACITY_TARGET_API_GROUPS;
        assert!(!allowed.is_empty());
        for forbidden in [
            constants::CAPI_MACHINE_API_GROUP,
            constants::API_GROUP,
            constants::SPOT_SCHEDULE_API_GROUP,
            "",
        ] {
            assert!(
                !allowed.contains(&forbidden),
                "{forbidden} must not be a capacity target group"
            );
        }
    }
}
