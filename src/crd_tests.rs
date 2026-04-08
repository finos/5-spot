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
            cron: None,
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
            cron: None,
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
    }

    // ========================================================================
    // Serialization tests
    // ========================================================================

    #[test]
    fn test_scheduled_machine_spec_serialization() {
        use serde_json::json;

        let spec = ScheduledMachineSpec {
            schedule: ScheduleSpec {
                cron: None,
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-17".to_string()],
                timezone: "UTC".to_string(),
                enabled: true,
            },
            cluster_name: "test-cluster".to_string(),
            bootstrap_spec: EmbeddedResource {
                api_version: "bootstrap.cluster.x-k8s.io/v1beta1".to_string(),
                kind: "K0sWorkerConfig".to_string(),
                spec: json!({"args": []}),
            },
            infrastructure_spec: EmbeddedResource {
                api_version: "infrastructure.cluster.x-k8s.io/v1beta1".to_string(),
                kind: "RemoteMachine".to_string(),
                spec: json!({"address": "192.168.1.100", "port": 22}),
            },
            machine_template: None,
            priority: 50,
            graceful_shutdown_timeout: "5m".to_string(),
            node_drain_timeout: "5m".to_string(),
            kill_switch: false,
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
}
