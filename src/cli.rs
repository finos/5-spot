// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! The `5spot` command line (ADR 0020).
//!
//! One binary runs every 5-Spot component, chosen by subcommand. Bare `5spot`,
//! with no subcommand, runs the controller, so controller Deployments that
//! predate the subcommands keep working unchanged.

use clap::{Parser, Subcommand};

use crate::cmd;

/// Kubernetes operator for time-based machine scheduling.
#[derive(Parser, Debug)]
#[command(name = "5spot", version, args_conflicts_with_subcommands = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Controller flags, accepted without a subcommand for compatibility.
    #[command(flatten)]
    controller: cmd::controller::Args,
}

impl Cli {
    /// The component to run: the subcommand, or the controller if none.
    pub(crate) fn into_command(self) -> Command {
        self.command.unwrap_or(Command::Controller(self.controller))
    }
}

/// One 5-Spot component per subcommand.
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// The ScheduledMachine controller (the default)
    Controller(cmd::controller::Args),

    /// The ScheduledCapacity controller (ADR 0011)
    CapacityController(cmd::capacity_controller::Args),

    /// A first-party spot-schedule provider (ADR 0009)
    #[command(subcommand)]
    Provider(ProviderCommand),

    /// A node-side agent on the workload cluster
    #[command(subcommand)]
    Agent(AgentCommand),

    /// Run a command in another process's namespaces (internal; used by the
    /// kata-config agent to restart the host k0s service)
    #[command(hide = true)]
    Nsenter(cmd::nsenter::Args),
}

/// First-party spot-schedule providers.
#[derive(Subcommand, Debug)]
pub(crate) enum ProviderCommand {
    /// TimeBasedSpotSchedule: active inside a day/hour window
    TimeBased(cmd::provider_time_based::Args),

    /// CapitalMarketsSchedule: active outside trading sessions
    CapitalMarkets(cmd::provider_capital_markets::Args),
}

/// Node-side agents.
#[derive(Subcommand, Debug)]
pub(crate) enum AgentCommand {
    /// Emergency reclaim trigger
    Reclaim(cmd::reclaim_agent::Args),

    /// Kata containerd drop-in delivery (privileged)
    KataConfig(cmd::kata_config_agent::Args),
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
