// Main entry point for 5-Spot Machine Scheduler

use anyhow::Result;
use clap::Parser;
use five_spot::constants::{HEALTH_PORT, METRICS_PORT};
use five_spot::crd::ScheduledMachine;
use five_spot::reconcilers::{error_policy, reconcile_scheduled_machine, Context};
use futures::StreamExt;
use kube::{
    runtime::{watcher::Config, Controller},
    Api, Client,
};
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing/logging
    let log_level = if cli.verbose {
        "debug,kube=info,hyper=info,tower=info"
    } else {
        "info,kube=warn,hyper=warn,tower=warn"
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!(
        instance_id = cli.instance_id,
        instance_count = cli.instance_count,
        metrics_port = cli.metrics_port,
        health_port = cli.health_port,
        "Starting 5-Spot Machine Scheduler"
    );

    // Create Kubernetes client
    let client = Client::try_default().await?;
    info!("Connected to Kubernetes API");

    // Create shared context
    let context = Arc::new(Context::new(
        client.clone(),
        cli.instance_id,
        cli.instance_count,
    ));

    // Create API for ScheduledMachine resources
    let scheduled_machines = Api::<ScheduledMachine>::all(client.clone());

    // Start health and metrics servers
    tokio::spawn(start_health_server(cli.health_port));
    tokio::spawn(run_metrics_server(cli.metrics_port));

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

/// Start health check server
async fn start_health_server(port: u16) {
    use warp::Filter;

    info!(port = port, "Starting health check server");

    let health =
        warp::path("health").map(|| warp::reply::with_status("OK", warp::http::StatusCode::OK));

    let ready =
        warp::path("ready").map(|| warp::reply::with_status("OK", warp::http::StatusCode::OK));

    let routes = health.or(ready);

    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
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
