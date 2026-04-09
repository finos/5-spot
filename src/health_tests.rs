// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
// Tests for health module

#[cfg(test)]
use super::*;

#[test]
fn test_health_state_new() {
    let state = HealthState::new();
    assert!(state.is_healthy(), "New state should be healthy");
    assert!(!state.is_ready(), "New state should not be ready");
}

#[test]
fn test_health_state_default() {
    let state = HealthState::default();
    assert!(state.is_healthy());
    assert!(!state.is_ready());
}

#[test]
fn test_set_k8s_connected() {
    let state = HealthState::new();
    assert!(!state.is_ready());

    state.set_k8s_connected(true);
    // Still not ready because ready flag is false
    assert!(!state.is_ready());

    state.set_ready(true);
    // Now should be ready
    assert!(state.is_ready());
}

#[test]
fn test_set_ready() {
    let state = HealthState::new();
    state.set_k8s_connected(true);
    state.set_ready(true);
    assert!(state.is_ready());

    state.set_ready(false);
    assert!(!state.is_ready());
}

#[test]
fn test_get_status() {
    let state = HealthState::new();
    let status = state.get_status();

    assert!(status.healthy);
    assert!(!status.ready);
    assert!(!status.k8s_connected);

    state.set_k8s_connected(true);
    state.set_ready(true);
    let status = state.get_status();

    assert!(status.healthy);
    assert!(status.ready);
    assert!(status.k8s_connected);
}

#[test]
fn test_health_state_clone() {
    let state = HealthState::new();
    let cloned = state.clone();

    state.set_k8s_connected(true);
    state.set_ready(true);

    // Cloned state should share the same atomic values
    assert!(cloned.is_ready());
}
