// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::{
        DEFAULT_LEASE_DURATION_SECS, DEFAULT_LEASE_GRACE_SECS, DEFAULT_LEASE_RENEW_DEADLINE_SECS,
    };

    fn config() -> LeaderElectionConfig {
        LeaderElectionConfig {
            lease_name: "5spot-capacity-controller".to_string(),
            lease_namespace: "5spot-system".to_string(),
            lease_duration_secs: DEFAULT_LEASE_DURATION_SECS,
            lease_renew_deadline_secs: DEFAULT_LEASE_RENEW_DEADLINE_SECS,
            fallback_identity: "capacity-local".to_string(),
        }
    }

    // ========================================================================
    // grace_secs
    // ========================================================================

    #[test]
    fn test_grace_secs_is_duration_minus_renew_deadline() {
        let config = config();
        assert_eq!(
            config.grace_secs(),
            DEFAULT_LEASE_DURATION_SECS - DEFAULT_LEASE_RENEW_DEADLINE_SECS
        );
    }

    /// A renew deadline equal to the duration leaves no slack, which would make
    /// the lease manager treat every renewal as already late. Fall back rather
    /// than hand it zero.
    #[test]
    fn test_grace_secs_falls_back_when_deadline_equals_duration() {
        let config = LeaderElectionConfig {
            lease_duration_secs: 15,
            lease_renew_deadline_secs: 15,
            ..config()
        };
        assert_eq!(config.grace_secs(), DEFAULT_LEASE_GRACE_SECS);
    }

    /// A deadline *longer* than the duration is nonsense configuration; the
    /// subtraction underflows, so the guard must catch it rather than wrap.
    #[test]
    fn test_grace_secs_falls_back_when_deadline_exceeds_duration() {
        let config = LeaderElectionConfig {
            lease_duration_secs: 10,
            lease_renew_deadline_secs: 30,
            ..config()
        };
        assert_eq!(config.grace_secs(), DEFAULT_LEASE_GRACE_SECS);
    }

    #[test]
    fn test_grace_secs_with_generous_duration() {
        let config = LeaderElectionConfig {
            lease_duration_secs: 60,
            lease_renew_deadline_secs: 10,
            ..config()
        };
        assert_eq!(config.grace_secs(), 50);
    }

    // ========================================================================
    // holder_id
    // ========================================================================

    /// No `unsafe` and no process-environment mutation: `holder_id` takes the
    /// pod name, and `start_leader_election` is the one place that reads
    /// `POD_NAME`. The previous version of this test called
    /// `std::env::set_var`, which is `unsafe` in this toolchain and broke the
    /// "only two unsafe blocks in the entire codebase" invariant documented in
    /// `netlink_proc.rs`. It was also racy: the environment is shared by every
    /// test thread.
    #[test]
    fn test_holder_id_prefers_the_pod_name() {
        assert_eq!(
            config().holder_id(Some("5spot-capacity-controller-abc123")),
            "5spot-capacity-controller-abc123",
            "in-cluster the pod name identifies the holder"
        );
    }

    #[test]
    fn test_holder_id_falls_back_when_no_pod_name() {
        assert_eq!(
            config().holder_id(None),
            "capacity-local",
            "outside a cluster the fallback identity is used"
        );
    }

    /// Kubernetes substitutes an empty string, not an absent variable, when a
    /// `fieldRef` resolves to nothing. An empty holder id would leave
    /// `kubectl get lease` unable to say who holds the lease, so it must fall
    /// back like an absent one.
    #[test]
    fn test_holder_id_treats_an_empty_or_blank_pod_name_as_absent() {
        for blank in ["", " ", "\t", "\n  "] {
            assert_eq!(
                config().holder_id(Some(blank)),
                "capacity-local",
                "a blank POD_NAME ({blank:?}) must fall back, not become the identity"
            );
        }
    }

    // ========================================================================
    // Lease naming
    // ========================================================================

    /// Each controller contends for its own Lease. Sharing one would couple
    /// two deliberately separate identities' availability (ADR 0011).
    #[test]
    fn test_lease_names_are_per_controller() {
        let capacity = config();
        let machine = LeaderElectionConfig {
            lease_name: "5spot-controller".to_string(),
            fallback_identity: "machine-local".to_string(),
            ..config()
        };
        assert_ne!(capacity.lease_name, machine.lease_name);
    }
}
