// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # 5-Spot Machine Scheduler — Entry Point
//!
//! This binary starts the controller process. It:
//!
//! 1. Parses CLI flags and environment variables via [`Cli`]
//! 2. Initialises structured logging (tracing)
//! 3. Creates a Kubernetes client with explicit read/write timeouts
//! 4. Verifies the [`ScheduledMachine`] CRD is installed
//! 5. Spawns the Prometheus metrics server and the HTTP health/readiness server
//! 6. Runs the `kube-rs` [`Controller`] loop, distributing reconciliation work
//!    across all active instances via consistent hashing

use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use five_spot::constants::{
    CAPI_GROUP, CAPI_MACHINE_API_VERSION, CAPI_RESOURCE_MACHINES, DEFAULT_LEASE_DURATION_SECS,
    DEFAULT_LEASE_GRACE_SECS, DEFAULT_LEASE_NAME, DEFAULT_LEASE_NAMESPACE,
    DEFAULT_LEASE_RENEW_DEADLINE_SECS, DEFAULT_LEASE_RETRY_PERIOD_SECS, HEALTH_PORT,
    K8S_API_TIMEOUT_SECS, METRICS_PORT,
};
use five_spot::crd::ScheduledMachine;
use five_spot::health::{start_health_server, HealthState};
use five_spot::labels::LABEL_SCHEDULED_MACHINE;
use five_spot::metrics::init_controller_info;
use five_spot::reconcilers::{
    error_policy, machine_to_scheduled_machine, node_to_scheduled_machines,
    reconcile_scheduled_machine, Context,
};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Node;
use kube::{
    api::{ApiResource, GroupVersionKind, ListParams},
    core::DynamicObject,
    runtime::{watcher::Config, Controller},
    Api, Client,
};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Command-line interface for the 5-Spot operator.
///
/// All fields can also be set via the corresponding environment variables,
/// which is the recommended approach for Kubernetes deployments.
#[derive(Parser, Debug)]
#[clap(
    name = "5spot",
    about = "Kubernetes operator for time-based machine scheduling",
    version
)]
struct Cli {
    /// Operator instance ID for multi-instance deployments
    #[clap(long, env = "OPERATOR_INSTANCE_ID", default_value = "0")]
    instance_id: u32,

    /// Total number of operator instances
    #[clap(long, env = "OPERATOR_INSTANCE_COUNT", default_value = "1")]
    instance_count: u32,

    /// Metrics server port
    #[clap(long, env = "METRICS_PORT", default_value_t = METRICS_PORT)]
    metrics_port: u16,

    /// Health check server port
    #[clap(long, env = "HEALTH_PORT", default_value_t = HEALTH_PORT)]
    health_port: u16,

    /// Enable verbose logging
    #[clap(short, long)]
    verbose: bool,

    /// Log format: "json" for structured JSON (SIEM/production), "text" for human-readable (local dev)
    #[clap(long, env = "RUST_LOG_FORMAT", default_value = "json")]
    log_format: String,

    // -----------------------------------------------------------------------
    // Leader election (Basel III HA)
    // -----------------------------------------------------------------------
    /// Enable leader election — only the lease holder reconciles resources.
    /// Required for multi-replica deployments (Basel III HA / NIST SI-2).
    #[clap(long, env = "ENABLE_LEADER_ELECTION", default_value = "false")]
    enable_leader_election: bool,

    /// Kubernetes Lease resource name used for leader election
    #[clap(long, env = "LEASE_NAME", default_value = DEFAULT_LEASE_NAME)]
    lease_name: String,

    /// Namespace in which to create the leader election Lease
    #[clap(long, env = "POD_NAMESPACE", default_value = DEFAULT_LEASE_NAMESPACE)]
    lease_namespace: String,

    /// Lease validity duration in seconds — how long the Lease is considered held
    #[clap(long, env = "LEASE_DURATION_SECONDS", default_value_t = DEFAULT_LEASE_DURATION_SECS)]
    lease_duration_secs: u64,

    /// Renew deadline in seconds — the leader must renew before this many seconds elapse
    #[clap(long, env = "LEASE_RENEW_DEADLINE_SECONDS", default_value_t = DEFAULT_LEASE_RENEW_DEADLINE_SECS)]
    lease_renew_deadline_secs: u64,

