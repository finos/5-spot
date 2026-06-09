// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    // ========================================================================
    // sha256_hex — content hashing
    // ========================================================================

    #[test]
    fn test_sha256_hex_known_vectors() {
        // NIST/standard SHA-256 vectors pin the implementation so a crate swap
        // or encoding bug surfaces immediately.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_hex_differs_for_different_input() {
        assert_ne!(sha256_hex(b"version = 1"), sha256_hex(b"version = 2"));
    }

    // ========================================================================
    // file_sha256 — hash of an on-disk file (absent => None)
    // ========================================================================

    #[test]
    fn test_file_sha256_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        assert_eq!(file_sha256(&missing).unwrap(), None);
    }

    #[test]
    fn test_file_sha256_present_matches_sha256_hex() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("present.toml");
        fs::write(&path, b"hello-kata").unwrap();
        assert_eq!(file_sha256(&path).unwrap(), Some(sha256_hex(b"hello-kata")));
    }

    // ========================================================================
    // decide_action — pure sync decision
    // ========================================================================

    #[test]
    fn test_decide_write_when_source_present_and_dest_absent() {
        assert_eq!(decide_action(Some(b"x"), None), SyncAction::Write);
    }

    #[test]
    fn test_decide_noop_when_hashes_match() {
        let h = sha256_hex(b"same");
        assert_eq!(decide_action(Some(b"same"), Some(&h)), SyncAction::NoOp);
    }

    #[test]
    fn test_decide_write_when_hashes_differ() {
        let stale = sha256_hex(b"old");
        assert_eq!(decide_action(Some(b"new"), Some(&stale)), SyncAction::Write);
    }

    #[test]
    fn test_decide_delete_when_source_absent_and_dest_present() {
        let h = sha256_hex(b"orphan");
        assert_eq!(decide_action(None, Some(&h)), SyncAction::Delete);
    }

    #[test]
    fn test_decide_noop_when_both_absent() {
        assert_eq!(decide_action(None, None), SyncAction::NoOp);
    }

    // ========================================================================
    // atomic_write — temp + rename, mode 0644, parent dirs
    // ========================================================================

    #[test]
    fn test_atomic_write_creates_file_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        atomic_write(&dest, b"body").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"body");
    }

    #[test]
    fn test_atomic_write_sets_mode_0644() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        atomic_write(&dest, b"x").unwrap();
        let mode = fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o644,
            "drop-in must be world-readable 0644, got {mode:o}"
        );
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        fs::write(&dest, b"old").unwrap();
        atomic_write(&dest, b"new").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        // Mimic /etc/k0s/container.d/ not existing yet.
        let dest = dir.path().join("etc/k0s/container.d/kata-containers.toml");
        atomic_write(&dest, b"deep").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"deep");
    }

    #[test]
    fn test_atomic_write_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        atomic_write(&dest, b"x").unwrap();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries,
            vec!["kata.toml".to_string()],
            "atomic_write must rename its temp file away, leaving only the dest: {entries:?}"
        );
    }

    #[test]
    fn test_atomic_write_stray_temp_does_not_corrupt_dest() {
        // Crash-safety: a leftover temp file from a prior interrupted write
        // must not affect the destination — a fresh write renames over dest
        // atomically regardless.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        fs::write(dir.path().join("kata.toml.5spot-tmp.999999"), b"garbage").unwrap();
        atomic_write(&dest, b"clean").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"clean");
    }

    // ========================================================================
    // remove_if_present — idempotent unlink
    // ========================================================================

    #[test]
    fn test_remove_if_present_removes_existing_returns_true() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        fs::write(&dest, b"x").unwrap();
        assert!(remove_if_present(&dest).unwrap());
        assert!(!Path::new(&dest).exists());
    }

    #[test]
    fn test_remove_if_present_absent_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nope.toml");
        assert!(!remove_if_present(&dest).unwrap());
    }

    // ========================================================================
    // sync_once — end-to-end reconcile of dest to source
    // ========================================================================

    #[test]
    fn test_sync_once_writes_when_dest_absent() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml");
        let dest = dir.path().join("dest.toml");
        fs::write(&src, b"version = 2\n").unwrap();
        let outcome = sync_once(&src, &dest).unwrap();
        assert_eq!(outcome, SyncOutcome::Wrote(sha256_hex(b"version = 2\n")));
        assert_eq!(fs::read(&dest).unwrap(), b"version = 2\n");
    }

    #[test]
    fn test_sync_once_noop_when_dest_matches() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml");
        let dest = dir.path().join("dest.toml");
        fs::write(&src, b"same").unwrap();
        fs::write(&dest, b"same").unwrap();
        let outcome = sync_once(&src, &dest).unwrap();
        assert_eq!(outcome, SyncOutcome::Unchanged(Some(sha256_hex(b"same"))));
    }

    #[test]
    fn test_sync_once_rewrites_on_drift() {
        // Host file edited out-of-band must be restored to the source content.
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml");
        let dest = dir.path().join("dest.toml");
        fs::write(&src, b"canonical").unwrap();
        fs::write(&dest, b"tampered").unwrap();
        let outcome = sync_once(&src, &dest).unwrap();
        assert_eq!(outcome, SyncOutcome::Wrote(sha256_hex(b"canonical")));
        assert_eq!(fs::read(&dest).unwrap(), b"canonical");
    }

    #[test]
    fn test_sync_once_deletes_when_source_absent() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml"); // never created
        let dest = dir.path().join("dest.toml");
        fs::write(&dest, b"orphan").unwrap();
        let outcome = sync_once(&src, &dest).unwrap();
        assert_eq!(outcome, SyncOutcome::Deleted);
        assert!(!Path::new(&dest).exists());
    }

    #[test]
    fn test_sync_once_noop_when_both_absent() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml");
        let dest = dir.path().join("dest.toml");
        let outcome = sync_once(&src, &dest).unwrap();
        assert_eq!(outcome, SyncOutcome::Unchanged(None));
    }

    // ---- parse_kata_ref: the 5spot.finos.org/kata-config-ref annotation ----

    #[test]
    fn test_parse_kata_ref_full_object() {
        let json = r#"{
            "namespace": "5spot-system",
            "kind": "ConfigMap",
            "name": "kata-drop-in",
            "key": "kata-containers.toml",
            "destPath": "/etc/k0s/container.d/kata-containers.toml",
            "restartService": "k0sworker.service"
        }"#;
        let r = parse_kata_ref(json).expect("must parse the controller-stamped annotation");
        assert_eq!(r.namespace, "5spot-system");
        assert_eq!(r.kind, "ConfigMap");
        assert_eq!(r.name, "kata-drop-in");
        assert_eq!(r.key, "kata-containers.toml");
        assert_eq!(r.dest_path, "/etc/k0s/container.d/kata-containers.toml");
        assert_eq!(r.restart_service, "k0sworker.service");
    }

    #[test]
    fn test_parse_kata_ref_rejects_missing_field() {
        // destPath omitted → hard error (the camelCase rename is required).
        let json = r#"{"namespace":"ns","kind":"Secret","name":"n","key":"k","restartService":"s.service"}"#;
        assert!(
            parse_kata_ref(json).is_err(),
            "a missing destPath must be a parse error, not a silent default"
        );
    }

    #[test]
    fn test_parse_kata_ref_rejects_non_json() {
        assert!(parse_kata_ref("not json").is_err());
    }

    // ---- sync_content: the API-read sync path ----

    #[test]
    fn test_sync_content_writes_when_present_and_dest_absent() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        let outcome = sync_content(Some(b"version = 2\n"), &dest).unwrap();
        assert!(matches!(outcome, SyncOutcome::Wrote(_)));
        assert_eq!(fs::read(&dest).unwrap(), b"version = 2\n");
        let mode = fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn test_sync_content_deletes_when_content_absent() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        fs::write(&dest, b"orphan").unwrap();
        let outcome = sync_content(None, &dest).unwrap();
        assert_eq!(outcome, SyncOutcome::Deleted);
        assert!(!dest.exists());
    }

    #[test]
    fn test_sync_content_noop_when_hashes_match() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("kata.toml");
        fs::write(&dest, b"same").unwrap();
        let outcome = sync_content(Some(b"same"), &dest).unwrap();
        assert!(matches!(outcome, SyncOutcome::Unchanged(Some(_))));
    }
}
