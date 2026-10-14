// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the controller subcommand's arguments (ADR 0020).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::cli::{Cli, Command};
    use clap::Parser as _;

    fn parse(argv: &[&str]) -> Args {
        match Cli::try_parse_from(argv).expect("parses").into_command() {
            Command::Controller(args) => args,
            other => panic!("expected the controller, got {other:?}"),
        }
    }

    #[test]
    fn test_defaults_match_the_shared_constants() {
        let args = parse(&["5spot"]);
        assert_eq!(args.instance_id, 0);
        assert_eq!(args.instance_count, 1);
        assert_eq!(args.metrics_port, METRICS_PORT);
        assert_eq!(args.health_port, HEALTH_PORT);
        assert_eq!(args.lease_name, DEFAULT_LEASE_NAME);
        assert_eq!(args.lease_namespace, DEFAULT_LEASE_NAMESPACE);
        assert_eq!(args.lease_duration_secs, DEFAULT_LEASE_DURATION_SECS);
        assert_eq!(
            args.lease_renew_deadline_secs,
            DEFAULT_LEASE_RENEW_DEADLINE_SECS
        );
        assert_eq!(args.log_format, "json");
    }

    #[test]
    fn test_safety_defaults() {
        // Leader election off for single-replica and local runs; finalizer
        // force-removal on, so a stuck cleanup cannot wedge namespace deletion.
        let args = parse(&["5spot"]);
        assert!(!args.enable_leader_election);
        assert!(args.force_finalizer_on_timeout);
        assert!(!args.verbose);
    }

    #[test]
    fn test_bare_and_explicit_controller_parse_identically() {
        let flags = [
            "--instance-id",
            "2",
            "--instance-count",
            "4",
            "--enable-leader-election",
        ];
        let bare = parse(&[["5spot"].as_slice(), flags.as_slice()].concat());
        let explicit = parse(&[["5spot", "controller"].as_slice(), flags.as_slice()].concat());
        assert_eq!((bare.instance_id, bare.instance_count), (2, 4));
        assert_eq!((explicit.instance_id, explicit.instance_count), (2, 4));
        assert!(bare.enable_leader_election && explicit.enable_leader_election);
    }

    #[test]
    fn test_a_non_numeric_port_is_rejected() {
        assert!(Cli::try_parse_from(["5spot", "controller", "--metrics-port", "http"]).is_err());
    }
}
