// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
// Tests for labels module

#[cfg(test)]
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