    /// Retry period in seconds — documented for ops parity; not a direct `LeaseManager` parameter
    #[clap(long, env = "LEASE_RETRY_PERIOD_SECONDS", default_value_t = DEFAULT_LEASE_RETRY_PERIOD_SECS)]
    _lease_retry_period_secs: u64,

    /// On finalizer-cleanup timeout, force-remove the finalizer so namespace
    /// deletion is unblocked (default). Set to `false` for strict-cleanup
    /// mode where the SM stays stuck until cleanup succeeds — operators
    /// then need an external sweep to garbage-collect stalled SMs. The
    /// `fivespot_finalizer_cleanup_timeouts_total` metric and the
    /// `FinalizerCleanupTimedOut` Warning event fire in both modes.
    #[clap(long, env = "FORCE_FINALIZER_ON_TIMEOUT", default_value_t = true)]
    force_finalizer_on_timeout: bool,
}

/// Async entry point.
///
/// Initialises the controller and blocks until the process receives a shutdown
/// signal (SIGTERM / SIGINT).  Returns an error if:
/// - The Kubernetes client cannot be configured
/// - The `ScheduledMachine` CRD is not present in the cluster
#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing/logging
    let log_level = if cli.verbose {
        "debug,kube=info,hyper=info,tower=info"
    } else {
        "info,kube=warn,hyper=warn,tower=warn"
    };

    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| log_level.into());

    if cli.log_format.eq_ignore_ascii_case("json") {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    info!(
        instance_id = cli.instance_id,
        instance_count = cli.instance_count,
        metrics_port = cli.metrics_port,
        health_port = cli.health_port,
        "Starting 5-Spot Machine Scheduler"
    );

    // Initialize health state
    let health_state = HealthState::new();

    // Initialize controller info metric
    init_controller_info(env!("CARGO_PKG_VERSION"), cli.instance_id);

    // Create Kubernetes client with explicit timeouts (NIST SC-5, Basel III)
    let mut kube_config = kube::Config::infer().await?;
    kube_config.read_timeout = Some(std::time::Duration::from_secs(K8S_API_TIMEOUT_SECS));
    kube_config.write_timeout = Some(std::time::Duration::from_secs(K8S_API_TIMEOUT_SECS));
    let client = Client::try_from(kube_config)?;
    info!(
        timeout_secs = K8S_API_TIMEOUT_SECS,
        "Connected to Kubernetes API"
    );

    // Mark Kubernetes as connected
    health_state.set_k8s_connected(true);

    // Create shared context
    let context = Arc::new(
        Context::new(client.clone(), cli.instance_id, cli.instance_count)
            .with_force_finalizer_on_timeout(cli.force_finalizer_on_timeout),
    );

    // Leader election (Basel III HA — P2-4)
    //
    // When enabled, all replicas start as non-leaders (is_leader = false).  A
    // background task runs the LeaseManager and flips is_leader to true only
    // when this instance holds the Kubernetes Lease.  reconcile_guarded returns
    // Action::await_change() immediately for non-leaders, so standby replicas
    // react instantly once they acquire the lease — without polling.
    if cli.enable_leader_election {
        let grace_secs = cli
            .lease_duration_secs
            .checked_sub(cli.lease_renew_deadline_secs)
            .filter(|g| *g > 0)
            .unwrap_or(DEFAULT_LEASE_GRACE_SECS);

        let holder_id =
            std::env::var("POD_NAME").unwrap_or_else(|_| format!("5spot-{}", cli.instance_id));

        info!(
            lease_name = %cli.lease_name,
            lease_namespace = %cli.lease_namespace,
            lease_duration_secs = cli.lease_duration_secs,
            grace_secs,
            holder_id = %holder_id,
            "Leader election enabled — starting as non-leader"
        );

        // All replicas start as non-leaders; the background task will flip
        // is_leader once the Kubernetes Lease is acquired.
        context.is_leader.store(false, Ordering::Release);

        let manager = kube_lease_manager::LeaseManagerBuilder::new(client.clone(), &cli.lease_name)
            .with_namespace(&cli.lease_namespace)
            .with_duration(cli.lease_duration_secs)
            .with_grace(grace_secs)
            .with_identity(&holder_id)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialise leader election: {e}"))?;

        let is_leader = Arc::clone(&context.is_leader);
        tokio::spawn(async move {
            let (mut channel, task) = manager.watch().await;
            loop {
                if let Ok(()) = channel.changed().await {
                    let acquired = *channel.borrow_and_update();
                    is_leader.store(acquired, Ordering::Release);
                    if acquired {
                        info!(holder_id = %holder_id, "Acquired leadership lease");
                    } else {
                        info!(holder_id = %holder_id, "Lost leadership lease — standby");
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
    } else {
        info!("Leader election disabled — this instance will reconcile all resources");
    }

    // Create API for ScheduledMachine resources
    let scheduled_machines = Api::<ScheduledMachine>::all(client.clone());

    // Verify CRD is installed before starting controller
    if let Err(e) = check_crd_installed(&scheduled_machines).await {
        error!(
            error = %e,
            "ScheduledMachine CRD is not installed. Please install the CRD first:"
        );
        error!("  kubectl apply -f deploy/crds/scheduledmachine.yaml");
        error!("Or generate and apply: cargo run --bin crdgen | kubectl apply -f -");
        return Err(anyhow::anyhow!(
            "Required CRD 'scheduledmachines.5spot.finos.org' is not installed in the cluster"
        ));
    }
    info!("ScheduledMachine CRD verified");

    // Start health and metrics servers
    let health_state_clone = health_state.clone();
    tokio::spawn(async move {
        start_health_server(cli.health_port, health_state_clone).await;
    });
    tokio::spawn(run_metrics_server(cli.metrics_port));

    // Mark controller as ready
    health_state.set_ready(true);

    info!("Starting controller for ScheduledMachine resources");

    // Secondary watches — event-driven reactivity without polling.
    // 1. CAPI Machine (dynamic GVK) — filtered by the scheduled-machine label we
    //    already stamp on every Machine we create. Reverse-mapped via that label.
    // 2. Kubernetes Node — name-matched against every SM's status.nodeRef.name
    //    using a snapshot of the controller's own primary-resource Store.
    let machine_ar = ApiResource::from_gvk_with_plural(
        &GroupVersionKind::gvk(CAPI_GROUP, CAPI_MACHINE_API_VERSION, "Machine"),
        CAPI_RESOURCE_MACHINES,
    );
    let machines_api: Api<DynamicObject> = Api::all_with(client.clone(), &machine_ar);
    let nodes_api: Api<Node> = Api::all(client.clone());

    let controller = Controller::new(scheduled_machines, Config::default());
    let sm_store = controller.store();

    controller
        .watches_with(
            machines_api,
            machine_ar.clone(),
            Config::default().labels(LABEL_SCHEDULED_MACHINE),
            |machine: DynamicObject| machine_to_scheduled_machine(&machine),
        )
        .watches(nodes_api, Config::default(), move |node: Node| {
            let snapshot = sm_store.state();
            node_to_scheduled_machines(&node, snapshot.iter().map(std::convert::AsRef::as_ref))
        })
        .shutdown_on_signal()
        .run(reconcile_scheduled_machine, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(o) => {
                    info!(
                        resource = o.0.name,
                        namespace = ?o.0.namespace,
                        "Reconciliation completed"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Reconciliation error");
                }
            }
        })
        .await;

    info!("Controller shut down");
    Ok(())
}

/// Check if the `ScheduledMachine` CRD is installed in the cluster
async fn check_crd_installed(api: &Api<ScheduledMachine>) -> Result<()> {
    // Try to list resources with limit 0 - this will fail if CRD doesn't exist
    api.list(&ListParams::default().limit(1))
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to access ScheduledMachine resources: {e}. \
                 This usually means the CRD is not installed."
            )
        })?;
    Ok(())
}

/// Run the metrics server on the specified port
async fn run_metrics_server(port: u16) {
    use prometheus::{Encoder, TextEncoder};
    use warp::Filter;

    info!(port = port, "Starting metrics server");

    let metrics = warp::path("metrics").map(|| {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = vec![];

        match encoder.encode(&metric_families, &mut buffer) {
            Ok(()) => warp::reply::with_status(
                String::from_utf8_lossy(&buffer).to_string(),
                warp::http::StatusCode::OK,
            ),
            Err(e) => {
                error!(error = %e, "Failed to encode metrics");
                warp::reply::with_status(
                    format!("# Error encoding metrics: {e}\n"),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
            }
        }
    });

    warp::serve(metrics).run(([0, 0, 0, 0], port)).await;
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
