// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # `5spot-capacity-controller`
//!
//! Standalone controller binary for `ScheduledCapacity` (ADR 0011): the
//! **non-handover** pattern, where a physical machine stays in the cluster the
//! whole time and a bounded slice of it is conceded while a spot schedule is
//! open.
//!
//! ## Why this is its own binary and its own identity
//!
//! It could have been a second reconciler inside the main controller. It is
//! not, deliberately (ADR 0011, option D):
//!
//! - The main controller's `ClusterRole` can delete CAPI `Machine`s
//!   cluster-wide and carries bootstrap/infrastructure wildcards the threat
//!   model already records as HIGH residuals. Adding "write a foreign API
//!   group" to that identity would compound them. **Compromising this binary
//!   cannot delete a `Machine`, and compromising the machine controller cannot
//!   write capacity.**
//! - The consumer dependency stays optional: a cluster with no capacity
//!   consumer installs no Deployment, no CRD and no RBAC for any of this.
//! - It is symmetric with the two provider binaries that already *produce* the
//!   spot-schedule contract. This one *consumes* it.
//!
//! ## What it is allowed to do
//!
//! Patch exactly one numeric field, on exactly the API groups in
//! `ALLOWED_CAPACITY_TARGET_API_GROUPS`, with `patch` and nothing else: no
//! `create`, no `delete`, no Secret access. See
//! `deploy/capacity-controller/clusterrole.yaml`.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::Parser;
use kube::Client;
use tracing::info;

use five_spot::constants::{
    DEFAULT_LEASE_DURATION_SECS, DEFAULT_LEASE_NAMESPACE, DEFAULT_LEASE_RENEW_DEADLINE_SECS,
    HEALTH_PORT, METRICS_PORT,
};
use five_spot::health::{start_health_server, HealthState};
use five_spot::leader::{start_leader_election, LeaderElectionConfig};
use five_spot::metrics;
use five_spot::reconcilers::scheduled_capacity;

/// Default `Lease` name. Distinct from the machine controller's so one
/// controller's leadership never gates the other's: they are deliberately
/// separate identities.
const DEFAULT_CAPACITY_LEASE_NAME: &str = "5spot-capacity-controller";

/// CLI / environment configuration for the capacity controller.
#[derive(Debug, Parser)]
#[command(
    name = "5spot-capacity-controller",
    about = "Gates one numeric capacity field on a foreign object from a spot-schedule verdict (ADR 0011)"
)]
struct Cli {
    /// Port for the Prometheus `/metrics` endpoint.
    #[arg(long, env = "METRICS_PORT", default_value_t = METRICS_PORT)]
    metrics_port: u16,

    /// Port for the `/healthz` and `/readyz` endpoints.
    #[arg(long, env = "HEALTH_PORT", default_value_t = HEALTH_PORT)]
    health_port: u16,

    /// Enable leader election; only the lease holder reconciles.
    #[arg(long, env = "ENABLE_LEADER_ELECTION", default_value_t = false)]
    enable_leader_election: bool,

    /// Name of the `Lease` used for leader election.
    #[arg(long, env = "LEASE_NAME", default_value = DEFAULT_CAPACITY_LEASE_NAME)]
    lease_name: String,

    /// Namespace holding the `Lease`.
    #[arg(long, env = "LEASE_NAMESPACE", default_value = DEFAULT_LEASE_NAMESPACE)]
    lease_namespace: String,

    /// Lease duration in seconds.
    #[arg(long, env = "LEASE_DURATION_SECS", default_value_t = DEFAULT_LEASE_DURATION_SECS)]
    lease_duration_secs: u64,

    /// Renew deadline in seconds; the leader must renew before this elapses.
    #[arg(long, env = "LEASE_RENEW_DEADLINE_SECS", default_value_t = DEFAULT_LEASE_RENEW_DEADLINE_SECS)]
    lease_renew_deadline_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    info!(
        metrics_port = cli.metrics_port,
        health_port = cli.health_port,
        leader_election = cli.enable_leader_election,
        "5spot-capacity-controller starting"
    );

    let client = Client::try_default()
        .await
        .context("building in-cluster kube client")?;

    let health_state = HealthState::new();
    health_state.set_k8s_connected(true);
    tokio::spawn(start_health_server(cli.health_port, health_state));
    tokio::spawn(metrics::serve_metrics(cli.metrics_port));

    // With leader election off, every replica acts as leader: the single-replica
    // default deployment, and local runs against a kubeconfig.
    let is_leader = Arc::new(AtomicBool::new(!cli.enable_leader_election));

    if cli.enable_leader_election {
        let leader_config = LeaderElectionConfig {
            lease_name: cli.lease_name.clone(),
            lease_namespace: cli.lease_namespace.clone(),
            lease_duration_secs: cli.lease_duration_secs,
            lease_renew_deadline_secs: cli.lease_renew_deadline_secs,
            fallback_identity: DEFAULT_CAPACITY_LEASE_NAME.to_string(),
        };
        start_leader_election(client.clone(), &leader_config, Arc::clone(&is_leader)).await?;
    } else {
        info!("Leader election disabled; this instance will reconcile all resources");
    }

    scheduled_capacity::run(client, is_leader).await
}

#[cfg(test)]
#[path = "scheduled_capacity_controller_tests.rs"]
mod tests;
