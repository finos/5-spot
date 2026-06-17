// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # `spot-schedule-time-based`
//!
//! Standalone controller binary for the **core/default** spot-schedule provider
//! (ADR 0009). It reconciles
//! [`TimeBasedSpotSchedule`](five_spot::crd::TimeBasedSpotSchedule) objects in
//! the `spotschedules.5spot.finos.org` group and publishes the duck-typed
//! `status.active` that a `ScheduledMachine.spec.schedule` consumes.
//!
//! Activity is computed from a declarative day-of-week / hour-of-day window
//! evaluated in the schedule's timezone — the reified former inline
//! `spec.schedule`. No network calls; the controller is event-driven and
//! requeues each object once at its next window boundary (see
//! `five_spot::providers::time_based`).

use anyhow::{Context as _, Result};
use clap::Parser;
use kube::Client;
use tracing::info;

use five_spot::metrics;
use five_spot::providers::time_based;

/// Default port for the provider's Prometheus `/metrics` endpoint — matches the
/// controller's `METRICS_PORT` convention.
const DEFAULT_METRICS_PORT: u16 = 8080;

/// CLI / environment configuration for the TimeBasedSpotSchedule provider.
#[derive(Debug, Parser)]
#[command(
    name = "spot-schedule-time-based",
    about = "Core spot-schedule provider: computes TimeBasedSpotSchedule.status.active from a day/hour window"
)]
struct Cli {
    /// Port for the Prometheus `/metrics` endpoint.
    #[arg(long, env = "METRICS_PORT", default_value_t = DEFAULT_METRICS_PORT)]
    metrics_port: u16,
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
        "spot-schedule-time-based provider started"
    );

    let client = Client::try_default()
        .await
        .context("building in-cluster kube client")?;

    tokio::spawn(metrics::serve_metrics(cli.metrics_port));

    time_based::run(client).await
}
