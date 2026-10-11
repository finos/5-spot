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

// ============================================================================
// Label domain: one source of truth for the scheduled-machine key
// ============================================================================

#[test]
fn test_scheduled_machine_label_matches_the_stamped_constant() {
    // Regression: the API group was renamed `5spot.eribourg.dev` ->
    // `5spot.finos.org`, but this module kept the old domain. The reconciler
    // STAMPS constants::SCHEDULED_MACHINE_LABEL on every Machine it creates
    // while main.rs SELECTS on LABEL_SCHEDULED_MACHINE, so a divergence makes
    // both secondary watches match nothing and silently disables the
    // Machine/Node -> ScheduledMachine mappings.
    assert_eq!(
        LABEL_SCHEDULED_MACHINE,
        crate::constants::SCHEDULED_MACHINE_LABEL,
        "the watched label must be the same key the reconciler stamps"
    );
}

#[test]
fn test_all_five_spot_labels_use_the_current_api_group() {
    // No label key may carry the retired domain: anything still on it is both
    // unreachable by the watches and outside RESERVED_LABEL_PREFIXES, so a
    // user could inject it through machineTemplate.labels.
    for key in [
        LABEL_SCHEDULED_MACHINE,
        LABEL_MACHINE_PHASE,
        LABEL_SCHEDULE_ENABLED,
        LABEL_CLUSTER_DEPLOYMENT,
        LABEL_PRIORITY,
        LABEL_OPERATOR_INSTANCE,
    ] {
        assert!(
            key.starts_with("5spot.finos.org/"),
            "label key `{key}` is not on the 5spot.finos.org group"
        );
    }
}

#[test]
fn test_scheduled_machine_label_is_a_reserved_prefix() {
    // The key the controller owns must be un-forgeable by a user: some entry
    // in RESERVED_LABEL_PREFIXES has to cover it, or validate_labels will let
    // machineTemplate.labels set the very key the reverse mapping trusts.
    assert!(
        crate::constants::RESERVED_LABEL_PREFIXES
            .iter()
            .any(|prefix| LABEL_SCHEDULED_MACHINE.starts_with(prefix)),
        "`{LABEL_SCHEDULED_MACHINE}` is not covered by RESERVED_LABEL_PREFIXES"
    );
}
