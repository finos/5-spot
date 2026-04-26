// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! # Reclaim agent — process-match detector
//!
//! Core logic for the node-side `5spot-reclaim-agent` binary (roadmap
//! `5spot-emergency-reclaim-by-process-match.md`, Phase 2). This module is
//! I/O-light and fully unit-testable; the binary entry point in
//! `src/bin/reclaim_agent.rs` wires it to a real kube client and a real
//! `/proc`.
//!
//! ## Detection contract
//!
//! The agent scans `/proc/<pid>/{comm,cmdline}` looking for any process
//! whose basename matches [`Config::match_commands`] exactly, or whose
//! argv contains any of [`Config::match_argv_substrings`] as a substring.
//! On first match the agent writes three annotations onto its own `Node`
//! object (see [`crate::constants::RECLAIM_REQUESTED_ANNOTATION`] and
//! siblings) and exits.
//!
//! ## Rungs
//!
//! This module implements _rung 1_ (the poll MVP). Rung 2 (netlink proc
//! connector) will reuse [`Match`] / [`build_patch_body`] unchanged and
//! only swap the event source.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

/// Default interval between `/proc` sweeps, in milliseconds. 250 ms caps
/// detection latency at a quarter second while keeping CPU usage trivial
/// on a quiet node.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 250;

// ============================================================================
// Config
// ============================================================================

/// Agent configuration.
///
/// In production the agent receives this via a reactive watch on its
/// per-node `ConfigMap` — see [`configmap_to_config`] for the bridge
/// from an observed `ConfigMap` to `Option<Config>`. [`parse_config`]
/// and [`load_config`] remain available for local-dev / smoke-test
/// paths where a TOML file is the more convenient input.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Exact basenames matched against `/proc/<pid>/comm`. Case-sensitive.
    #[serde(default)]
    pub match_commands: Vec<String>,

    /// Substrings matched against `/proc/<pid>/cmdline` (NUL-separated
    /// argv joined into one logical string). Case-sensitive.
    #[serde(default)]
    pub match_argv_substrings: Vec<String>,

    /// Poll interval in milliseconds. Must be non-zero; a value of 0
    /// would spin the detector loop.
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

/// Parse a TOML config string into a [`Config`].
///
/// # Errors
/// Returns an error if the TOML is malformed or if a validation rule
/// (e.g. non-zero poll interval) is violated.
pub fn parse_config(input: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(input).map_err(ConfigError::Parse)?;
    if config.poll_interval_ms == 0 {
        return Err(ConfigError::Invalid(
            "poll_interval_ms must be > 0".to_string(),
        ));
    }
    Ok(config)
}

/// Read and parse a config file from disk.
///
/// # Errors
/// Returns an error if the file cannot be read or the contents fail
/// [`parse_config`] validation.
pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = fs::read_to_string(path)
        .map_err(|e| ConfigError::Invalid(format!("cannot read config {}: {e}", path.display())))?;
    parse_config(&contents)
}

/// Errors returned by [`parse_config`] / [`load_config`].
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// TOML parse failure.
    #[error("failed to parse reclaim-agent config: {0}")]
    Parse(#[from] toml::de::Error),
    /// Logical validation failure (e.g. `poll_interval_ms == 0`).
    #[error("invalid reclaim-agent config: {0}")]
    Invalid(String),
}

// ============================================================================
// Match / detection
// ============================================================================

/// Where the match was observed — `/proc/<pid>/comm` (exact basename)
/// or `/proc/<pid>/cmdline` (argv substring).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchSource {
    /// Matched against `/proc/<pid>/comm`.
    Comm,
    /// Matched against `/proc/<pid>/cmdline`.
    Argv,
}

impl MatchSource {
    /// Short tag used in the reason annotation. Kept stable because
    /// downstream audit tooling parses it.
    #[must_use]
    pub fn tag(self) -> &'static str {
        // Both sources share the same operator-facing tag; the enum
        // variant is kept for internal attribution and logging only.
        match self {
            MatchSource::Comm | MatchSource::Argv => "process-match",
        }
    }
}

/// A single detected process match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Match {
    /// Process ID of the matched process.
    pub pid: u32,
    /// The pattern from the config that matched.
    pub matched_pattern: String,
    /// Which `/proc` surface produced the match.
    pub source: MatchSource,
}

/// Scan `/proc` (or a fixture root in tests) for the first process that
/// matches the config. Returns `Ok(None)` if nothing matches.
///
/// # Errors
/// Returns an error if the proc root does not exist or cannot be listed.
/// Per-pid read failures (from race conditions — processes exit during
/// the scan) are tolerated silently so a noisy node doesn't break the
/// whole loop.
pub fn scan_proc(proc_root: &Path, config: &Config) -> Result<Option<Match>, io::Error> {
    if config.match_commands.is_empty() && config.match_argv_substrings.is_empty() {
        return Ok(None);
    }

    let entries = fs::read_dir(proc_root)?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };

        if let Some(m) = match_pid(proc_root, pid, config) {
            return Ok(Some(m));
        }
    }
    Ok(None)
}

