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
fn test_record_spot_schedule_resolution() {
    record_spot_schedule_resolution("capital-markets", "CapitalMarketsSchedule", "active");
    record_spot_schedule_resolution("capital-markets", "CapitalMarketsSchedule", "inactive");
    record_spot_schedule_resolution("capital-markets", "CapitalMarketsSchedule", "unresolved");
}

#[test]
fn test_record_spot_schedule_resolution_error() {
    record_spot_schedule_resolution_error(
        "capital-markets",
        "CapitalMarketsSchedule",
        "ProviderNotFound",
    );
}

#[test]
fn test_record_spot_schedule_transition() {
    record_spot_schedule_transition("capital-markets", "CapitalMarketsSchedule");
}

#[test]
fn test_set_time_based_active() {
    set_time_based_active("default", "weekdays-9-5", true);
    set_time_based_active("default", "weekdays-9-5", false);
}

#[test]
fn test_record_time_based_transition() {
    record_time_based_transition("default", "weekdays-9-5");
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

/// The kata-config counters are label-less process-globals, so the tests below
/// serialize on this lock — without it, concurrent kata tests increment the
/// same counters between another test's before/after reads.
static KATA_METRICS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_record_kata_config_write_increments_writes_and_stamps_last_sync() {
    let _guard = KATA_METRICS_LOCK.lock().unwrap();
    let writes_before = KATA_CONFIG_WRITES_TOTAL.get();
    let drift_before = KATA_CONFIG_DRIFT_CORRECTED_TOTAL.get();
    record_kata_config_write(false);
    assert!(
        (KATA_CONFIG_WRITES_TOTAL.get() - writes_before - 1.0).abs() < f64::EPSILON,
        "KATA_CONFIG_WRITES_TOTAL must increment by one"
    );
    assert!(
        (KATA_CONFIG_DRIFT_CORRECTED_TOTAL.get() - drift_before).abs() < f64::EPSILON,
        "a non-drift write must not touch the drift counter"
    );
    assert!(
        KATA_CONFIG_LAST_SYNC_TIMESTAMP_SECONDS.get() > 0.0,
        "last-sync gauge must be stamped with a wall-clock timestamp"
    );
}

#[test]
fn test_record_kata_config_write_drift_corrected_increments_both_counters() {
    let _guard = KATA_METRICS_LOCK.lock().unwrap();
    let writes_before = KATA_CONFIG_WRITES_TOTAL.get();
    let drift_before = KATA_CONFIG_DRIFT_CORRECTED_TOTAL.get();
    record_kata_config_write(true);
    assert!(
        (KATA_CONFIG_WRITES_TOTAL.get() - writes_before - 1.0).abs() < f64::EPSILON,
        "a drift-correcting write still counts as a write"
    );
    assert!(
        (KATA_CONFIG_DRIFT_CORRECTED_TOTAL.get() - drift_before - 1.0).abs() < f64::EPSILON,
        "KATA_CONFIG_DRIFT_CORRECTED_TOTAL must increment by one"
    );
}

#[test]
fn test_record_kata_config_delete_increments_deletes_and_stamps_last_sync() {
    let _guard = KATA_METRICS_LOCK.lock().unwrap();
    let before = KATA_CONFIG_DELETES_TOTAL.get();
    record_kata_config_delete();
    assert!(
        (KATA_CONFIG_DELETES_TOTAL.get() - before - 1.0).abs() < f64::EPSILON,
        "KATA_CONFIG_DELETES_TOTAL must increment by one"
    );
    assert!(KATA_CONFIG_LAST_SYNC_TIMESTAMP_SECONDS.get() > 0.0);
}

#[test]
fn test_record_kata_config_sync_unchanged_only_stamps_last_sync() {
    let _guard = KATA_METRICS_LOCK.lock().unwrap();
    let writes_before = KATA_CONFIG_WRITES_TOTAL.get();
    let deletes_before = KATA_CONFIG_DELETES_TOTAL.get();
    record_kata_config_sync_unchanged();
    assert!(
        (KATA_CONFIG_WRITES_TOTAL.get() - writes_before).abs() < f64::EPSILON,
        "an unchanged tick must not count as a write"
    );
    assert!(
        (KATA_CONFIG_DELETES_TOTAL.get() - deletes_before).abs() < f64::EPSILON,
        "an unchanged tick must not count as a delete"
    );
    assert!(KATA_CONFIG_LAST_SYNC_TIMESTAMP_SECONDS.get() > 0.0);
}

#[test]
fn test_record_kata_config_restart_increments_counter() {
    let _guard = KATA_METRICS_LOCK.lock().unwrap();
    let before = KATA_CONFIG_RESTARTS_TOTAL.get();
    record_kata_config_restart();
    assert!(
        (KATA_CONFIG_RESTARTS_TOTAL.get() - before - 1.0).abs() < f64::EPSILON,
        "KATA_CONFIG_RESTARTS_TOTAL must increment by one"
    );
}

#[test]
fn test_record_kata_config_sync_error_increments_counter() {
    let _guard = KATA_METRICS_LOCK.lock().unwrap();
    let before = KATA_CONFIG_SYNC_ERRORS_TOTAL.get();
    record_kata_config_sync_error();
    assert!(
        (KATA_CONFIG_SYNC_ERRORS_TOTAL.get() - before - 1.0).abs() < f64::EPSILON,
        "KATA_CONFIG_SYNC_ERRORS_TOTAL must increment by one"
    );
}
