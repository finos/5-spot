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
