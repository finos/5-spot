// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # `spot-schedule-capital-markets`
//!
//! Standalone controller binary for the **reference** spot-schedule provider
//! (ADR 0006, roadmap Phase 5). It reconciles
//! [`CapitalMarketsSchedule`](five_spot::crd::CapitalMarketsSchedule) objects in
//! the `spotschedules.5spot.finos.org` group and publishes the duck-typed
//! `status.active` that a `ScheduledMachine.spec.spotSchedule` consumes.
//!
//! Activity is computed from a declarative exchange calendar (trading sessions,
//! statutory holidays, early-close days) evaluated in the schedule's timezone —
//! no network calls; operators sync real calendars into `spec` via GitOps. The
//! controller is event-driven: it watches the CRD and requeues each object once
//! at its next calendar boundary (see `five_spot::providers::capital_markets`).

use anyhow::{Context as _, Result};
use clap::Parser;
use kube::Client;
use tracing::info;

use five_spot::metrics;
use five_spot::providers::capital_markets;

/// Default port for the provider's Prometheus `/metrics` endpoint — matches the
/// controller's `METRICS_PORT` convention.
const DEFAULT_METRICS_PORT: u16 = 8080;

/// CLI / environment configuration for the CapitalMarketsSchedule provider.
#[derive(Debug, Parser)]
#[command(
    name = "spot-schedule-capital-markets",
    about = "Reference spot-schedule provider: computes CapitalMarketsSchedule.status.active from an exchange calendar"
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
        "spot-schedule-capital-markets provider started"
    );

    let client = Client::try_default()
        .await
        .context("building in-cluster kube client")?;

    tokio::spawn(metrics::serve_metrics(cli.metrics_port));

    capital_markets::run(client).await
}
