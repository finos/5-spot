// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    // ========================================================================
    // Helpers — build a synthetic /proc tree under a tempdir
    // ========================================================================

    /// Write a fake `/proc/<pid>/comm` + `/proc/<pid>/cmdline` pair.
    /// cmdline uses NUL separators, matching the real kernel format.
    fn write_proc_entry(proc_root: &Path, pid: u32, comm: &str, argv: &[&str]) {
        let pid_dir = proc_root.join(pid.to_string());
        fs::create_dir_all(&pid_dir).expect("create pid dir");
        // /proc/<pid>/comm is the executable basename with a trailing newline
        fs::write(pid_dir.join("comm"), format!("{comm}\n")).expect("write comm");
        // /proc/<pid>/cmdline is NUL-separated argv, NUL-terminated
        let mut buf = argv.join("\0");
        buf.push('\0');
        fs::write(pid_dir.join("cmdline"), buf).expect("write cmdline");
    }

    fn base_config() -> Config {
        Config {
            match_commands: vec!["java".to_string(), "idea".to_string()],
            match_argv_substrings: vec!["intellij".to_string()],
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
        }
    }

    // ========================================================================
    // Config parsing
    // ========================================================================

    #[test]
    fn test_config_parse_happy_path() {
        let toml_str = r#"
            match_commands = ["java", "idea", "steam"]
            match_argv_substrings = ["intellij", "blender"]
            poll_interval_ms = 100
        "#;
        let config = parse_config(toml_str).expect("parse");
        assert_eq!(config.match_commands, vec!["java", "idea", "steam"]);
        assert_eq!(config.match_argv_substrings, vec!["intellij", "blender"]);
        assert_eq!(config.poll_interval_ms, 100);
    }

    #[test]
    fn test_config_defaults_applied_when_fields_missing() {
        let toml_str = r#"
            match_commands = ["java"]
        "#;
        let config = parse_config(toml_str).expect("parse");
        assert_eq!(config.match_commands, vec!["java"]);
        assert!(
            config.match_argv_substrings.is_empty(),
            "absent argv list must default to empty"
        );
        assert_eq!(
            config.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS,
            "absent poll interval must default to DEFAULT_POLL_INTERVAL_MS"
        );
    }

    #[test]
    fn test_config_empty_patterns_is_valid_but_inert() {
        // An empty config is well-formed; the caller decides that an agent
        // with nothing to match is a no-op rather than an error.
        let config = parse_config("").expect("empty TOML must parse");
        assert!(config.match_commands.is_empty());
        assert!(config.match_argv_substrings.is_empty());
        assert_eq!(config.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
    }

    #[test]
    fn test_config_rejects_malformed_toml() {
        let err = parse_config("match_commands = not-a-list")
            .expect_err("malformed TOML must be rejected");
        assert!(
            err.to_string().to_lowercase().contains("toml")
                || err.to_string().to_lowercase().contains("parse")
                || err.to_string().to_lowercase().contains("expected"),
            "error message should name the parse failure, got: {err}"
        );
    }

    #[test]
    fn test_config_rejects_zero_poll_interval() {
        // A 0-ms poll interval means "spin forever burning CPU" — almost
        // certainly a user typo. Rejecting early beats a silent CPU leak.
        let toml_str = r#"
            match_commands = ["java"]
            poll_interval_ms = 0
        "#;
        let err = parse_config(toml_str).expect_err("0 poll interval must be rejected");
        assert!(
            err.to_string().to_lowercase().contains("poll"),
            "error must name the field, got: {err}"
        );
    }

    #[test]
    fn test_load_config_from_file() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("reclaim.toml");
        fs::write(
            &path,
            "match_commands = [\"java\"]\nmatch_argv_substrings = []\npoll_interval_ms = 250\n",
        )
        .expect("write config");
        let config = load_config(&path).expect("load");
        assert_eq!(config.match_commands, vec!["java"]);
    }

    #[test]
    fn test_load_config_missing_file_is_error() {
        let tmp = TempDir::new().expect("tempdir");
        let err = load_config(&tmp.path().join("does-not-exist.toml"))
            .expect_err("missing config file must error");
        // Callers (systemd unit, container entrypoint) need a clear error so
        // they can surface a ConfigMap-projection failure rather than
        // silently running with an empty match list.
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("not found")
                || msg.contains("no such")
                || msg.contains("cannot")
                || msg.contains("read"),
            "error must indicate file access failure, got: {err}"
        );
    }

    // ========================================================================
    // Detection — /proc scanning
    // ========================================================================

    #[test]
    fn test_scan_proc_matches_exact_comm() {
        let tmp = TempDir::new().expect("tempdir");
        write_proc_entry(
            tmp.path(),
            42,
            "java",
            &["/usr/bin/java", "-jar", "app.jar"],
        );
        let config = base_config();
        let result = scan_proc(tmp.path(), &config).expect("scan");
        let m = result.expect("should match on comm=java");
        assert_eq!(m.pid, 42);
        assert_eq!(m.matched_pattern, "java");
        assert_eq!(m.source, MatchSource::Comm);
    }

    #[test]
    fn test_scan_proc_matches_argv_substring() {
        let tmp = TempDir::new().expect("tempdir");
        // comm is "wrapper" (not in match_commands), but argv contains
        // "intellij" which is in match_argv_substrings
        write_proc_entry(
            tmp.path(),
            99,
            "wrapper",
            &["/opt/intellij/bin/idea-wrapper", "--no-splash"],
        );
        let config = base_config();
        let result = scan_proc(tmp.path(), &config).expect("scan");
        let m = result.expect("should match on argv substring");
        assert_eq!(m.pid, 99);
        assert_eq!(m.matched_pattern, "intellij");
        assert_eq!(m.source, MatchSource::Argv);
    }

    #[test]
    fn test_scan_proc_no_match_returns_none() {
        let tmp = TempDir::new().expect("tempdir");
        write_proc_entry(tmp.path(), 1, "systemd", &["/sbin/init"]);
        write_proc_entry(tmp.path(), 2, "sshd", &["/usr/sbin/sshd", "-D"]);
        let config = base_config();
        let result = scan_proc(tmp.path(), &config).expect("scan");
        assert!(result.is_none(), "no matching process → None");
    }

    #[test]
    fn test_scan_proc_skips_non_numeric_entries() {
        // /proc contains things like /proc/cpuinfo, /proc/sys/... — the
        // scanner must silently skip anything whose name is not a pid.
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir_all(tmp.path().join("sys")).expect("mkdir");
        fs::write(tmp.path().join("cpuinfo"), "model name: fake").expect("write");
        write_proc_entry(tmp.path(), 123, "java", &["java"]);
        let config = base_config();
        let result = scan_proc(tmp.path(), &config).expect("scan");
        let m = result.expect("should still find the java pid");
        assert_eq!(m.pid, 123);
    }

    #[test]
    fn test_scan_proc_handles_pid_without_comm_file() {
        // Races: a process can exit between readdir and opening its files.
        // The scanner must tolerate that without erroring the whole loop.
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir_all(tmp.path().join("55")).expect("empty pid dir");
        write_proc_entry(tmp.path(), 77, "java", &["java"]);
        let config = base_config();
        let result = scan_proc(tmp.path(), &config).expect("scan must not fail on bare pid dir");
        assert_eq!(
            result.expect("77 still matches").pid,
            77,
            "missing comm on pid 55 must not mask the match on 77"
        );
    }

    #[test]
    fn test_scan_proc_missing_root_is_error() {
        // /proc itself missing is a real error — the container is misconfigured
        // (hostPID not set, /proc not mounted). Don't silently return None.
        let tmp = TempDir::new().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");
        let err = scan_proc(&missing, &base_config()).expect_err("missing /proc must error");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("proc") || msg.contains("not found") || msg.contains("no such"),
            "error must identify the missing proc root, got: {err}"
        );
    }

    #[test]
    fn test_scan_proc_treats_empty_match_lists_as_no_match() {
        let tmp = TempDir::new().expect("tempdir");
        write_proc_entry(tmp.path(), 1, "java", &["java"]);
        let config = Config {
            match_commands: vec![],
            match_argv_substrings: vec![],
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
        };
        let result = scan_proc(tmp.path(), &config).expect("scan");
        assert!(
            result.is_none(),
            "empty config must never match — operator intent is 'no opt-in'"
        );
    }

    #[test]
    fn test_scan_proc_argv_substring_match_is_case_sensitive() {
        // Case-sensitive matching mirrors how the kernel exposes comm/cmdline.
        // Guards against a user writing "Java" in the config and being surprised.
        let tmp = TempDir::new().expect("tempdir");
        write_proc_entry(tmp.path(), 7, "Java", &["/usr/bin/Java"]);
        let config = base_config(); // match_commands = ["java", "idea"]
        let result = scan_proc(tmp.path(), &config).expect("scan");
        assert!(
            result.is_none(),
            "uppercase 'Java' must not match lowercase pattern 'java'"
        );
    }

    // ========================================================================
    // Annotation patch body
    // ========================================================================

    #[test]
    fn test_build_patch_body_contains_required_annotations() {
        let match_info = Match {
            pid: 42,
            matched_pattern: "java".to_string(),
            source: MatchSource::Comm,
        };
        let ts = "2026-04-19T21:30:00Z";
        let patch = build_patch_body(&match_info, ts);
        let annotations = patch
            .pointer("/metadata/annotations")
            .expect("patch must write /metadata/annotations")
            .as_object()
            .expect("annotations must be an object");
        assert_eq!(
            annotations
                .get(crate::constants::RECLAIM_REQUESTED_ANNOTATION)
                .and_then(|v| v.as_str()),
            Some(crate::constants::RECLAIM_REQUESTED_VALUE),
            "requested annotation must be literal 'true'"
        );
        assert_eq!(
            annotations
                .get(crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION)
                .and_then(|v| v.as_str()),
            Some(ts)
        );
        let reason = annotations
            .get(crate::constants::RECLAIM_REASON_ANNOTATION)
            .and_then(|v| v.as_str())
            .expect("reason annotation must be present");
        assert!(
            reason.contains("java"),
            "reason must include the matched pattern so audit logs are useful"
        );
        assert!(
            reason.starts_with("process-match:"),
            "reason must begin with the source tag for machine-parseability, got: {reason}"
        );
    }

    #[test]
    fn test_build_patch_body_argv_source_tag_differs_from_comm() {
        let comm_match = Match {
            pid: 1,
            matched_pattern: "java".to_string(),
            source: MatchSource::Comm,
        };
        let argv_match = Match {
            pid: 2,
            matched_pattern: "intellij".to_string(),
            source: MatchSource::Argv,
        };
        let ts = "2026-04-19T21:30:00Z";
        let a = build_patch_body(&comm_match, ts).to_string();
        let b = build_patch_body(&argv_match, ts).to_string();
        // Both use the same high-level tag per the contract ("process-match:")
        // but the matched pattern must differ in the body so operators can
        // distinguish which rule fired.
        assert!(a.contains("java"));
        assert!(b.contains("intellij"));
        assert_ne!(
            a, b,
            "different matches must produce different patch bodies"
        );
    }

    #[test]
    fn test_build_patch_body_is_strategic_merge_safe() {
        // The Node is patched with strategic-merge / merge-patch; the body
        // MUST only write into metadata.annotations so we never clobber
        // labels, spec, status, or any sibling field a kubelet/user wrote.
        let m = Match {
            pid: 1,
            matched_pattern: "java".to_string(),
            source: MatchSource::Comm,
        };
        let patch = build_patch_body(&m, "2026-04-19T21:30:00Z");
        let obj = patch.as_object().expect("patch is object");
        assert_eq!(obj.len(), 1, "top-level must be only {{metadata}}");
        let meta = obj
            .get("metadata")
            .and_then(|v| v.as_object())
            .expect("metadata must be object");
        assert_eq!(
            meta.len(),
            1,
            "metadata must contain only {{annotations}} — writing labels/name/etc is a foot-gun"
        );
        assert!(meta.contains_key("annotations"));
    }

    // ========================================================================
    // Idempotence guard
    // ========================================================================

    #[test]
    fn test_already_requested_detects_prior_annotation() {
        let existing: std::collections::BTreeMap<String, String> = [(
            crate::constants::RECLAIM_REQUESTED_ANNOTATION.to_string(),
            crate::constants::RECLAIM_REQUESTED_VALUE.to_string(),
        )]
        .into_iter()
        .collect();
        assert!(
            already_requested(&existing),
            "existing 'true' annotation must be detected so the agent exits idempotently"
        );
    }

    #[test]
    fn test_already_requested_false_on_empty_annotations() {
        let existing = std::collections::BTreeMap::<String, String>::default();
        assert!(!already_requested(&existing));
    }

    #[test]
    fn test_already_requested_false_on_wrong_value() {
        // Any value other than the literal "true" is treated as not-requested.
        // Guards against partial writes and against "false"/"0"/"" foot-guns.
        let existing: std::collections::BTreeMap<String, String> = [(
            crate::constants::RECLAIM_REQUESTED_ANNOTATION.to_string(),
            "false".to_string(),
        )]
        .into_iter()
        .collect();
        assert!(!already_requested(&existing));
    }

    // ========================================================================
    // ConfigMap → Config extraction (reactive watch path)
    //
    // The agent no longer loads a file at startup; instead, it watches its own
    // per-node `ConfigMap` and reloads on every change.  `configmap_to_config`
    // is the pure bridge between an observed `ConfigMap` event and a usable
    // `Option<Config>`:
    //   - `Ok(Some(Config))`  → arm the scanner with these commands
    //   - `Ok(None)`          → nothing to match on, idle quietly
    //   - `Err(_)`            → malformed payload; caller keeps last-good config
    // ========================================================================

    #[test]
    fn test_configmap_to_config_parses_valid_reclaim_toml() {
        use k8s_openapi::api::core::v1::ConfigMap;
        use std::collections::BTreeMap;

        let mut data = BTreeMap::new();
        data.insert(
            RECLAIM_CONFIG_DATA_KEY.to_string(),
            "match_commands = [\"java\"]\npoll_interval_ms = 100\n".to_string(),
        );
        let cm = ConfigMap {
            data: Some(data),
            ..Default::default()
        };
        let cfg = configmap_to_config(&cm)
            .expect("parse")
            .expect("key present → Some");
        assert_eq!(cfg.match_commands, vec!["java"]);
        assert_eq!(cfg.poll_interval_ms, 100);
    }

    #[test]
    fn test_configmap_to_config_missing_key_returns_ok_none() {
        // CM with data{} but no `reclaim.toml` key — the controller projected
        // an empty spec.  Agent must idle, not error.
        use k8s_openapi::api::core::v1::ConfigMap;
        use std::collections::BTreeMap;

        let cm = ConfigMap {
            data: Some(BTreeMap::new()),
            ..Default::default()
        };
        assert!(
            configmap_to_config(&cm).expect("ok").is_none(),
            "missing data key → idle (None), not error"
        );
    }

    #[test]
    fn test_configmap_to_config_data_field_absent_returns_ok_none() {
        // Freshly created ConfigMap with `data: null` — same idle intent.
        use k8s_openapi::api::core::v1::ConfigMap;
        let cm = ConfigMap::default();
        assert!(
            configmap_to_config(&cm).expect("ok").is_none(),
            "no data block → idle (None)"
        );
    }

    #[test]
    fn test_configmap_to_config_malformed_toml_returns_err() {
        // Operator hand-edited the CM and broke the TOML.  Surface the error
        // to the caller so it can log and hold the previous known-good config.
        use k8s_openapi::api::core::v1::ConfigMap;
        use std::collections::BTreeMap;

        let mut data = BTreeMap::new();
        data.insert(
            RECLAIM_CONFIG_DATA_KEY.to_string(),
            "this is = [ not toml".to_string(),
        );
        let cm = ConfigMap {
            data: Some(data),
            ..Default::default()
        };
        let err = configmap_to_config(&cm).expect_err("malformed TOML must error");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("parse") || msg.contains("toml"),
            "error must name the parse failure, got: {err}"
        );
    }

    // ========================================================================
    // Phase 4: host-identity verification
    //
    // Closes the "modified DaemonSet hard-codes NODE_NAME" attack: before the
    // agent PATCHes a Node with reclaim annotations, it cross-checks
    // /etc/machine-id (the host's stable identifier, set by
    // systemd-machine-id-setup / kairos / k0s-installer) against the target
    // Node's status.nodeInfo.machineID (which kubelet populates from the
    // same source). Mismatch ⇒ refuse to patch.
    //
    // Both helpers are pure — no kube I/O — so the binary can wire them
    // around its own fetch.
    // ========================================================================

    #[test]
    fn test_read_host_machine_id_returns_trimmed_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("machine-id");
        // Real /etc/machine-id format: 32 hex digits + trailing newline.
        fs::write(&path, "abc123def4567890aabbccddeeff0011\n").unwrap();
        let id = read_host_machine_id(&path).expect("read");
        assert_eq!(
            id, "abc123def4567890aabbccddeeff0011",
            "trailing newline must be trimmed"
        );
    }

    #[test]
    fn test_read_host_machine_id_missing_file_errors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("does-not-exist");
        let err = read_host_machine_id(&path).expect_err("missing file must error");
        let msg = err.to_string();
        assert!(
            msg.contains("does-not-exist") || msg.to_lowercase().contains("cannot read"),
            "error must name the missing path, got: {msg}"
        );
    }

    #[test]
    fn test_read_host_machine_id_empty_file_errors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("machine-id");
        fs::write(&path, "").unwrap();
        let err = read_host_machine_id(&path).expect_err("empty file must error");
        assert!(err.to_string().to_lowercase().contains("empty"));
    }

    #[test]
    fn test_read_host_machine_id_whitespace_only_errors() {
        // Defensive: a corrupted /etc/machine-id with only whitespace
        // (newlines, spaces) is indistinguishable from "no identity"
        // and must fail closed rather than producing an empty token.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("machine-id");
        fs::write(&path, "   \n\t\n").unwrap();
        let err = read_host_machine_id(&path).expect_err("whitespace-only must error");
        assert!(err.to_string().to_lowercase().contains("empty"));
    }

    fn node_with_machine_id(machine_id: &str) -> k8s_openapi::api::core::v1::Node {
        use k8s_openapi::api::core::v1::{Node, NodeStatus, NodeSystemInfo};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        Node {
            metadata: ObjectMeta {
                name: Some("worker-1".to_string()),
                ..Default::default()
            },
            status: Some(NodeStatus {
                node_info: Some(NodeSystemInfo {
                    machine_id: machine_id.to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_compare_machine_ids_match_returns_ok() {
        let node = node_with_machine_id("abc123def4567890aabbccddeeff0011");
        compare_machine_ids(&node, "worker-1", "abc123def4567890aabbccddeeff0011")
            .expect("match must succeed");
    }

    #[test]
    fn test_compare_machine_ids_mismatch_errors_with_both_ids() {
        // Spoofed-NODE_NAME exploit: agent runs on host-A but DaemonSet was
        // edited to NODE_NAME=victim-host. The fetched victim-host Node has
        // a DIFFERENT machineID than this agent's /etc/machine-id. Refuse.
        let node = node_with_machine_id("victim-host-id-aaaaaaaaaaaaaaaa");
        let err = compare_machine_ids(&node, "victim-host", "agent-host-id-bbbbbbbbbbbbbbbb")
            .expect_err("mismatch must error");
        let msg = err.to_string();
        assert!(
            msg.contains("victim-host"),
            "error must name the target node, got: {msg}"
        );
        assert!(
            msg.contains("agent-host-id-bbbbbbbbbbbbbbbb"),
            "error must include the agent's host id for forensics, got: {msg}"
        );
        assert!(
            msg.contains("victim-host-id-aaaaaaaaaaaaaaaa"),
            "error must include the Node's id, got: {msg}"
        );
        assert!(
            msg.to_lowercase().contains("mismatch") || msg.to_lowercase().contains("refus"),
            "error must signal refusal, got: {msg}"
        );
    }

    #[test]
    fn test_compare_machine_ids_node_without_status_errors() {
        use k8s_openapi::api::core::v1::Node;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        let node = Node {
            metadata: ObjectMeta {
                name: Some("worker-1".to_string()),
                ..Default::default()
            },
            status: None,
            ..Default::default()
        };
        let err = compare_machine_ids(&node, "worker-1", "anything").expect_err("must error");
        assert!(
            err.to_string().to_lowercase().contains("machineid")
                || err.to_string().to_lowercase().contains("missing"),
            "error must signal missing machine-id, got: {err}"
        );
    }

    #[test]
    fn test_compare_machine_ids_empty_node_machine_id_errors() {
        // Defensive: a Node with status.nodeInfo.machineID="" (kubelet hasn't
        // populated it yet) must fail-closed — we cannot verify identity.
        let node = node_with_machine_id("");
        let err = compare_machine_ids(&node, "worker-1", "anything").expect_err("must error");
        assert!(
            err.to_string().to_lowercase().contains("machineid")
                || err.to_string().to_lowercase().contains("missing"),
            "error must signal missing machine-id, got: {err}"
        );
    }

    #[test]
    fn test_compare_machine_ids_whitespace_node_machine_id_is_ignored() {
        // A whitespace-only machineID is equivalent to absent — fail closed.
        let node = node_with_machine_id("   \n");
        let err = compare_machine_ids(&node, "worker-1", "anything").expect_err("must error");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("machineid") || msg.contains("no status") || msg.contains("missing"),
            "error must signal absent machine-id, got: {err}"
        );
    }
}
