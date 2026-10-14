// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the `5spot` command-line surface (ADR 0020).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use clap::CommandFactory as _;

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(args)
            .unwrap_or_else(|e| panic!("{args:?} should parse: {e}"))
            .into_command()
    }

    // ---- backward compatibility: bare `5spot` is the controller ----

    #[test]
    fn test_bare_5spot_runs_the_controller() {
        // Existing controller Deployments run the image with no arguments.
        assert!(matches!(parse(&["5spot"]), Command::Controller(_)));
    }

    #[test]
    fn test_bare_5spot_accepts_controller_flags() {
        let Command::Controller(args) = parse(&["5spot", "--metrics-port", "9100"]) else {
            panic!("expected the controller");
        };
        assert_eq!(args.metrics_port, 9100);
    }

    #[test]
    fn test_controller_flags_cannot_be_combined_with_another_subcommand() {
        // A controller flag in front of `provider time-based` is a mistake,
        // not something to silently ignore.
        assert!(
            Cli::try_parse_from(["5spot", "--metrics-port", "1", "provider", "time-based"])
                .is_err()
        );
    }

    // ---- one subcommand per component ----

    #[test]
    fn test_controller_subcommand() {
        let Command::Controller(args) = parse(&["5spot", "controller", "--instance-count", "3"])
        else {
            panic!("expected the controller");
        };
        assert_eq!(args.instance_count, 3);
    }

    #[test]
    fn test_capacity_controller_subcommand() {
        let Command::CapacityController(args) =
            parse(&["5spot", "capacity-controller", "--lease-name", "cap-lease"])
        else {
            panic!("expected the capacity controller");
        };
        assert_eq!(args.lease_name, "cap-lease");
    }

    #[test]
    fn test_provider_time_based_subcommand() {
        let Command::Provider(ProviderCommand::TimeBased(args)) =
            parse(&["5spot", "provider", "time-based", "--metrics-port", "9201"])
        else {
            panic!("expected the time-based provider");
        };
        assert_eq!(args.metrics_port, 9201);
    }

    #[test]
    fn test_provider_capital_markets_subcommand() {
        let Command::Provider(ProviderCommand::CapitalMarkets(args)) = parse(&[
            "5spot",
            "provider",
            "capital-markets",
            "--metrics-port",
            "9202",
        ]) else {
            panic!("expected the capital-markets provider");
        };
        assert_eq!(args.metrics_port, 9202);
    }

    #[test]
    fn test_agent_reclaim_subcommand() {
        let Command::Agent(AgentCommand::Reclaim(args)) =
            parse(&["5spot", "agent", "reclaim", "--node-name", "node-a"])
        else {
            panic!("expected the reclaim agent");
        };
        assert_eq!(args.node_name, "node-a");
    }

    #[test]
    fn test_agent_kata_config_subcommand() {
        let Command::Agent(AgentCommand::KataConfig(args)) = parse(&[
            "5spot",
            "agent",
            "kata-config",
            "--node-name",
            "node-b",
            "--oneshot",
        ]) else {
            panic!("expected the kata-config agent");
        };
        assert_eq!(args.node_name, "node-b");
        assert!(args.oneshot);
    }

    #[test]
    fn test_provider_without_a_kind_is_an_error() {
        assert!(Cli::try_parse_from(["5spot", "provider"]).is_err());
    }

    #[test]
    fn test_unknown_subcommand_is_an_error() {
        assert!(Cli::try_parse_from(["5spot", "spot-schedule-time-based"]).is_err());
    }

    // ---- the hidden nsenter subcommand ----

    #[test]
    fn test_nsenter_parses_the_kata_restart_command_line() {
        // Exactly what nsenter_restart_argv builds, minus argv[0].
        let Command::Nsenter(args) = parse(&[
            "5spot",
            "nsenter",
            "-t",
            "1",
            "-m",
            "-u",
            "-i",
            "-n",
            "-p",
            "--",
            "systemctl",
            "restart",
            "--",
            "k0sworker.service",
        ]) else {
            panic!("expected nsenter");
        };
        assert_eq!(args.target, 1);
        assert!(args.mount && args.uts && args.ipc && args.net && args.pid);
        // The SECOND `--` belongs to systemctl (ADR 0012) and must survive.
        assert_eq!(
            args.command,
            ["systemctl", "restart", "--", "k0sworker.service"]
        );
    }

    #[test]
    fn test_nsenter_requires_a_command() {
        assert!(Cli::try_parse_from(["5spot", "nsenter", "-t", "1", "-m"]).is_err());
    }

    #[test]
    fn test_nsenter_requires_a_target() {
        assert!(Cli::try_parse_from(["5spot", "nsenter", "-m", "--", "true"]).is_err());
    }

    #[test]
    fn test_nsenter_is_hidden_from_help() {
        let help = Cli::command().render_help().to_string();
        assert!(!help.contains("nsenter"), "nsenter is internal: {help}");
        for visible in ["controller", "capacity-controller", "provider", "agent"] {
            assert!(
                help.contains(visible),
                "{visible} missing from help: {help}"
            );
        }
    }

    #[test]
    fn test_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
