// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
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
    // ScheduleSpec tests
    // ========================================================================

    #[test]
    fn test_schedule_spec_get_active_weekdays() {
        let spec = ScheduleSpec {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "UTC".to_string(),
            enabled: true,
        };

        let weekdays = spec.get_active_weekdays().unwrap();
        assert_eq!(weekdays, Some(HashSet::from([0, 1, 2, 3, 4])));
    }

    #[test]
    fn test_schedule_spec_get_active_hours() {
        let spec = ScheduleSpec {
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

    #[test]
    fn test_scheduled_machine_spec_serialization() {
        use serde_json::json;

        let spec = ScheduledMachineSpec {
            schedule: ScheduleSpec {
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-17".to_string()],
                timezone: "UTC".to_string(),
                enabled: true,
            },
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
            kill_if_commands: None,
        };

        // Test that it serializes without errors
        let json_output = serde_json::to_string(&spec).unwrap();
        assert!(json_output.contains("mon-fri"));
        assert!(json_output.contains("192.168.1.100"));
        assert!(json_output.contains("bootstrap"));
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
            schedule: ScheduleSpec {
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-17".to_string()],
                timezone: "UTC".to_string(),
                enabled: true,
            },
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
            kill_if_commands: None,
        }
    }

    #[test]
    fn test_kill_if_commands_absent_deserializes_as_none() {
        let json = serde_json::json!({
            "schedule": {
                "daysOfWeek": ["mon-fri"],
                "hoursOfDay": ["9-17"],
                "timezone": "UTC",
                "enabled": true
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
            "schedule": {"daysOfWeek": [], "hoursOfDay": [], "timezone": "UTC", "enabled": true},
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
}
