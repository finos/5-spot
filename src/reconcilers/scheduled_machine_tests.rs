// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
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
                spec: json!({"args": []}),
            },
            infrastructure_spec: crate::crd::EmbeddedResource {
                api_version: "infrastructure.cluster.x-k8s.io/v1beta1".to_string(),
                kind: "RemoteMachine".to_string(),
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

    // ========================================================================
    // Node drain helper tests
    // ========================================================================

    #[test]
    fn test_should_evict_pod_normal_pod() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(should_evict_pod(&pod), "Normal pod should be evicted");
    }

    #[test]
    fn test_should_evict_pod_succeeded() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Succeeded".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(
            !should_evict_pod(&pod),
            "Succeeded pod should not be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_failed() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Failed".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(!should_evict_pod(&pod), "Failed pod should not be evicted");
    }

    #[test]
    fn test_should_evict_pod_daemonset() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".to_string(),
                    kind: "DaemonSet".to_string(),
                    name: "test-daemonset".to_string(),
                    uid: "test-uid".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(
            !should_evict_pod(&pod),
            "DaemonSet pod should not be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_deployment() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".to_string(),
                    kind: "ReplicaSet".to_string(),
                    name: "test-replicaset".to_string(),
                    uid: "test-uid".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "Deployment/ReplicaSet pod should be evicted"
        );
    }

    // ========================================================================
    // Additional should_evict_pod tests - Edge cases
    // ========================================================================

    #[test]
    fn test_should_evict_pod_running_phase() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("running-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(should_evict_pod(&pod), "Running pod should be evicted");
    }

    #[test]
    fn test_should_evict_pod_pending_phase() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("pending-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Pending".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(should_evict_pod(&pod), "Pending pod should be evicted");
    }

    #[test]
    fn test_should_evict_pod_unknown_phase() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("unknown-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Unknown".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "Unknown phase pod should be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_no_status() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("no-status-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: None,
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "Pod with no status should be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_status_no_phase() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("no-phase-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: None,
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "Pod with status but no phase should be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_empty_owner_references() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("empty-owners-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "Pod with empty owner references should be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_statefulset() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("statefulset-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".to_string(),
                    kind: "StatefulSet".to_string(),
                    name: "test-statefulset".to_string(),
                    uid: "test-uid".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "StatefulSet pod should be evicted (not a DaemonSet)"
        );
    }

    #[test]
    fn test_should_evict_pod_job() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("job-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "batch/v1".to_string(),
                    kind: "Job".to_string(),
                    name: "test-job".to_string(),
                    uid: "test-uid".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(should_evict_pod(&pod), "Job pod should be evicted");
    }

    #[test]
    fn test_should_evict_pod_multiple_owners_with_daemonset() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("multi-owner-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![
                    OwnerReference {
                        api_version: "apps/v1".to_string(),
                        kind: "ReplicaSet".to_string(),
                        name: "test-rs".to_string(),
                        uid: "test-uid-1".to_string(),
                        ..Default::default()
                    },
                    OwnerReference {
                        api_version: "apps/v1".to_string(),
                        kind: "DaemonSet".to_string(),
                        name: "test-ds".to_string(),
                        uid: "test-uid-2".to_string(),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(
            !should_evict_pod(&pod),
            "Pod with DaemonSet among owners should not be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_multiple_owners_no_daemonset() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::Pod;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("multi-owner-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![
                    OwnerReference {
                        api_version: "apps/v1".to_string(),
                        kind: "ReplicaSet".to_string(),
                        name: "test-rs".to_string(),
                        uid: "test-uid-1".to_string(),
                        ..Default::default()
                    },
                    OwnerReference {
                        api_version: "apps/v1".to_string(),
                        kind: "StatefulSet".to_string(),
                        name: "test-sts".to_string(),
                        uid: "test-uid-2".to_string(),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(
            should_evict_pod(&pod),
            "Pod with multiple non-DaemonSet owners should be evicted"
        );
    }

    #[test]
    fn test_should_evict_pod_failed_daemonset() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        // A DaemonSet pod that has Failed status - should not be evicted
        // because we check phase first, then owner
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("failed-ds-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".to_string(),
                    kind: "DaemonSet".to_string(),
                    name: "test-ds".to_string(),
                    uid: "test-uid".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Failed".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(
            !should_evict_pod(&pod),
            "Failed DaemonSet pod should not be evicted (Failed phase checked first)"
        );
    }

    #[test]
    fn test_should_evict_pod_succeeded_regular() {
        use crate::reconcilers::helpers::should_evict_pod;
        use k8s_openapi::api::core::v1::{Pod, PodStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

        // A regular pod that has Succeeded - should not be evicted
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("succeeded-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "batch/v1".to_string(),
                    kind: "Job".to_string(),
                    name: "test-job".to_string(),
                    uid: "test-uid".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Succeeded".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(
            !should_evict_pod(&pod),
            "Succeeded Job pod should not be evicted"
        );
    }

    // ========================================================================
    // Duration parsing tests - Additional edge cases
    // ========================================================================

    #[test]
    fn test_parse_duration_zero_seconds() {
        let duration = parse_duration("0s").unwrap();
        assert_eq!(duration.as_secs(), 0);
    }

    #[test]
    fn test_parse_duration_zero_minutes() {
        let duration = parse_duration("0m").unwrap();
        assert_eq!(duration.as_secs(), 0);
    }

    #[test]
    fn test_parse_duration_large_value() {
        let duration = parse_duration("1000m").unwrap();
        assert_eq!(duration.as_secs(), 60000);
    }

    #[test]
    fn test_parse_duration_single_digit() {
        let duration = parse_duration("1s").unwrap();
        assert_eq!(duration.as_secs(), 1);
    }

    #[test]
    fn test_parse_duration_leading_whitespace() {
        // parse_duration trims whitespace, so this should succeed
        let duration = parse_duration(" 5m").unwrap();
        assert_eq!(
            duration.as_secs(),
            300,
            "Leading whitespace should be trimmed"
        );
    }

    #[test]
    fn test_parse_duration_trailing_whitespace() {
        // parse_duration trims whitespace, so this should succeed
        let duration = parse_duration("5m ").unwrap();
        assert_eq!(
            duration.as_secs(),
            300,
            "Trailing whitespace should be trimmed"
        );
    }

    #[test]
    fn test_parse_duration_both_whitespace() {
        // parse_duration trims whitespace, so this should succeed
        let duration = parse_duration("  10s  ").unwrap();
        assert_eq!(
            duration.as_secs(),
            10,
            "Both leading and trailing whitespace should be trimmed"
        );
    }

    #[test]
    fn test_parse_duration_negative() {
        let result = parse_duration("-5m");
        assert!(result.is_err(), "Negative duration should fail");
    }

    #[test]
    fn test_parse_duration_decimal() {
        let result = parse_duration("5.5m");
        assert!(result.is_err(), "Decimal duration should fail");
    }

    #[test]
    fn test_parse_duration_uppercase_unit() {
        let result = parse_duration("5M");
        assert!(result.is_err(), "Uppercase unit should fail");
    }

    #[test]
    fn test_parse_duration_no_unit() {
        let result = parse_duration("5");
        assert!(result.is_err(), "Missing unit should fail");
    }

    #[test]
    fn test_parse_duration_only_unit() {
        let result = parse_duration("m");
        assert!(result.is_err(), "Only unit should fail");
    }

    #[test]
    fn test_parse_duration_multiple_units() {
        let result = parse_duration("5m30s");
        assert!(result.is_err(), "Multiple units should fail");
    }

    // ========================================================================
    // Schedule evaluation tests - Additional cases
    // ========================================================================

    #[test]
    fn test_evaluate_schedule_cron_not_implemented() {
        let schedule = crate::crd::ScheduleSpec {
            cron: Some("0 9 * * 1-5".to_string()),
            days_of_week: vec![],
            hours_of_day: vec![],
            timezone: "UTC".to_string(),
            enabled: true,
        };

        let result = evaluate_schedule(&schedule, None);
        assert!(result.is_err(), "Cron expression should return error");
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("Cron") || err_msg.contains("cron"),
            "Error should mention cron"
        );
    }

    #[test]
    fn test_evaluate_schedule_empty_days_and_hours() {
        let schedule = crate::crd::ScheduleSpec {
            cron: None,
            days_of_week: vec![],
            hours_of_day: vec![],
            timezone: "UTC".to_string(),
            enabled: true,
        };

        // Empty days/hours means always active
        let result = evaluate_schedule(&schedule, None);
        assert!(result.is_ok(), "Empty days/hours should not error");
    }

    // ========================================================================
    // Finalizer tests - Additional cases
    // ========================================================================

    #[test]
    fn test_has_finalizer_wrong_finalizer() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let resource = ScheduledMachine {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                finalizers: Some(vec!["some-other-finalizer".to_string()]),
                ..Default::default()
            },
            spec: create_test_spec(),
            status: None,
        };

        assert!(
            !has_finalizer(&resource),
            "Should not detect wrong finalizer"
        );
    }

    #[test]
    fn test_has_finalizer_multiple_finalizers() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let resource = ScheduledMachine {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                finalizers: Some(vec![
                    "other-finalizer".to_string(),
                    FINALIZER_SCHEDULED_MACHINE.to_string(),
                    "another-finalizer".to_string(),
                ]),
                ..Default::default()
            },
            spec: create_test_spec(),
            status: None,
        };

        assert!(
            has_finalizer(&resource),
            "Should detect finalizer among multiple"
        );
    }

    #[test]
    fn test_has_finalizer_empty_list() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let resource = ScheduledMachine {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                finalizers: Some(vec![]),
                ..Default::default()
            },
            spec: create_test_spec(),
            status: None,
        };

        assert!(
            !has_finalizer(&resource),
            "Should not detect finalizer in empty list"
        );
    }

    // ========================================================================
    // Resource processing tests - Additional cases
    // ========================================================================

    #[test]
    fn test_should_process_resource_different_namespaces() {
        let result1 = should_process_resource("test", "namespace-a", 50, 3);
        let result2 = should_process_resource("test", "namespace-b", 50, 3);

        // Different namespaces may hash to different instances
        // Just verify they're consistent individually
        let result1_again = should_process_resource("test", "namespace-a", 50, 3);
        let result2_again = should_process_resource("test", "namespace-b", 50, 3);

        assert_eq!(result1, result1_again, "Same ns-a should be consistent");
        assert_eq!(result2, result2_again, "Same ns-b should be consistent");
    }

    #[test]
    fn test_should_process_resource_different_priorities() {
        let result1 = should_process_resource("test", "default", 10, 3);
        let result2 = should_process_resource("test", "default", 90, 3);

        // Different priorities may hash to different instances
        // Just verify they're consistent individually
        let result1_again = should_process_resource("test", "default", 10, 3);
        let result2_again = should_process_resource("test", "default", 90, 3);

        assert_eq!(
            result1, result1_again,
            "Same priority 10 should be consistent"
        );
        assert_eq!(
            result2, result2_again,
            "Same priority 90 should be consistent"
        );
    }

    #[test]
    fn test_should_process_resource_zero_instances() {
        // Edge case: 0 instances should probably always return true or handle gracefully
        // This tests the boundary condition
        let result = should_process_resource("test", "default", 50, 0);
        // With 0 instances, modulo would panic, so implementation should handle this
        // The actual behavior depends on implementation - just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_should_process_resource_empty_name() {
        let result = should_process_resource("", "default", 50, 1);
        assert!(result, "Empty name with single instance should process");
    }

    #[test]
    fn test_should_process_resource_empty_namespace() {
        let result = should_process_resource("test", "", 50, 1);
        assert!(
            result,
            "Empty namespace with single instance should process"
        );
    }

    // ========================================================================
    // generate_reconcile_id — P2-3 correlation ID tests (TDD)
    // ========================================================================

    fn make_sm_with_uid(uid: &str) -> ScheduledMachine {
        let mut sm = ScheduledMachine::new("test-sm", create_test_spec());
        sm.metadata.uid = Some(uid.to_string());
        sm.metadata.namespace = Some("default".to_string());
        sm
    }

    fn make_sm_without_uid() -> ScheduledMachine {
        let mut sm = ScheduledMachine::new("test-sm", create_test_spec());
        sm.metadata.uid = None;
        sm.metadata.namespace = Some("default".to_string());
        sm
    }

    // ---- Positive: well-formed ID ----

    #[test]
    fn test_generate_reconcile_id_is_non_empty() {
        let sm = make_sm_with_uid("a1b2c3d4-0000-0000-0000-abcdef123456");
        let id = generate_reconcile_id(&sm);
        assert!(!id.is_empty(), "reconcile_id must not be empty");
    }

    #[test]
    fn test_generate_reconcile_id_uses_uid_last_segment() {
        // Last '-'-separated segment of the UID must be the ID prefix
        let sm = make_sm_with_uid("a1b2c3d4-0000-0000-0000-deadbeef0001");
        let id = generate_reconcile_id(&sm);
        assert!(
            id.starts_with("deadbeef0001-"),
            "reconcile_id should start with last UID segment, got: {id}"
        );
    }

    #[test]
    fn test_generate_reconcile_id_suffix_is_hex() {
        // The timestamp portion (after the UID prefix) must be lowercase hex
        let sm = make_sm_with_uid("aaaabbbb-0000-0000-0000-ccccddddeeee");
        let id = generate_reconcile_id(&sm);
        // Format: "{uid_last_segment}-{hex_timestamp}"
        let hex_part = id
            .split_once('-')
            .map(|x| x.1)
            .expect("reconcile_id must contain a '-' separator");
        assert!(
            hex_part.chars().all(|c| c.is_ascii_hexdigit()),
            "timestamp portion should be hex digits, got: {hex_part}"
        );
    }

    // ---- Negative: no UID falls back to "unknown" ----

    #[test]
    fn test_generate_reconcile_id_falls_back_to_unknown_when_no_uid() {
        let sm = make_sm_without_uid();
        let id = generate_reconcile_id(&sm);
        assert!(
            id.starts_with("unknown-"),
            "reconcile_id should start with 'unknown' when UID is absent, got: {id}"
        );
    }

    // ---- Exception: uniqueness across calls ----

    #[tokio::test]
    async fn test_generate_reconcile_id_is_unique_across_calls() {
        let sm = make_sm_with_uid("aaaabbbb-0000-0000-0000-111122223333");
        let id1 = generate_reconcile_id(&sm);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let id2 = generate_reconcile_id(&sm);
        assert_ne!(
            id1, id2,
            "Each reconciliation must produce a distinct correlation ID"
        );
    }
}
