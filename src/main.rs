// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! `5spot`: every 5-Spot runtime component in one binary (ADR 0020).
//!
//! See [`cli`] for the subcommands and [`cmd`] for the components.

mod cli;
mod cmd;

use anyhow::{Context as _, Result};
use clap::Parser as _;

use crate::cli::{AgentCommand, Cli, Command, ProviderCommand};

fn main() -> Result<()> {
    let command = Cli::parse().into_command();

    // `nsenter` must run before any thread exists: setns(2) into a mount
    // namespace fails in a multi-threaded process. So it is dispatched here,
    // before the async runtime is built, and exits with the command's status.
    if let Command::Nsenter(args) = &command {
        std::process::exit(cmd::nsenter::run(args)?);
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the async runtime")?
        .block_on(run(command))
}

/// Run the selected long-lived component.
async fn run(command: Command) -> Result<()> {
    match command {
        Command::Controller(args) => cmd::controller::run(args).await,
        Command::CapacityController(args) => cmd::capacity_controller::run(args).await,
        Command::Provider(ProviderCommand::TimeBased(args)) => {
            cmd::provider_time_based::run(args).await
        }
        Command::Provider(ProviderCommand::CapitalMarkets(args)) => {
            cmd::provider_capital_markets::run(args).await
        }
        Command::Agent(AgentCommand::Reclaim(args)) => cmd::reclaim_agent::run(args).await,
        Command::Agent(AgentCommand::KataConfig(args)) => cmd::kata_config_agent::run(args).await,
        // Handled in `main` before the runtime exists; reaching it here would
        // mean entering a mount namespace from a multi-threaded process.
        Command::Nsenter(_) => anyhow::bail!("nsenter must run before the async runtime starts"),
    }
}
