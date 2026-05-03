// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
// Tests for metrics module

#[cfg(test)]
use super::*;

#[test]
fn test_record_reconciliation_success() {
    // Just verify it doesn't panic
    record_reconciliation_success("Active", 0.5);
}

#[test]
fn test_record_reconciliation_failure() {
    record_reconciliation_failure("Error", 1.0);
}

#[test]
fn test_record_schedule_evaluation() {
    record_schedule_evaluation(true);
    record_schedule_evaluation(false);
}

#[test]
fn test_set_machines_by_phase() {
    set_machines_by_phase("Active", 5.0);
    set_machines_by_phase("Inactive", 3.0);
}

#[test]
fn test_record_error() {
    record_error("api_error");
    record_error("timeout");
}

#[test]
fn test_record_node_drain() {
    record_node_drain(true);
    record_node_drain(false);
}

#[test]
fn test_record_pod_eviction() {
    record_pod_eviction(true);
    record_pod_eviction(false);
}

#[test]
fn test_record_finalizer_cleanup_timeout_increments_counter() {
    let before = FINALIZER_CLEANUP_TIMEOUTS_TOTAL.get();
    record_finalizer_cleanup_timeout();
    let after = FINALIZER_CLEANUP_TIMEOUTS_TOTAL.get();
    assert!(
        after > before,
        "FINALIZER_CLEANUP_TIMEOUTS_TOTAL must increment on call: before={before} after={after}"
    );
}

#[test]
fn test_init_controller_info() {
    init_controller_info("0.1.0", 0);
}

#[test]
fn test_set_leader_status() {
    set_leader_status(true);
    set_leader_status(false);
}

#[test]
fn test_record_emergency_drain_observes_each_outcome() {
    let before_success = EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&["success"])
        .get_sample_count();
    record_emergency_drain(2.5, EmergencyDrainOutcome::Success);
    let after_success = EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&["success"])
        .get_sample_count();
    assert_eq!(after_success, before_success + 1);

    let before_timeout = EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&["timeout"])
        .get_sample_count();
    record_emergency_drain(60.0, EmergencyDrainOutcome::Timeout);
    let after_timeout = EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&["timeout"])
        .get_sample_count();
    assert_eq!(after_timeout, before_timeout + 1);

    let before_error = EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&["error"])
        .get_sample_count();
    record_emergency_drain(0.1, EmergencyDrainOutcome::Error);
    let after_error = EMERGENCY_DRAIN_DURATION_SECONDS
        .with_label_values(&["error"])
        .get_sample_count();
    assert_eq!(after_error, before_error + 1);
}

#[test]
fn test_record_emergency_reclaim_increments_per_sm_counter() {
    let ns = "test-emergency-reclaim";
    let name = "sm-counter-fixture";
    let before = EMERGENCY_RECLAIMS_TOTAL
        .with_label_values(&[ns, name])
        .get();
    record_emergency_reclaim(ns, name);
    record_emergency_reclaim(ns, name);
    let after = EMERGENCY_RECLAIMS_TOTAL
        .with_label_values(&[ns, name])
        .get();
    assert!(
        (after - before - 2.0).abs() < f64::EPSILON,
        "EMERGENCY_RECLAIMS_TOTAL must increment exactly twice: before={before} after={after}"
    );
}

#[test]
fn test_record_rapid_re_reclaim_increments_per_sm_counter() {
    let ns = "test-rapid-re-reclaim";
    let name = "sm-loop-fixture";
    let before = RAPID_RE_RECLAIMS_TOTAL.with_label_values(&[ns, name]).get();
    record_rapid_re_reclaim(ns, name);
    let after = RAPID_RE_RECLAIMS_TOTAL.with_label_values(&[ns, name]).get();
    assert!(
        (after - before - 1.0).abs() < f64::EPSILON,
        "RAPID_RE_RECLAIMS_TOTAL must increment by one: before={before} after={after}"
    );
}