fn match_pid(proc_root: &Path, pid: u32, config: &Config) -> Option<Match> {
    let pid_dir = proc_root.join(pid.to_string());

    if let Ok(comm_raw) = fs::read_to_string(pid_dir.join("comm")) {
        let comm = comm_raw.trim_end_matches('\n');
        for pattern in &config.match_commands {
            if comm == pattern {
                return Some(Match {
                    pid,
                    matched_pattern: pattern.clone(),
                    source: MatchSource::Comm,
                });
            }
        }
    }

    if let Ok(cmdline_raw) = fs::read(pid_dir.join("cmdline")) {
        // cmdline uses NUL as argv separator. Replace with space so
        // substring matching works across argv boundaries ("/opt/foo -x").
        let cmdline: String = cmdline_raw
            .into_iter()
            .map(|b| if b == 0 { ' ' } else { b as char })
            .collect();
        for pattern in &config.match_argv_substrings {
            if cmdline.contains(pattern) {
                return Some(Match {
                    pid,
                    matched_pattern: pattern.clone(),
                    source: MatchSource::Argv,
                });
            }
        }
    }

    None
}

// ============================================================================
// Patch body + idempotence
// ============================================================================

/// Build the JSON patch body used to PATCH the node's annotations. The
/// body is intentionally minimal — only `metadata.annotations` is
/// touched, so a strategic-merge / merge-patch application cannot clobber
/// labels, spec, or status written by kubelet or other controllers.
#[must_use]
pub fn build_patch_body(m: &Match, timestamp: &str) -> serde_json::Value {
    let reason = format!("{}: {}", m.source.tag(), m.matched_pattern);
    serde_json::json!({
        "metadata": {
            "annotations": {
                crate::constants::RECLAIM_REQUESTED_ANNOTATION: crate::constants::RECLAIM_REQUESTED_VALUE,
                crate::constants::RECLAIM_REASON_ANNOTATION: reason,
                crate::constants::RECLAIM_REQUESTED_AT_ANNOTATION: timestamp,
            }
        }
    })
}

/// Test whether the `Node` already carries the reclaim request
/// annotation set to the literal `"true"` value. The agent uses this to
/// exit idempotently: a second firing must not overwrite the original
/// timestamp / reason.
#[must_use]
pub fn already_requested(annotations: &BTreeMap<String, String>) -> bool {
    annotations
        .get(crate::constants::RECLAIM_REQUESTED_ANNOTATION)
        .is_some_and(|v| v == crate::constants::RECLAIM_REQUESTED_VALUE)
}

// Re-export under the module path so tests can write
// `use super::super::RECLAIM_CONFIG_DATA_KEY` without reaching into
// `crate::constants::…` explicitly. Keeps the ConfigMap contract owned by
// the reclaim_agent module at the source-of-truth level.
pub use crate::constants::RECLAIM_CONFIG_DATA_KEY;

// ============================================================================
// ConfigMap → Config bridge (reactive watch path)
// ============================================================================

// ============================================================================
// Host-identity verification — Phase 4 of the 2026-04-25 security audit
//
// Closes the "modified DaemonSet hard-codes NODE_NAME" attack: an
// attacker with `update daemonsets` could change the agent's NODE_NAME
// env var to a victim node, causing the agent to PATCH the wrong Node
// with reclaim annotations. The fix cross-checks /etc/machine-id (the
// host's stable identifier set by systemd-machine-id-setup / kairos /
// k0s-installer) against the target Node's status.nodeInfo.machineID
// (which kubelet populates from the same source). Mismatch ⇒ refuse
// to patch.
//
// The exploit precondition (`update daemonsets`) is cluster-admin in
// most clusters, so this is defence-in-depth — but the binding makes
// the cross-check cheap and removes a category of impersonation.
// ============================================================================

