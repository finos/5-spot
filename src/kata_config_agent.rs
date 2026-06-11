// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Kata config agent — host-filesystem sync engine
//!
//! Core logic for the node-side `5spot-kata-config-agent` binary (ADR 0002 /
//! ADR 0003, roadmap `completed-5spot-kata-config-per-node.md`, Phase 3). This module is
//! I/O-light around the filesystem and fully unit-testable; the binary entry
//! point in `src/bin/kata_config_agent.rs` wires it to a poll loop.
//!
//! ## Sync contract
//!
//! The agent reconciles a single host destination file to match the drop-in
//! content it reads from the workload-cluster `ConfigMap`/`Secret` named by its
//! Node's `5spot.finos.org/kata-config-ref` annotation (ADR 0002 — the agent
//! reads via the kube API, not a mounted file, because a cluster-wide DaemonSet
//! cannot template a `configMap.name` volume per replica):
//!
//! - content present, hashes differ → **atomically** write the host file
//!   (temp-file in the destination directory + `rename`, mode `0644`).
//! - content present, hashes match → no-op (drift-watch).
//! - content absent (key removed / object deleted / annotation cleared) →
//!   unlink the host file (GitOps: absent in source ⇒ absent on host).
//!
//! Writes are atomic so a crash mid-write can never leave a partially-written
//! drop-in that containerd would fail to parse.
//!
//! ## Restart orchestration (Phase 4 — ADR 0003)
//!
//! After a write/delete the host k0s service must be restarted so containerd
//! reloads the drop-in. This module owns the I/O-light, unit-testable pieces of
//! that mechanism — the [`nsenter_restart_argv`] command-line builder, the
//! [`RestartExecutor`] abstraction (so tests assert the command line without
//! executing it), the bare applied-hash value carried in the
//! `5spot.finos.org/kata-config-applied` Node annotation, and the
//! [`needs_restart`] / [`restart_if_needed`] restart-loop guard. The concrete
//! `nsenter`-executing implementation lives in the binary; the poll loop wires
//! these together.
//!
//! ## Host-path containment (ADR 0005)
//!
//! The drop-in destination is the **fixed**
//! [`crate::constants::KATA_CONFIG_DEST_PATH`] — no CRD field or annotation
//! carries a host path. [`confine_dest_path`] additionally resolves and
//! re-checks that constant against the `/etc/k0s/` base (canonicalized, so
//! host-side symlink games fail) before every write and unlink, fail closed.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Parsed `5spot.finos.org/kata-config-ref` Node annotation — the compact JSON
/// object the controller stamps to tell the agent which workload object to read
/// and where to write it (ADR 0002). Field names match the controller's
/// `build_kata_config_ref_annotation_patch`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct KataRef {
    /// Workload-cluster namespace holding the source object.
    pub namespace: String,
    /// Source kind: `"ConfigMap"` or `"Secret"`.
    pub kind: String,
    /// Source object name.
    pub name: String,
    /// `data` key whose value is the drop-in content.
    pub key: String,
    /// systemd unit to restart so containerd reloads the drop-in (Phase 4).
    #[serde(rename = "restartService")]
    pub restart_service: String,
}

/// Parse the `5spot.finos.org/kata-config-ref` annotation value (a JSON string)
/// into a [`KataRef`].
///
/// # Errors
/// Returns the [`serde_json::Error`] if the value is not the expected JSON
/// object (missing/extra fields, wrong types).
pub fn parse_kata_ref(annotation: &str) -> Result<KataRef, serde_json::Error> {
    serde_json::from_str(annotation)
}

/// Default interval between drift-watch sweeps, in seconds. The agent reads
/// the projected source and reconciles the host file every tick; 30s caps
/// drift-correction latency while keeping the loop near-idle.
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;

/// File mode applied to the written drop-in. `0644` is world-readable so
/// containerd (running as root) can read it; not writable by group/other.
const DEST_FILE_MODE: u32 = 0o644;

/// Suffix for the in-flight temporary file written next to the destination
/// before the atomic `rename`. Includes the PID so a crashed prior run's
/// leftover never collides with a live write.
const TEMP_SUFFIX: &str = "5spot-tmp";

