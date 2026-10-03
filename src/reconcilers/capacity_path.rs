// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Capacity field paths: validation, patch construction, readback (ADR 0011)
//!
//! Pure, no I/O, so every rejection below is exhaustively unit-testable
//! without a cluster.
//!
//! ## Why this is a security module, not a string utility
//!
//! `ScheduledCapacity.spec.capacity.path` names a field on an object **5-Spot
//! does not own**, and the value comes from whoever can create a
//! `ScheduledCapacity`. Left unconstrained it would be an arbitrary-write
//! primitive against a foreign API group. The constraints:
//!
//! - **Charset**: dot-separated camelCase segments
//!   (`[a-z][a-zA-Z0-9]*`). No array indices, wildcards, `..`, quotes, spaces,
//!   `/`, `~` or `$`, so a path can never be read as a JSON Pointer, a JSONPath
//!   expression, or a traversal.
//! - **Prefix**: a write path must start with `spec.`. `metadata.` is rejected
//!   because writing `ownerReferences`, `finalizers` or labels on a foreign
//!   object is an elevation primitive rather than a capacity knob; `status.` is
//!   rejected because a status is a controller's own report. A drain path must
//!   start with `status.` for the mirror-image reason: reading back the `spec`
//!   value 5-Spot just wrote would make handback complete instantly and
//!   falsely.
//! - **Depth**: at most [`CAPACITY_PATH_MAX_SEGMENTS`] segments, bounding the
//!   nesting a merge patch will construct.
//!
//! The CRD schema enforces the same rules at admission
//! ([`CAPACITY_PATH_PATTERN`](crate::constants::CAPACITY_PATH_PATTERN) plus a
//! CEL prefix rule). **This module is not a duplicate of that check, it is the
//! one that still holds** when the deployed CRD is out of date relative to the
//! running controller, which is exactly the situation a schema cannot defend
//! against.

use serde_json::{Map, Value};

use crate::constants::{
    CAPACITY_DRAINED_PATH_PREFIX, CAPACITY_PATH_MAX_SEGMENTS, CAPACITY_WRITE_PATH_PREFIX,
};

/// Maximum total length of a capacity path, matching the CRD schema's
/// `maxLength` so the two layers reject the same inputs.
const CAPACITY_PATH_MAX_LEN: usize = 253;

/// Why a capacity path was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapacityPathError {
    /// The path was empty.
    #[error("capacity path is empty")]
    Empty,

    /// The path exceeded [`CAPACITY_PATH_MAX_LEN`].
    #[error("capacity path is {len} characters, the maximum is {max}")]
    TooLong {
        /// Actual length.
        len: usize,
        /// Permitted maximum.
        max: usize,
    },

    /// The path had more dot-separated segments than permitted.
    #[error("capacity path has {found} segments, the maximum is {max}")]
    TooManySegments {
        /// Actual segment count.
        found: usize,
        /// Permitted maximum.
        max: usize,
    },

    /// A segment was not a camelCase Kubernetes field name.
    #[error(
        "capacity path segment '{segment}' is not a camelCase field name \
         (must match [a-z][a-zA-Z0-9]*)"
    )]
    InvalidSegment {
        /// The offending segment.
        segment: String,
    },

    /// The path did not start with the required prefix.
    #[error("capacity path '{found}' must start with '{expected}'")]
    WrongPrefix {
        /// The required prefix.
        expected: &'static str,
        /// The path as supplied.
        found: String,
    },
}

/// Validate `path` against the charset and depth rules and split it into
/// segments. Prefix-agnostic: callers go through [`validate_write_path`] or
/// [`validate_drained_path`].
///
/// # Errors
/// [`CapacityPathError::Empty`], [`CapacityPathError::TooLong`],
/// [`CapacityPathError::TooManySegments`], or
/// [`CapacityPathError::InvalidSegment`].
fn validate_segments(path: &str) -> Result<Vec<&str>, CapacityPathError> {
    if path.is_empty() {
        return Err(CapacityPathError::Empty);
    }
    if path.len() > CAPACITY_PATH_MAX_LEN {
        return Err(CapacityPathError::TooLong {
            len: path.len(),
            max: CAPACITY_PATH_MAX_LEN,
        });
    }

    // `split('.')` yields an empty segment for a leading, trailing, or doubled
    // dot, each of which fails the charset check below. That is deliberate:
    // there is no separate "malformed dot" error because an empty segment is
    // already not a field name.
    let segments: Vec<&str> = path.split('.').collect();
    if segments.len() > CAPACITY_PATH_MAX_SEGMENTS {
        return Err(CapacityPathError::TooManySegments {
            found: segments.len(),
            max: CAPACITY_PATH_MAX_SEGMENTS,
        });
    }

    for segment in &segments {
        if !is_camel_case_field(segment) {
            return Err(CapacityPathError::InvalidSegment {
                segment: (*segment).to_string(),
            });
        }
    }
    Ok(segments)
}

