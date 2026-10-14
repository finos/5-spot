// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the `agent reclaim` subcommand's arguments (ADR 0020).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::cli::{AgentCommand, Cli, Command};
    use clap::Parser as _;

    fn parse(extra: &[&str]) -> Result<Args, clap::Error> {
        let argv = ["5spot", "agent", "reclaim"]
            .into_iter()
            .chain(extra.iter().copied());
        match Cli::try_parse_from(argv)?.into_command() {
            Command::Agent(AgentCommand::Reclaim(args)) => Ok(args),
            other => panic!("expected the reclaim agent, got {other:?}"),
        }
    }

    #[test]
    fn test_defaults() {
        let args = parse(&["--node-name", "n1"]).expect("parses");
        assert_eq!(args.node_name, "n1");
        assert_eq!(args.proc_root, PathBuf::from(DEFAULT_PROC_ROOT));
        assert_eq!(args.machine_id_path, PathBuf::from(DEFAULT_MACHINE_ID_PATH));
        assert!(matches!(args.detector, DetectorFlag::Auto));
        assert!(!args.oneshot);
        assert!(
            !args.skip_host_id_check,
            "the host-identity check is on by default"
        );
    }

    #[test]
    fn test_node_name_is_required() {
        // The DaemonSet supplies it from the downward API; without it the agent
        // would not know which Node to annotate.
        if std::env::var_os("NODE_NAME").is_none() {
            assert!(parse(&[]).is_err());
        }
    }

    #[test]
    fn test_detector_values() {
        for (value, expect_netlink, expect_poll) in
            [("netlink", true, false), ("poll", false, true)]
        {
            let args = parse(&["--node-name", "n", "--detector", value]).expect("parses");
            assert_eq!(
                matches!(args.detector, DetectorFlag::Netlink),
                expect_netlink
            );
            assert_eq!(matches!(args.detector, DetectorFlag::Poll), expect_poll);
        }
        assert!(parse(&["--node-name", "n", "--detector", "ebpf"]).is_err());
    }
}
