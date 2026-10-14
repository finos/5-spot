// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    // The constants come in through the module's own imports.
    use super::super::*;
    use crate::cli::{Cli, Command};
    use clap::Parser as _;

    /// Parse `5spot capacity-controller <extra>` through the real command line
    /// (ADR 0020), and return the capacity controller's arguments.
    fn parse(extra: &[&str]) -> Args {
        let argv = ["5spot", "capacity-controller"]
            .into_iter()
            .chain(extra.iter().copied());
        match Cli::try_parse_from(argv).expect("parses").into_command() {
            Command::CapacityController(args) => args,
            other => panic!("expected the capacity controller, got {other:?}"),
        }
    }

    /// Parse with no arguments, as the Deployment does when it relies on
    /// defaults and environment variables.
    fn defaults() -> Args {
        parse(&[])
    }

    #[test]
    fn test_cli_defaults_match_the_shared_constants() {
        let cli = defaults();
        assert_eq!(cli.metrics_port, METRICS_PORT);
        assert_eq!(cli.health_port, HEALTH_PORT);
        assert_eq!(cli.lease_namespace, DEFAULT_LEASE_NAMESPACE);
        assert_eq!(cli.lease_duration_secs, DEFAULT_LEASE_DURATION_SECS);
        assert_eq!(
            cli.lease_renew_deadline_secs,
            DEFAULT_LEASE_RENEW_DEADLINE_SECS
        );
    }

    /// Leader election defaults OFF, which is what makes a single-replica
    /// Deployment and a local run against a kubeconfig both work without
    /// contending for a Lease the ServiceAccount may not even be able to create.
    #[test]
    fn test_leader_election_defaults_off() {
        assert!(!defaults().enable_leader_election);
    }

    /// The capacity controller must not share the machine controller's Lease.
    /// Sharing one would couple two deliberately separate identities'
    /// availability: a capacity controller holding the lease would silence the
    /// machine controller, which has nothing to do with capacity (ADR 0011).
    #[test]
    fn test_default_lease_name_differs_from_the_machine_controller() {
        assert_eq!(defaults().lease_name, DEFAULT_CAPACITY_LEASE_NAME);
        assert_ne!(
            defaults().lease_name,
            five_spot::constants::DEFAULT_LEASE_NAME,
            "the two controllers must contend for different Leases"
        );
    }

    #[test]
    fn test_cli_accepts_explicit_overrides() {
        let cli = parse(&[
            "--metrics-port",
            "9090",
            "--health-port",
            "9091",
            "--enable-leader-election",
            "--lease-name",
            "custom-lease",
            "--lease-namespace",
            "other-ns",
            "--lease-duration-secs",
            "30",
            "--lease-renew-deadline-secs",
            "20",
        ]);
        assert_eq!(cli.metrics_port, 9090);
        assert_eq!(cli.health_port, 9091);
        assert!(cli.enable_leader_election);
        assert_eq!(cli.lease_name, "custom-lease");
        assert_eq!(cli.lease_namespace, "other-ns");
        assert_eq!(cli.lease_duration_secs, 30);
        assert_eq!(cli.lease_renew_deadline_secs, 20);
    }

    /// The default lease parameters must leave a positive grace period, or the
    /// lease manager would treat every renewal as already late.
    #[test]
    fn test_default_lease_parameters_leave_a_positive_grace() {
        let cli = defaults();
        let config = LeaderElectionConfig {
            lease_name: cli.lease_name,
            lease_namespace: cli.lease_namespace,
            lease_duration_secs: cli.lease_duration_secs,
            lease_renew_deadline_secs: cli.lease_renew_deadline_secs,
            fallback_identity: DEFAULT_CAPACITY_LEASE_NAME.to_string(),
        };
        assert!(config.grace_secs() > 0);
        assert!(config.lease_renew_deadline_secs < config.lease_duration_secs);
    }

    /// `--help` must build: a clap definition with a duplicate long flag or a
    /// bad default panics at parse time, and a controller that cannot start is
    /// worse than one that misbehaves. Checked over the whole `5spot` command,
    /// since every subcommand's flags now share one definition.
    #[test]
    fn test_cli_definition_is_valid() {
        use clap::CommandFactory as _;
        Cli::command().debug_assert();
    }
}