/// `true` if `segment` is a camelCase Kubernetes field name: an ASCII lowercase
/// first character followed by ASCII alphanumerics only.
///
/// Deliberately an allowlist rather than a denylist of dangerous characters: a
/// denylist is a bet that the list is complete, and this value reaches a
/// foreign API group.
fn is_camel_case_field(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

/// Validate a **write** path: the charset and depth rules plus the `spec.`
/// prefix.
///
/// # Errors
/// [`CapacityPathError::WrongPrefix`] when the path does not start with
/// `spec.`, plus anything [`validate_segments`] returns.
pub fn validate_write_path(path: &str) -> Result<Vec<&str>, CapacityPathError> {
    validate_prefixed(path, CAPACITY_WRITE_PATH_PREFIX)
}

/// Validate a **drain** path: the charset and depth rules plus the `status.`
/// prefix.
///
/// # Errors
/// [`CapacityPathError::WrongPrefix`] when the path does not start with
/// `status.`, plus anything [`validate_segments`] returns.
pub fn validate_drained_path(path: &str) -> Result<Vec<&str>, CapacityPathError> {
    validate_prefixed(path, CAPACITY_DRAINED_PATH_PREFIX)
}

/// Shared implementation: prefix check first (the cheap, decisive one), then
/// charset and depth.
fn validate_prefixed<'a>(
    path: &'a str,
    prefix: &'static str,
) -> Result<Vec<&'a str>, CapacityPathError> {
    if path.is_empty() {
        return Err(CapacityPathError::Empty);
    }
    // `starts_with(prefix)` where prefix includes the dot, so a bare "spec"
    // fails here rather than passing a substring check.
    if !path.starts_with(prefix) {
        return Err(CapacityPathError::WrongPrefix {
            expected: prefix,
            found: path.to_string(),
        });
    }
    validate_segments(path)
}

/// Build the JSON **merge patch** that sets the validated `path` to `value`,
/// nesting one object per segment.
///
/// `spec.warmReplicas` with `10` yields `{"spec":{"warmReplicas":10}}`, which
/// touches that field and nothing else. Merge patch rather than server-side
/// apply on purpose (ADR 0011): SSA would make 5-Spot a permanent co-owner of
/// the field and fight the consumer's own field manager on every reconcile.
///
/// # Errors
/// Anything [`validate_write_path`] returns. The path is re-validated here so
/// this function cannot become a second way past the gate.
pub fn build_merge_patch(path: &str, value: i64) -> Result<Value, CapacityPathError> {
    let segments = validate_write_path(path)?;

    // Build from the leaf outward: the innermost value first, then wrap it in
    // one object per remaining segment.
    let mut node = Value::from(value);
    for segment in segments.iter().rev() {
        let mut object = Map::new();
        object.insert((*segment).to_string(), node);
        node = Value::Object(object);
    }
    Ok(node)
}

/// Read the integer at `path` in `object`, or `None` when the path is absent,
/// an intermediate is not an object, or the leaf is not an integer.
///
/// `Some(0)` and `None` are deliberately different answers: the whole handback
/// decision turns on telling "the consumer reports nothing in use" from "the
/// consumer reports nothing at all", and collapsing them would complete a
/// handback that never happened.
///
/// Accepts unsigned values too, since consumer counters are commonly `uint32`
/// on the wire.
#[must_use]
pub fn read_i64_at(object: &Value, path: &str) -> Option<i64> {
    if path.is_empty() {
        return None;
    }
    let mut node = object;
    for segment in path.split('.') {
        node = node.get(segment)?;
    }
    node.as_i64().or_else(|| {
        node.as_u64()
            .and_then(|unsigned| i64::try_from(unsigned).ok())
    })
}

#[cfg(test)]
#[path = "capacity_path_tests.rs"]
mod tests;
