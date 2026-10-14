// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the hidden `5spot nsenter` subcommand (ADR 0020).
//
// What these cannot cover: actually entering another process's namespaces
// needs CAP_SYS_ADMIN and a target in a different namespace set, so the happy
// path of `enter` is verified on a real host instead (a privileged container
// with hostPID running `5spot nsenter -t 1 ... -- <host-only command>`) and in
// the k0smotron e2e tier. Everything around it is pinned here.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use std::path::Path;

    fn args(mount: bool, uts: bool, ipc: bool, net: bool, pid: bool) -> Args {
        Args {
            target: 1,
            mount,
            uts,
            ipc,
            net,
            pid,
            command: vec!["true".to_string()],
        }
    }

    #[test]
    fn test_all_namespaces_are_entered_with_mount_last() {
        // nsenter's order. The mount namespace goes last so every
        // /proc/<pid>/ns/* path is opened, and every other setns done, while
        // still in the agent's own mount namespace.
        assert_eq!(
            namespaces(&args(true, true, true, true, true)),
            [
                Namespace::Ipc,
                Namespace::Uts,
                Namespace::Net,
                Namespace::Pid,
                Namespace::Mnt
            ]
        );
    }

    #[test]
    fn test_only_the_requested_namespaces_are_entered() {
        assert_eq!(
            namespaces(&args(true, false, false, false, false)),
            [Namespace::Mnt]
        );
        assert_eq!(
            namespaces(&args(false, true, false, true, false)),
            [Namespace::Uts, Namespace::Net]
        );
    }

    #[test]
    fn test_no_namespace_flags_enters_nothing() {
        assert!(namespaces(&args(false, false, false, false, false)).is_empty());
    }

    #[test]
    fn test_namespace_paths_match_procfs() {
        assert_eq!(ns_path(1, Namespace::Mnt), Path::new("/proc/1/ns/mnt"));
        assert_eq!(ns_path(1, Namespace::Uts), Path::new("/proc/1/ns/uts"));
        assert_eq!(ns_path(1, Namespace::Ipc), Path::new("/proc/1/ns/ipc"));
        assert_eq!(ns_path(1, Namespace::Net), Path::new("/proc/1/ns/net"));
        assert_eq!(ns_path(42, Namespace::Pid), Path::new("/proc/42/ns/pid"));
    }

    #[test]
    fn test_a_missing_target_process_is_an_error_not_a_silent_run() {
        // If the namespaces cannot be opened the command must NOT run in the
        // agent's own namespaces instead: that would restart nothing on the
        // host while reporting success.
        let mut a = args(true, false, false, false, false);
        a.target = u32::MAX;
        a.command = vec!["/bin/sh".into(), "-c".into(), "exit 0".into()];
        let err = run(&a).expect_err("an unreachable target must fail");
        assert!(
            format!("{err:#}").contains(&format!("/proc/{}/ns/mnt", u32::MAX))
                || format!("{err:#}").contains("not supported"),
            "error should name the namespace path or the platform: {err:#}"
        );
    }

    #[test]
    fn test_exit_code_passes_through_a_normal_exit() {
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 3"])
            .status()
            .expect("sh runs");
        assert_eq!(exit_code(status), 3);
    }

    #[test]
    fn test_exit_code_maps_a_signal_to_128_plus_signal() {
        // The shell convention, so a SIGKILLed systemctl is not reported as 0.
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .expect("sh runs");
        assert_eq!(exit_code(status), SIGNAL_EXIT_BASE + 15);
    }

    #[test]
    fn test_run_with_no_namespaces_just_runs_the_command() {
        // Degenerate but valid, as with nsenter: nothing to enter, so the
        // command runs where we are and its status comes back.
        let mut a = args(false, false, false, false, false);
        a.command = vec!["/bin/sh".into(), "-c".into(), "exit 7".into()];
        assert_eq!(run(&a).expect("runs"), 7);
    }

    #[test]
    fn test_a_command_that_cannot_be_spawned_is_an_error() {
        let mut a = args(false, false, false, false, false);
        a.command = vec!["/nonexistent/5spot-test-binary".into()];
        assert!(run(&a).is_err());
    }
}
