// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the `agent kata-config` subcommand's arguments (ADR 0020).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::cli::{AgentCommand, Cli, Command};
    use clap::Parser as _;

    fn parse(extra: &[&str]) -> Result<Args, clap::Error> {
        let argv = ["5spot", "agent", "kata-config"]
            .into_iter()
            .chain(extra.iter().copied());
        match Cli::try_parse_from(argv)?.into_command() {
            Command::Agent(AgentCommand::KataConfig(args)) => Ok(args),
            other => panic!("expected the kata-config agent, got {other:?}"),
        }
    }

    #[test]
    fn test_defaults() {
        let args = parse(&["--node-name", "n1"]).expect("parses");
        assert_eq!(args.host_root, PathBuf::from("/host"));
        assert_eq!(args.poll_interval_secs, DEFAULT_POLL_INTERVAL_SECS);
        assert_eq!(args.metrics_port, DEFAULT_METRICS_PORT);
        assert!(!args.oneshot);
    }

    #[test]
    fn test_node_name_is_required() {
        if std::env::var_os("NODE_NAME").is_none() {
            assert!(parse(&[]).is_err());
        }
    }

    #[test]
    fn test_restart_reexecutes_this_binary() {
        // The executor's program is the running binary itself, so the restart
        // works in an image with no nsenter (ADR 0020).
        assert_eq!(SELF_EXE, "/proc/self/exe");
        let argv = nsenter_restart_argv(SELF_EXE, "k0sworker.service");
        assert_eq!(&argv[..2], ["/proc/self/exe", "nsenter"]);
    }
}
