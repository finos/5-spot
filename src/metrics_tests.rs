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