/// Compute the lowercase hex SHA-256 of `bytes`. Used for content/drift
/// comparison and (Phase 4) the applied-hash node annotation.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Return `Some(sha256_hex)` of the file at `path`, or `None` if the file does
/// not exist.
///
/// # Errors
/// Returns the underlying [`io::Error`] for any failure other than the file
/// being absent (which is the expected steady state before first provision).
pub fn file_sha256(path: &Path) -> io::Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(sha256_hex(&bytes))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The reconcile action chosen by [`decide_action`] for one sync tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncAction {
    /// Source present and differs from dest (or dest absent) → write it.
    Write,
    /// Source absent but dest present → unlink the host file (GitOps delete).
    Delete,
    /// Source and dest already agree (or both absent) → nothing to do.
    NoOp,
}

/// Decide what to do given the source bytes (if present) and the current
/// destination hash (if the host file exists). Pure — no I/O.
#[must_use]
pub fn decide_action(source: Option<&[u8]>, dest_hash: Option<&str>) -> SyncAction {
    match source {
        Some(bytes) => {
            if dest_hash == Some(sha256_hex(bytes).as_str()) {
                SyncAction::NoOp
            } else {
                SyncAction::Write
            }
        }
        None => {
            if dest_hash.is_some() {
                SyncAction::Delete
            } else {
                SyncAction::NoOp
            }
        }
    }
}

/// Build the path of the temporary file written alongside `dest`.
fn temp_path_for(parent: &Path, dest: &Path) -> PathBuf {
    let file_name = dest.file_name().map_or_else(
        || "kata-config".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let pid = std::process::id();
    parent.join(format!("{file_name}.{TEMP_SUFFIX}.{pid}"))
}

/// Atomically write `content` to `dest`: create any missing parent directories,
/// write a temp file in the destination directory, set mode `0644`, then
/// `rename` it over `dest`. The rename is atomic on a single filesystem, so a
/// crash can never leave a partially-written drop-in for containerd to choke
/// on. Ownership is the agent's process UID (root in the DaemonSet).
///
/// # Errors
/// Returns the underlying [`io::Error`] if any directory creation, temp write,
/// permission set, or rename fails.
pub fn atomic_write(dest: &Path, content: &[u8]) -> io::Result<()> {
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent)?;

    let tmp = temp_path_for(parent, dest);
    // Best-effort cleanup of a stale temp from a prior crashed run with the
    // same PID (astronomically unlikely, but keeps the write deterministic).
    let _ = fs::remove_file(&tmp);
    fs::write(&tmp, content)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(DEST_FILE_MODE))?;

    match fs::rename(&tmp, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Don't leak the temp file if the rename failed.
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Remove `dest` if it exists. Returns `Ok(true)` if a file was removed,
/// `Ok(false)` if it was already absent (idempotent tear-down).
///
/// # Errors
/// Returns the underlying [`io::Error`] for any failure other than the file
/// being absent.
pub fn remove_if_present(dest: &Path) -> io::Result<bool> {
    match fs::remove_file(dest) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// What [`sync_once`] did this tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncOutcome {
    /// Wrote the host file; carries the SHA-256 of the content now on disk.
    Wrote(String),
    /// Deleted the host file (source went away).
    Deleted,
    /// No change; carries the current dest hash (`None` if dest is absent).
    Unchanged(Option<String>),
}

/// Reconcile `dest_path` to match in-memory `content` exactly once (the API-read
/// path — `content` is the drop-in body extracted from the workload object, or
/// `None` when the object/key/annotation is absent):
///
/// - content present, differs/absent dest → [`atomic_write`], returns `Wrote`.
/// - content absent, dest present → [`remove_if_present`], returns `Deleted`.
/// - already in sync → returns `Unchanged`.
///
/// # Errors
/// Returns an [`io::Error`] if hashing the destination, writing, or unlinking
/// fails.
pub fn sync_content(content: Option<&[u8]>, dest_path: &Path) -> io::Result<SyncOutcome> {
    let dest_hash = file_sha256(dest_path)?;

    match decide_action(content, dest_hash.as_deref()) {
        SyncAction::Write => {
            let bytes = content.expect("Write action implies the content is present");
            atomic_write(dest_path, bytes)?;
            Ok(SyncOutcome::Wrote(sha256_hex(bytes)))
        }
        SyncAction::Delete => {
            remove_if_present(dest_path)?;
            Ok(SyncOutcome::Deleted)
        }
        SyncAction::NoOp => Ok(SyncOutcome::Unchanged(dest_hash)),
    }
}

/// Suffix every kata drop-in must carry — the only file type k0s's containerd
/// import directory consumes, and a cheap fail-closed bound on what the
/// privileged agent will write (ADR 0005).
const DEST_FILE_SUFFIX: &str = ".toml";

/// Resolve `dest_path` to its in-pod location under `host_root`, enforcing the
/// ADR 0005 containment at the privileged trust boundary. The caller passes the
/// fixed [`crate::constants::KATA_CONFIG_DEST_PATH`] — no user input reaches
/// this function — so the checks are defense-in-depth against host-side
/// symlink games and future regressions, run before **every** host write and
/// tear-down unlink.
///
/// Checks, fail-closed:
/// 1. must start with [`crate::constants::KATA_CONFIG_DEST_BASE`] (`/etc/k0s/`,
///    slash-aware) and end with `.toml`;
/// 2. lexical: every `/`-separated segment must be a normal name — no `..`,
///    no `.`, no empty segment — so no later join can reinterpret the path;
/// 3. physical: the destination parent is created (the write needs it anyway)
///    and canonicalized, and must remain under the canonicalized
///    `<host_root>/etc/k0s` — a symlinked directory inside the base resolves
///    elsewhere and fails this check even though it passes the lexical one.
///
/// Returns the canonicalized parent joined with the file name — the path the
/// caller hands to [`sync_content`].
///
/// # Errors
/// [`io::ErrorKind::PermissionDenied`] for any containment violation; the
/// underlying [`io::Error`] if directory creation or canonicalization fails.
pub fn confine_dest_path(host_root: &Path, dest_path: &str) -> io::Result<PathBuf> {
    use crate::constants::KATA_CONFIG_DEST_BASE;

    let deny = |why: &str| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing destPath {dest_path:?}: {why} \
                 (ADR 0005 confines kata writes to {KATA_CONFIG_DEST_BASE}*.toml)"
            ),
        )
    };

    if !dest_path.starts_with(KATA_CONFIG_DEST_BASE) {
        return Err(deny("outside the allowed base"));
    }
    if !dest_path.ends_with(DEST_FILE_SUFFIX) {
        return Err(deny("not a .toml drop-in"));
    }
    // Lexical pass. `split('/')` on an absolute path yields a leading ""
    // (before the root slash) — every later segment must be a plain name.
    if dest_path
        .split('/')
        .skip(1)
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(deny("non-canonical path segment"));
    }

    let full = host_root.join(dest_path.trim_start_matches('/'));
    let parent = full.parent().ok_or_else(|| deny("no parent directory"))?;
    let file_name = full
        .file_name()
        .ok_or_else(|| deny("no file name"))?
        .to_os_string();

    // Physical pass: canonicalize and re-check the boundary.
    let base = host_root.join(KATA_CONFIG_DEST_BASE.trim_start_matches('/'));
    fs::create_dir_all(parent)?;
    fs::create_dir_all(&base)?;
    let canon_parent = parent.canonicalize()?;
    let canon_base = base.canonicalize()?;
    if !canon_parent.starts_with(&canon_base) {
        return Err(deny("resolves outside the allowed base (symlink?)"));
    }
    Ok(canon_parent.join(file_name))
}

