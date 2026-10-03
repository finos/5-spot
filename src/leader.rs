// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Leader election (Basel III HA)
//!
//! Shared Kubernetes `Lease`-based leader election for every 5-Spot controller
//! binary. Extracted from `main.rs` when ADR 0011 added a second binary that
//! writes cluster state: two copies of a sixty-line lease dance would have
//! drifted, and the two controllers must agree on what "leader" means.
//!
//! ## How it is used
//!
//! All replicas start as **non-leaders**. A background task runs the
//! `LeaseManager` and flips the shared `AtomicBool` to `true` only while this
//! instance holds the Lease. Reconcilers guard on that flag and return
//! `Action::await_change()` immediately when not leading, so standby replicas
//! react the instant they acquire the lease without polling for it.
//!
//! ## Why each controller gets its own lease name
//!
//! The machine controller and the capacity controller are separate identities
//! with separate blast radii (ADR 0011). Sharing one lease would couple their
//! availability: a capacity controller holding the lease would silence the
//! machine controller, which has nothing to do with capacity. Each binary
//! passes its own [`LeaderElectionConfig::lease_name`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use kube::Client;
use tracing::{error, info};

use crate::constants::DEFAULT_LEASE_GRACE_SECS;

/// Environment variable carrying the pod's own name, used as the lease holder
/// identity so `kubectl get lease` names the actual holder.
const POD_NAME_ENV: &str = "POD_NAME";

/// Parameters for one controller's leader election.
#[derive(Clone, Debug)]
pub struct LeaderElectionConfig {
    /// Name of the Kubernetes `Lease` object to contend for. Distinct per
    /// controller, so one controller's leadership never gates another's.
    pub lease_name: String,
    /// Namespace holding the `Lease`.
    pub lease_namespace: String,
    /// How long a held lease remains valid without renewal.
    pub lease_duration_secs: u64,
    /// The leader must renew before this many seconds elapse.
    pub lease_renew_deadline_secs: u64,
    /// Fallback holder identity when `POD_NAME` is unset (local runs).
    pub fallback_identity: String,
}

impl LeaderElectionConfig {
    /// Grace period handed to the `LeaseManager`: the slack between the lease
    /// duration and the renew deadline.
    ///
    /// Falls back to [`DEFAULT_LEASE_GRACE_SECS`] when the configured values
    /// would produce zero or a negative grace, which would make the manager
    /// treat every renewal as already late.
    #[must_use]
    pub fn grace_secs(&self) -> u64 {
        self.lease_duration_secs
            .checked_sub(self.lease_renew_deadline_secs)
            .filter(|grace| *grace > 0)
            .unwrap_or(DEFAULT_LEASE_GRACE_SECS)
    }

    /// Lease holder identity: `pod_name` when running in-cluster, else
    /// [`fallback_identity`](Self::fallback_identity).
    ///
    /// The environment read is the caller's ([`pod_name_from_env`]) rather than
    /// this function's, so the decision stays pure and testable. Reading
    /// `POD_NAME` here would force its tests to mutate process environment,
    /// which is `unsafe` in this toolchain and would break the codebase-wide
    /// "only two unsafe blocks" invariant documented in
    /// [`crate::netlink_proc`]; it is also racy, since the environment is
    /// shared by every test thread.
    ///
    /// An empty `POD_NAME` falls back too. Kubernetes yields an empty string
    /// rather than an unset variable when a `fieldRef` resolves to nothing, and
    /// an empty lease identity would make `kubectl get lease` unable to name
    /// the holder.
    #[must_use]
    pub fn holder_id(&self, pod_name: Option<&str>) -> String {
        pod_name
            .filter(|name| !name.trim().is_empty())
            .map_or_else(|| self.fallback_identity.clone(), str::to_string)
    }
}

/// Read the pod's own name from the environment, for
/// [`LeaderElectionConfig::holder_id`].
#[must_use]
pub fn pod_name_from_env() -> Option<String> {
    std::env::var(POD_NAME_ENV).ok()
}

/// Start leader election in a background task, flipping `is_leader` as this
/// instance acquires and loses the Lease.
///
/// Sets `is_leader` to `false` before returning: every replica starts as a
/// standby and is promoted only by the background task, so a replica that
/// never wins never reconciles.
///
/// # Arguments
/// * `client` - Kubernetes client authenticated as the controller's ServiceAccount
/// * `config` - this controller's lease parameters
/// * `is_leader` - the flag reconcilers guard on
///
/// # Errors
/// Returns an error if the `LeaseManager` cannot be initialised (for example
/// the ServiceAccount cannot create or get the `Lease`). Once started, lease
/// loss is a state change rather than an error and is reported through
/// `is_leader`.
pub async fn start_leader_election(
    client: Client,
    config: &LeaderElectionConfig,
    is_leader: Arc<AtomicBool>,
) -> Result<()> {
    let holder_id = config.holder_id(pod_name_from_env().as_deref());
    let grace_secs = config.grace_secs();

    info!(
        lease_name = %config.lease_name,
        lease_namespace = %config.lease_namespace,
        lease_duration_secs = config.lease_duration_secs,
        grace_secs,
        holder_id = %holder_id,
        "Leader election enabled, starting as non-leader"
    );

    // All replicas start as non-leaders; the background task promotes the one
    // that acquires the Lease.
    is_leader.store(false, Ordering::Release);

    let manager = kube_lease_manager::LeaseManagerBuilder::new(client, &config.lease_name)
        .with_namespace(&config.lease_namespace)
        .with_duration(config.lease_duration_secs)
        .with_grace(grace_secs)
        .with_identity(&holder_id)
        .build()
        .await
        .context("initialising leader election")?;

    tokio::spawn(async move {
        let (mut channel, task) = manager.watch().await;
        loop {
            if channel.changed().await.is_ok() {
                let acquired = *channel.borrow_and_update();
                is_leader.store(acquired, Ordering::Release);
                if acquired {
                    info!(holder_id = %holder_id, "Acquired leadership lease");
                } else {
                    info!(holder_id = %holder_id, "Lost leadership lease, standby");
                }
            } else {
                error!("Leader election watch channel closed unexpectedly");
                break;
            }
        }
        drop(channel);
        if let Err(e) = task.await {
            error!(error = %e, "Leader election background task failed");
        }
    });

    Ok(())
}

#[cfg(test)]
#[path = "leader_tests.rs"]
mod tests;
