#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::FINALIZER_SCHEDULED_MACHINE;
    use crate::crd::ScheduledMachine;
    use crate::reconcilers::helpers::parse_duration;
    use serde_json::json;

    // ========================================================================
    // Helper to create test ScheduledMachineSpec
    // ========================================================================

    fn create_test_spec() -> crate::crd::ScheduledMachineSpec {
        crate::crd::ScheduledMachineSpec {
            schedule: crate::crd::ScheduleSpec {
                cron: None,
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-17".to_string()],
                timezone: "UTC".to_string(),
                enabled: true,
            },
            cluster_name: "test-cluster".to_string(),
            bootstrap_spec: crate::crd::EmbeddedResource {
                api_version: "bootstrap.cluster.x-k8s.io/v1beta1".to_string(),
                kind: "K0sWorkerConfig".to_string(),
                namespace: None,
                spec: json!({"args": []}),
            },
            infrastructure_spec: crate::crd::EmbeddedResource {
                api_version: "infrastructure.cluster.x-k8s.io/v1beta1".to_string(),
                kind: "RemoteMachine".to_string(),
                namespace: None,
                spec: json!({"address": "192.168.1.100", "port": 22}),
            },
            machine_template: None,
            priority: 50,
            graceful_shutdown_timeout: "5m".to_string(),
            node_drain_timeout: "5m".to_string(),
            kill_switch: false,
        }
    }

    // ========================================================================
    // Schedule evaluation tests
    // ========================================================================

    #[test]
    fn test_evaluate_schedule_disabled() {
        let schedule = crate::crd::ScheduleSpec {
            cron: None,
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "UTC".to_string(),
            enabled: false,
        };

        let result = evaluate_schedule(&schedule, None).unwrap();
        assert!(!result, "Disabled schedule should return false");
    }

    #[test]
    fn test_evaluate_schedule_invalid_timezone() {
        let schedule = crate::crd::ScheduleSpec {
            cron: None,
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "Invalid/Timezone".to_string(),
            enabled: true,
        };

        let result = evaluate_schedule(&schedule, None);
        assert!(result.is_err(), "Invalid timezone should return error");
    }

    // ========================================================================
    // Resource processing tests
    // ========================================================================

    #[test]
    fn test_should_process_resource_single_instance() {
        let result = should_process_resource("test", "default", 50, 1);
        assert!(result, "Single instance should always process");
    }

    #[test]
    fn test_should_process_resource_multiple_instances() {
        // Set environment variable for testing
        std::env::set_var("OPERATOR_INSTANCE_ID", "0");

        let result = should_process_resource("test-resource", "default", 50, 3);
        // The result depends on the hash, but it should be consistent
        let result2 = should_process_resource("test-resource", "default", 50, 3);
        assert_eq!(result, result2, "Same resource should get same result");
    }

    // ========================================================================
    // Duration parsing tests
    // ========================================================================

    #[test]
    fn test_parse_duration_seconds() {
        let duration = parse_duration("30s").unwrap();
        assert_eq!(duration.as_secs(), 30);
    }

    #[test]
    fn test_parse_duration_minutes() {
        let duration = parse_duration("5m").unwrap();
        assert_eq!(duration.as_secs(), 300);
    }

    #[test]
    fn test_parse_duration_hours() {
        let duration = parse_duration("2h").unwrap();
        assert_eq!(duration.as_secs(), 7200);
    }

    #[test]
    fn test_parse_duration_invalid_unit() {
        let result = parse_duration("5d");
        assert!(result.is_err(), "Invalid unit should return error");
    }

    #[test]
    fn test_parse_duration_invalid_value() {
        let result = parse_duration("abcs");
        assert!(result.is_err(), "Invalid value should return error");
    }

    #[test]
    fn test_parse_duration_empty() {
        let result = parse_duration("");
        assert!(result.is_err(), "Empty string should return error");
    }

    // ========================================================================
    // Finalizer tests
    // ========================================================================

    #[test]
    fn test_has_finalizer() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let resource = ScheduledMachine {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                finalizers: Some(vec![FINALIZER_SCHEDULED_MACHINE.to_string()]),
                ..Default::default()
            },
            spec: create_test_spec(),
            status: None,
        };

        assert!(has_finalizer(&resource), "Should detect finalizer");
    }

    #[test]
    fn test_has_finalizer_absent() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let resource = ScheduledMachine {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                finalizers: None,
                ..Default::default()
            },
            spec: create_test_spec(),
            status: None,
        };

        assert!(!has_finalizer(&resource), "Should not detect finalizer");
    }
}