/// Reconcile `dest_path` to match the file at `source_path` exactly once.
///
/// Thin wrapper over [`sync_content`] that reads `source_path` first (absent
/// file ⇒ `None` content). Retained for the file-source path and its tests.
///
/// # Errors
/// Returns an [`io::Error`] if reading the source (other than absent), hashing
/// the destination, writing, or unlinking fails.
pub fn sync_once(source_path: &Path, dest_path: &Path) -> io::Result<SyncOutcome> {
    let source = match fs::read(source_path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    sync_content(source.as_deref(), dest_path)
}

// ============================================================================
// Phase 4 — host k0s-service restart orchestration (ADR 0003)
// ============================================================================

/// Sentinel recorded in [`AppliedRecord::hash`] when the agent has torn the
/// drop-in down (no content on the host). Distinguishes "applied nothing" from
/// "applied content `X`" so the restart guard fires on a present → absent
/// transition.
pub const ABSENT_HASH_MARKER: &str = "absent";

/// `nsenter` binary — entered to run a command in the host's namespaces.
const NSENTER_BIN: &str = "nsenter";
/// Host init PID. `nsenter -t 1` targets systemd (PID 1) on the host; requires
/// `hostPID: true` on the pod so PID 1 resolves to the host's init, not the
/// container's.
const HOST_INIT_PID: &str = "1";
/// `systemctl` invoked inside the host mount namespace to restart the unit.
const SYSTEMCTL_BIN: &str = "systemctl";
/// `systemctl` subcommand.
const SYSTEMCTL_RESTART: &str = "restart";

/// Build the exact argv for the in-pod host-service restart (ADR 0003):
///
/// ```text
/// nsenter -t 1 -m -u -i -n -p -- systemctl restart <service>
/// ```
///
/// `-t 1` targets host PID 1 (systemd); `-m -u -i -n -p` enter the host mount,
/// UTS, IPC, network, and PID namespaces so `systemctl` and its D-Bus socket
/// resolve to the host's, matching an operator shell on the node.
#[must_use]
pub fn nsenter_restart_argv(service: &str) -> Vec<String> {
    [
        NSENTER_BIN,
        "-t",
        HOST_INIT_PID,
        "-m",
        "-u",
        "-i",
        "-n",
        "-p",
        "--",
        SYSTEMCTL_BIN,
        SYSTEMCTL_RESTART,
        service,
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Restarts a host systemd unit. Hidden behind a trait so the poll loop can be
/// unit-tested with a fake while the real implementation (`nsenter` into host
/// PID 1) is exercised only in integration tests / on a node.
pub trait RestartExecutor {
    /// Restart `service` on the host, blocking until the request is issued.
    ///
    /// # Errors
    /// Returns an [`io::Error`] if the restart command cannot be spawned or
    /// exits non-zero. Note the restart typically SIGKILLs this very pod
    /// (containerd bounces); a successful return is not guaranteed even on
    /// success — the caller treats an error as "retry next tick".
    fn restart(&self, service: &str) -> io::Result<()>;
}

/// The content hash a [`SyncOutcome`] implies is now on the host — the bare
/// value to record in the `5spot.finos.org/kata-config-applied` annotation and
/// compare against in [`needs_restart`]. A present file carries its content
/// hash; an absent one (deleted, or never written) maps to
/// [`ABSENT_HASH_MARKER`]. The annotation deliberately carries **no path**
/// (ADR 0005): the drop-in location is the fixed
/// [`crate::constants::KATA_CONFIG_DEST_PATH`], so a forged annotation cannot
/// steer a root unlink.
#[must_use]
pub fn intended_hash_for(outcome: &SyncOutcome) -> String {
    match outcome {
        SyncOutcome::Wrote(hash) | SyncOutcome::Unchanged(Some(hash)) => hash.clone(),
        SyncOutcome::Deleted | SyncOutcome::Unchanged(None) => ABSENT_HASH_MARKER.to_string(),
    }
}

/// Classify a sync outcome for metrics: `true` iff the agent rewrote content
/// whose hash was **already applied** (and restarted for) — i.e. it corrected
/// an out-of-band edit/deletion back to the desired state. A write of a hash
/// that differs from the applied record is a new-config rollout, not drift.
#[must_use]
pub fn is_drift_correction(outcome: &SyncOutcome, prev_applied_hash: Option<&str>) -> bool {
    match outcome {
        SyncOutcome::Wrote(hash) => prev_applied_hash == Some(hash.as_str()),
        SyncOutcome::Deleted | SyncOutcome::Unchanged(_) => false,
    }
}

/// The restart-loop guard: restart only when the content now on the host differs
/// from what was last applied (or nothing has been applied yet). Equal hashes
/// mean containerd was already restarted for this content — drift correction
/// rewrites the file but must not bounce the service again (ADR 0003).
#[must_use]
pub fn needs_restart(prev_applied_hash: Option<&str>, intended_hash: &str) -> bool {
    prev_applied_hash != Some(intended_hash)
}

/// Restart `service` via `executor` iff [`needs_restart`] says the applied state
/// is stale. Returns `Ok(true)` if a restart was issued, `Ok(false)` if it was
/// short-circuited. The caller must record the new [`AppliedRecord`] **before**
/// invoking this, so a SIGKILL mid-restart does not re-trigger next tick.
///
/// # Errors
/// Propagates any [`io::Error`] from the executor.
pub fn restart_if_needed(
    executor: &dyn RestartExecutor,
    prev_applied_hash: Option<&str>,
    intended_hash: &str,
    service: &str,
) -> io::Result<bool> {
    if !needs_restart(prev_applied_hash, intended_hash) {
        return Ok(false);
    }
    executor.restart(service)?;
    Ok(true)
}

#[cfg(test)]
#[path = "kata_config_agent_tests.rs"]
mod tests;
