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

    // ---- detector resolution (ADR 0021) ----
    //
    // The netlink proc connector only delivers to the host's initial network
    // namespace, and agent pods may not use hostNetwork (ADR 0004). Measured:
    // Subscriber::new() fails with ECONNREFUSED from any other namespace. So
    // `auto` must fall back to polling instead of exiting, which used to
    // crash-loop the shipped DaemonSet on every Linux node.

    /// `ECONNREFUSED` on Linux, as observed from a non-initial network namespace.
    #[cfg(target_os = "linux")]
    const ECONNREFUSED_FOR_TESTS: i32 = 111;

    #[cfg(target_os = "linux")]
    fn connection_refused() -> NetlinkError {
        NetlinkError::Io(std::io::Error::from_raw_os_error(ECONNREFUSED_FOR_TESTS))
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_auto_on_linux_falls_back_to_poll_when_netlink_cannot_open() {
        let detector = DetectorFlag::Auto.resolve_with(true, || Err(connection_refused()));
        assert_eq!(detector, Detector::Poll);
    }

    #[test]
    fn test_auto_on_linux_uses_netlink_when_it_opens() {
        assert_eq!(
            DetectorFlag::Auto.resolve_with(true, || Ok(())),
            Detector::Netlink
        );
    }

    #[test]
    fn test_auto_off_linux_polls_without_probing() {
        let detector = DetectorFlag::Auto
            .resolve_with(false, || panic!("netlink must not be probed off Linux"));
        assert_eq!(detector, Detector::Poll);
    }

    #[test]
    fn test_explicit_netlink_is_honoured_without_a_probe() {
        // An operator who asked for netlink gets it, and its failure stays loud
        // at run time rather than being silently downgraded here.
        let detector = DetectorFlag::Netlink
            .resolve_with(true, || panic!("an explicit choice must not be probed"));
        assert_eq!(detector, Detector::Netlink);
    }

    #[test]
    fn test_explicit_poll_is_honoured_without_a_probe() {
        let detector = DetectorFlag::Poll
            .resolve_with(true, || panic!("an explicit choice must not be probed"));
        assert_eq!(detector, Detector::Poll);
    }

    #[test]
    fn test_unsupported_platform_error_also_falls_back() {
        let detector = DetectorFlag::Auto.resolve_with(true, || Err(NetlinkError::Unsupported));
        assert_eq!(detector, Detector::Poll);
    }

    // ---- the Node annotation patch (bug-014) ----
    //
    // The agent used `PatchParams::apply(..).force()` with `Patch::Merge`. kube
    // refuses that pairing before sending anything ("PatchParams::force only
    // works with Patch::Apply"), so every match ended in an error and a crash:
    // the agent could never annotate its Node. Found by the k0smotron e2e tier.
    // kube's own validator is crate-private; its public request builder runs
    // the same check, so these tests go through it.

    fn node_patch_request(params: &PatchParams) -> Result<(), kube::core::request::Error> {
        let body = serde_json::json!({"metadata": {"annotations": {"example": "value"}}});
        kube::core::Request::new("/api/v1/nodes")
            .patch("node-a", params, &Patch::Merge(&body))
            .map(drop)
    }

    #[test]
    fn test_annotation_patch_passes_kube_request_validation() {
        node_patch_request(&annotation_patch_params()).expect("a valid Node patch request");
    }

    #[test]
    fn test_annotation_patch_names_its_field_manager_and_does_not_force() {
        let params = annotation_patch_params();
        assert_eq!(params.field_manager.as_deref(), Some(FIELD_MANAGER));
        assert!(!params.force, "force is only valid for server-side apply");
    }

    #[test]
    fn test_a_forced_merge_patch_is_rejected_by_kube() {
        // The exact pairing that broke the agent. If kube ever starts
        // accepting it this test says so, rather than the bug silently
        // becoming load-bearing.
        let err = node_patch_request(&PatchParams::apply(FIELD_MANAGER).force())
            .expect_err("force with a merge patch must be refused");
        assert!(
            err.to_string()
                .contains("force only works with Patch::Apply"),
            "{err}"
        );
    }

    // ------------------------------------------------------------------
    // --oneshot waits for the ConfigMap watcher's initial list (bug-016).
    // ------------------------------------------------------------------
    // The scanner used to read the config channel right after spawning the
    // watcher, so a oneshot run always saw `None` and exited "no configmap
    // observed", even with the ConfigMap present. Found by the k0smotron e2e.

    use std::time::Duration;
    use tokio::sync::watch;

    /// Long enough never to be the reason a passing case passes.
    const GENEROUS: Duration = Duration::from_secs(5);
    /// Short: the case it bounds has nothing that could ever arrive.
    const BRIEF: Duration = Duration::from_millis(20);

    #[tokio::test]
    async fn test_initial_sync_already_done_returns_at_once() {
        let (_tx, mut rx) = watch::channel(true);
        assert!(await_initial_sync(&mut rx, BRIEF).await);
    }

    #[tokio::test]
    async fn test_initial_sync_waits_for_the_watcher_to_finish_listing() {
        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            tx.send(true).expect("receiver alive");
            // Keep the sender alive until the receiver has looked.
            tokio::time::sleep(GENEROUS).await;
        });
        assert!(await_initial_sync(&mut rx, GENEROUS).await);
    }

    #[tokio::test]
    async fn test_initial_sync_gives_up_after_the_timeout() {
        let (_tx, mut rx) = watch::channel(false);
        assert!(!await_initial_sync(&mut rx, BRIEF).await);
    }

    #[tokio::test]
    async fn test_initial_sync_fails_when_the_watcher_is_gone() {
        let (tx, mut rx) = watch::channel(false);
        drop(tx);
        assert!(!await_initial_sync(&mut rx, GENEROUS).await);
    }

    /// Both ends of the agent's two channels, as `run` wires them.
    struct Channels {
        config_tx: watch::Sender<Option<Config>>,
        config_rx: watch::Receiver<Option<Config>>,
        synced_tx: watch::Sender<bool>,
        synced_rx: watch::Receiver<bool>,
    }

    fn channels() -> Channels {
        let (config_tx, config_rx) = watch::channel(None);
        let (synced_tx, synced_rx) = watch::channel(false);
        Channels {
            config_tx,
            config_rx,
            synced_tx,
            synced_rx,
        }
    }

    #[test]
    fn test_init_done_marks_the_initial_list_complete() {
        let Channels {
            config_tx,
            config_rx,
            synced_tx,
            synced_rx,
        } = channels();
        apply_event(watcher::Event::InitDone, &config_tx, &synced_tx, "cm");
        assert!(*synced_rx.borrow());
        assert!(config_rx.borrow().is_none(), "InitDone carries no config");
    }

    #[test]
    fn test_init_start_does_not_mark_the_list_complete() {
        let Channels {
            config_tx,
            config_rx: _,
            synced_tx,
            synced_rx,
        } = channels();
        apply_event(watcher::Event::Init, &config_tx, &synced_tx, "cm");
        assert!(!*synced_rx.borrow());
    }

    #[test]
    fn test_an_initial_configmap_without_data_leaves_the_agent_idle() {
        let Channels {
            config_tx,
            config_rx,
            synced_tx,
            synced_rx,
        } = channels();
        apply_event(
            watcher::Event::InitApply(ConfigMap::default()),
            &config_tx,
            &synced_tx,
            "cm",
        );
        assert!(config_rx.borrow().is_none());
        assert!(!*synced_rx.borrow(), "only InitDone completes the list");
    }

    #[test]
    fn test_a_deleted_configmap_disarms_the_agent() {
        let Channels {
            config_tx,
            config_rx,
            synced_tx,
            synced_rx: _,
        } = channels();
        let armed: Config =
            toml::from_str("match_commands = [\"e2ereclaim\"]").expect("valid reclaim.toml");
        config_tx.send(Some(armed)).expect("receiver alive");
        apply_event(
            watcher::Event::Delete(ConfigMap::default()),
            &config_tx,
            &synced_tx,
            "cm",
        );
        assert!(config_rx.borrow().is_none());
    }
}