/// Errors returned by host-identity verification.
#[derive(Debug, thiserror::Error)]
pub enum HostIdentityError {
    /// The machine-id file could not be read (e.g. missing mount,
    /// permission denied). The agent should refuse to patch.
    #[error("cannot read host machine-id from {path}: {source}")]
    ReadFailed {
        /// Path the agent tried to read (default `/etc/machine-id`).
        path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The machine-id file exists but is empty or whitespace-only —
    /// indistinguishable from "no host identity" and must fail closed.
    #[error("host machine-id at {path} is empty or whitespace-only")]
    Empty {
        /// Path to the offending file.
        path: String,
    },
    /// The fetched Node's `status.nodeInfo.machineID` does not match
    /// the agent's host machine-id. This is the attack-blocking case —
    /// either the DaemonSet was tampered with to point at a wrong
    /// Node, or kubelet+/etc/machine-id are inconsistent on the host.
    #[error(
        "host identity mismatch — refusing to patch wrong Node: agent /etc/machine-id={host_id:?}, \
         Node/{node_name}.status.nodeInfo.machineID={node_id:?}"
    )]
    Mismatch {
        /// Machine-id read from the agent's filesystem.
        host_id: String,
        /// Machine-id reported by the target Node's kubelet.
        node_id: String,
        /// Name of the Node the agent was about to patch.
        node_name: String,
    },
    /// The Node has no `status.nodeInfo.machineID` (kubelet hasn't
    /// populated it yet, or status is absent). Fail closed: we cannot
    /// verify identity.
    #[error("Node/{node_name} has no status.nodeInfo.machineID; cannot verify host identity")]
    NodeMachineIdMissing {
        /// Name of the Node missing the field.
        node_name: String,
    },
}

/// Read the host machine-id from disk and return it trimmed.
///
/// In production the path is `/etc/machine-id`; tests pass a tempfile.
/// The file is single-line `man machine-id` format (32 hex digits +
/// trailing newline) — we trim whitespace but otherwise accept any
/// non-empty content for compatibility with kairos / k0s-installer
/// variants that may format slightly differently.
///
/// # Errors
/// - [`HostIdentityError::ReadFailed`] if the file cannot be read.
/// - [`HostIdentityError::Empty`] if the file is empty or contains only
///   whitespace.
pub fn read_host_machine_id(path: &Path) -> Result<String, HostIdentityError> {
    let raw = fs::read_to_string(path).map_err(|e| HostIdentityError::ReadFailed {
        path: path.display().to_string(),
        source: e,
    })?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(HostIdentityError::Empty {
            path: path.display().to_string(),
        });
    }
    Ok(trimmed.to_string())
}

/// Compare a Node's `status.nodeInfo.machineID` against an expected
/// host machine-id.
///
/// Pure — the caller fetches the Node via the kube API, then this
/// function does the comparison and produces a precise error message
/// useful for forensics (both ids appear in the message so an
/// operator chasing a security alert sees the spoofed vs expected
/// values directly).
///
/// # Errors
/// - [`HostIdentityError::NodeMachineIdMissing`] when the Node has no
///   `status`, no `nodeInfo`, or an empty/whitespace-only `machineID`.
/// - [`HostIdentityError::Mismatch`] when both values are present and
///   differ.
pub fn compare_machine_ids(
    node: &k8s_openapi::api::core::v1::Node,
    node_name: &str,
    expected_host_id: &str,
) -> Result<(), HostIdentityError> {
    let node_id_raw = node
        .status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.machine_id.as_str());

    let node_id = match node_id_raw {
        Some(s) => s.trim(),
        None => {
            return Err(HostIdentityError::NodeMachineIdMissing {
                node_name: node_name.to_string(),
            })
        }
    };

    if node_id.is_empty() {
        return Err(HostIdentityError::NodeMachineIdMissing {
            node_name: node_name.to_string(),
        });
    }
    if node_id != expected_host_id {
        return Err(HostIdentityError::Mismatch {
            host_id: expected_host_id.to_string(),
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
        });
    }
    Ok(())
}

/// Translate a `ConfigMap` (as observed by a kube watcher) into an
/// `Option<Config>`:
///
/// * `Ok(Some(cfg))` — the ConfigMap carries a well-formed TOML payload at
///   [`RECLAIM_CONFIG_DATA_KEY`]; the caller should arm the scanner.
/// * `Ok(None)` — the ConfigMap exists but has no `reclaim.toml` entry
///   (e.g. the controller projected an empty `spec.killIfCommands`, or an
///   operator pre-created the CM without data). The caller should idle.
/// * `Err(_)` — the payload is present but malformed. The caller should
///   log and hold whatever last-good config it already had; losing arming
///   state on a bad edit would be a worse failure mode than running stale.
///
/// Pure — no I/O, no async. The watcher wires the stream, this function
/// just bridges each event into something the scanner loop can consume.
///
/// # Errors
/// Returns [`ConfigError::Parse`] or [`ConfigError::Invalid`] when the
/// `reclaim.toml` payload fails [`parse_config`] validation.
pub fn configmap_to_config(
    cm: &k8s_openapi::api::core::v1::ConfigMap,
) -> Result<Option<Config>, ConfigError> {
    let Some(data) = cm.data.as_ref() else {
        return Ok(None);
    };
    let Some(toml_body) = data.get(RECLAIM_CONFIG_DATA_KEY) else {
        return Ok(None);
    };
    parse_config(toml_body).map(Some)
}

#[cfg(test)]
#[path = "reclaim_agent_tests.rs"]
mod tests;
