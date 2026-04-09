// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
//! # Standard Kubernetes labels
//!
//! Label key constants and helper functions for building consistent
//! `BTreeMap<String, String>` label sets that follow the
//! [Kubernetes recommended labels](https://kubernetes.io/docs/concepts/overview/working-with-objects/common-labels/)
//! convention (`app.kubernetes.io/*`).
//!
//! All resources created by 5-Spot carry a common base label set (see
//! [`common_labels`]) plus resource-type-specific labels produced by the
//! specialised helpers.

// ============================================================================
// Recommended Kubernetes Labels
// ============================================================================

/// The name of the application (app.kubernetes.io/name)
pub const LABEL_APP_NAME: &str = "app.kubernetes.io/name";

/// The name of a higher level application this one is part of (app.kubernetes.io/part-of)
pub const LABEL_APP_PART_OF: &str = "app.kubernetes.io/part-of";

/// The component within the architecture (app.kubernetes.io/component)
pub const LABEL_APP_COMPONENT: &str = "app.kubernetes.io/component";

/// The tool being used to manage the operation of an application (app.kubernetes.io/managed-by)
pub const LABEL_APP_MANAGED_BY: &str = "app.kubernetes.io/managed-by";

/// A unique name identifying the instance of an application (app.kubernetes.io/instance)
pub const LABEL_APP_INSTANCE: &str = "app.kubernetes.io/instance";

/// The current version of the application (app.kubernetes.io/version)
pub const LABEL_APP_VERSION: &str = "app.kubernetes.io/version";

// ============================================================================
// 5-Spot-Specific Labels
// ============================================================================

/// Label for identifying `ScheduledMachine` resources
pub const LABEL_SCHEDULED_MACHINE: &str = "5spot.eribourg.dev/scheduled-machine";

/// Label for machine phase
pub const LABEL_MACHINE_PHASE: &str = "5spot.eribourg.dev/phase";

/// Label for schedule enabled status
pub const LABEL_SCHEDULE_ENABLED: &str = "5spot.eribourg.dev/schedule-enabled";

/// Label for cluster deployment reference
pub const LABEL_CLUSTER_DEPLOYMENT: &str = "5spot.eribourg.dev/cluster-deployment";

/// Label for machine priority
pub const LABEL_PRIORITY: &str = "5spot.eribourg.dev/priority";

/// Label for operator instance that owns this resource
pub const LABEL_OPERATOR_INSTANCE: &str = "5spot.eribourg.dev/operator-instance";

// ============================================================================
// Standard Label Values
// ============================================================================

/// Application name for 5-Spot
pub const VALUE_APP_NAME: &str = "5spot";

/// Managed by value
pub const VALUE_MANAGED_BY: &str = "5spot-controller";

/// Component value for scheduled machines
pub const VALUE_COMPONENT_SCHEDULED_MACHINE: &str = "scheduled-machine";

/// Component value for controller
pub const VALUE_COMPONENT_CONTROLLER: &str = "controller";

// ============================================================================
// Helper Functions
// ============================================================================

use std::collections::BTreeMap;

/// Create standard labels for a `ScheduledMachine` resource
#[must_use]
pub fn scheduled_machine_labels(
    name: &str,
    cluster_deployment: &str,
    phase: &str,
) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();

    labels.insert(LABEL_APP_NAME.to_string(), VALUE_APP_NAME.to_string());
    labels.insert(
        LABEL_APP_MANAGED_BY.to_string(),
        VALUE_MANAGED_BY.to_string(),
    );
    labels.insert(
        LABEL_APP_COMPONENT.to_string(),
        VALUE_COMPONENT_SCHEDULED_MACHINE.to_string(),
    );
    labels.insert(LABEL_SCHEDULED_MACHINE.to_string(), name.to_string());
    labels.insert(
        LABEL_CLUSTER_DEPLOYMENT.to_string(),
        cluster_deployment.to_string(),
    );
    labels.insert(LABEL_MACHINE_PHASE.to_string(), phase.to_string());

    labels
}

/// Create common labels that should be applied to all resources
#[must_use]
pub fn common_labels() -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();

    labels.insert(LABEL_APP_NAME.to_string(), VALUE_APP_NAME.to_string());
    labels.insert(
        LABEL_APP_MANAGED_BY.to_string(),
        VALUE_MANAGED_BY.to_string(),
    );

    labels
}

/// Add priority label to existing labels
#[must_use]
pub fn with_priority(
    mut labels: BTreeMap<String, String>,
    priority: u8,
) -> BTreeMap<String, String> {
    labels.insert(LABEL_PRIORITY.to_string(), priority.to_string());
    labels
}

/// Add operator instance label to existing labels
#[must_use]
pub fn with_operator_instance(
    mut labels: BTreeMap<String, String>,
    instance_id: u32,
) -> BTreeMap<String, String> {
    labels.insert(LABEL_OPERATOR_INSTANCE.to_string(), instance_id.to_string());
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduled_machine_labels() {
        let labels = scheduled_machine_labels("test-machine", "test-cluster", "Active");

        assert_eq!(
            labels.get(LABEL_APP_NAME),
            Some(&VALUE_APP_NAME.to_string())
        );
        assert_eq!(
            labels.get(LABEL_SCHEDULED_MACHINE),
            Some(&"test-machine".to_string())
        );
        assert_eq!(
            labels.get(LABEL_CLUSTER_DEPLOYMENT),
            Some(&"test-cluster".to_string())
        );
        assert_eq!(labels.get(LABEL_MACHINE_PHASE), Some(&"Active".to_string()));
    }

    #[test]
    fn test_common_labels() {
        let labels = common_labels();

        assert_eq!(
            labels.get(LABEL_APP_NAME),
            Some(&VALUE_APP_NAME.to_string())
        );
        assert_eq!(
            labels.get(LABEL_APP_MANAGED_BY),
            Some(&VALUE_MANAGED_BY.to_string())
        );
    }

    #[test]
    fn test_with_priority() {
        let mut labels = common_labels();
        labels = with_priority(labels, 75);

        assert_eq!(labels.get(LABEL_PRIORITY), Some(&"75".to_string()));
    }

    #[test]
    fn test_with_operator_instance() {
        let mut labels = common_labels();
        labels = with_operator_instance(labels, 2);

        assert_eq!(labels.get(LABEL_OPERATOR_INSTANCE), Some(&"2".to_string()));
    }
}
