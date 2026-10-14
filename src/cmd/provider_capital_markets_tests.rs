// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the `provider capital-markets` subcommand's arguments (ADR 0020).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::cli::{Cli, Command, ProviderCommand};
    use clap::Parser as _;

    fn parse(extra: &[&str]) -> Result<Args, clap::Error> {
        let argv = ["5spot", "provider", "capital-markets"]
            .into_iter()
            .chain(extra.iter().copied());
        match Cli::try_parse_from(argv)?.into_command() {
            Command::Provider(ProviderCommand::CapitalMarkets(args)) => Ok(args),
            other => panic!("expected the capital-markets provider, got {other:?}"),
        }
    }

    #[test]
    fn test_metrics_port_defaults() {
        assert_eq!(
            parse(&[]).expect("parses").metrics_port,
            DEFAULT_METRICS_PORT
        );
    }

    #[test]
    fn test_metrics_port_override() {
        assert_eq!(
            parse(&["--metrics-port", "9300"])
                .expect("parses")
                .metrics_port,
            9300
        );
    }

    #[test]
    fn test_controller_flags_are_not_provider_flags() {
        // Each subcommand owns its flags; the controller's are not inherited.
        assert!(parse(&["--instance-count", "2"]).is_err());
    }
}
