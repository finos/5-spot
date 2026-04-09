// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
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

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use five_spot::constants::{HEALTH_PORT, K8S_API_TIMEOUT_SECS, METRICS_PORT};
use five_spot::crd::ScheduledMachine;
use five_spot::health::{start_health_server, HealthState};
use five_spot::metrics::init_controller_info;
use five_spot::reconcilers::{error_policy, reconcile_scheduled_machine, Context};
use futures::StreamExt;
use kube::{
    api::ListParams,
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
}

/// Async entry point.
///
/// Initialises the controller and blocks until the process receives a shutdown
/// signal (SIGTERM / SIGINT).  Returns an error if:
/// - The Kubernetes client cannot be configured
/// - The `ScheduledMachine` CRD is not present in the cluster
#[tokio::main]
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
    let context = Arc::new(Context::new(
        client.clone(),
        cli.instance_id,
        cli.instance_count,
    ));

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
            "Required CRD 'scheduledmachines.5spot.io' is not installed in the cluster"
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

    // Run the controller
    Controller::new(scheduled_machines, Config::default())
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
