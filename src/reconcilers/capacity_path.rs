// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Capacity fields: validation, spec construction, readback (ADR 0011/0016)
//!
//! Pure, no I/O, so every rejection below is exhaustively unit-testable
//! without a cluster.
//!
//! ## What changed, and why this is no longer a security module
//!
//! Under ADR 0011 `spec.capacity.path` named a field on an object **5-Spot did
//! not own**, which made this module an arbitrary-write gate against a foreign
//! API group: the `spec.` prefix rule existed so a `ScheduledCapacity` author
//! could not aim 5-Spot's ServiceAccount at `metadata.ownerReferences` on
//! somebody else's object.
//!
//! ADR 0016 made 5-Spot **create and own** that object, and
//! `spec.capacity.field` became a path **relative to the owned object's
//! `spec`**. The dangerous prefixes are now unreachable rather than rejected:
//! whatever the field names is nested under a `spec` that 5-Spot built, so
//! there is no `metadata` in scope to reach. What is left is schema hygiene,
//! and it is kept strict anyway:
//!
//! - **Charset**: dot-separated camelCase segments (`[a-z][a-zA-Z0-9]*`). An
//!   allowlist, not a denylist of dangerous characters, because a denylist is a
//!   bet that the list is complete.
//! - **Reserved roots**: `spec`, `metadata` and `status` as a first segment are
//!   rejected, not because they are dangerous but because they are **mistakes**:
//!   `spec.warmReplicas` here would construct `spec.spec.warmReplicas`, and
//!   failing loudly beats building the wrong object.
//! - **Depth**: at most [`CAPACITY_FIELD_MAX_SEGMENTS`], one below
//!   [`CAPACITY_PATH_MAX_SEGMENTS`], because `spec` occupies the first position
//!   in the object actually constructed.
//! - **One source of truth**: the template may not set the knob itself, or the
//!   template and the schedule fight on every reconcile.
//!
//! The drain path keeps its `status.` prefix rule, and keeps it for the original
//! reason: it reads a **consumer's own report**, and reading back the `spec`
//! value 5-Spot just wrote would make handback complete instantly and falsely.
//!
//! The CRD schema enforces the charset and depth rules at admission. **This
//! module is not a duplicate of that check, it is the one that still holds**
//! when the deployed CRD is out of date relative to the running controller,
//! which is exactly the situation a schema cannot defend against.

use serde_json::{Map, Value};

use crate::constants::{
    CAPACITY_DRAINED_PATH_PREFIX, CAPACITY_FIELD_MAX_SEGMENTS, CAPACITY_PATH_MAX_SEGMENTS,
};

/// First segments rejected in `spec.capacity.field`. Each is already implicit
/// or structurally out of reach, so naming one is a mistake that would build the
/// wrong object rather than an attack that would reach a dangerous field.
const RESERVED_ROOT_SEGMENTS: &[&str] = &["spec", "metadata", "status"];

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

    /// A capacity field named a reserved root segment. `spec.warmReplicas`
    /// would construct `spec.spec.warmReplicas`, because the field is already
    /// relative to the owned object's `spec`.
    #[error(
        "capacity field '{found}' must be relative to the owned object's spec, so it \
         cannot start with the reserved segment '{segment}'"
    )]
    ReservedRootSegment {
        /// The offending first segment.
        segment: String,
        /// The field as supplied.
        found: String,
    },

    /// `spec.template` already sets the field the schedule owns.
    #[error(
        "spec.template already sets '{field}', which spec.capacity.field owns; remove it \
         from the template so the schedule is the only writer"
    )]
    TemplateSetsCapacityField {
        /// The contested field path.
        field: String,
    },

    /// The template, or an intermediate inside it, was not a JSON object.
    #[error("spec.template{at} is not an object, so '{field}' cannot be set in it")]
    NotAnObject {
        /// Where the non-object was found, as a readable suffix.
        at: String,
        /// The field being injected.
        field: String,
    },
}

/// Validate `path` against the charset rules and a caller-supplied depth cap,
/// and split it into segments. Prefix-agnostic: callers go through
/// [`validate_capacity_field`] or [`validate_drained_path`].
///
/// # Errors
/// [`CapacityPathError::Empty`], [`CapacityPathError::TooLong`],
/// [`CapacityPathError::TooManySegments`], or
/// [`CapacityPathError::InvalidSegment`].
fn validate_segments(path: &str, max_segments: usize) -> Result<Vec<&str>, CapacityPathError> {
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
    if segments.len() > max_segments {
        return Err(CapacityPathError::TooManySegments {
            found: segments.len(),
            max: max_segments,
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

/// Validate `spec.capacity.field`: the charset rules, a depth cap one below the
/// path cap, and no reserved root segment.
///
/// The path is **relative to the owned object's `spec`**, so there is no prefix
/// to require and `spec`, `metadata` or `status` in first position is a caller
/// mistake rather than an attack (ADR 0016 decision 3).
///
/// # Errors
/// [`CapacityPathError::ReservedRootSegment`] for an already-implicit or
/// out-of-reach root, plus anything [`validate_segments`] returns.
pub fn validate_capacity_field(field: &str) -> Result<Vec<&str>, CapacityPathError> {
    let segments = validate_segments(field, CAPACITY_FIELD_MAX_SEGMENTS)?;

    // Checked after the charset pass so a malformed field reports the more
    // specific error: "a-b is not a field name" beats "a-b has a bad root".
    let first = segments[0];
    if RESERVED_ROOT_SEGMENTS.contains(&first) {
        return Err(CapacityPathError::ReservedRootSegment {
            segment: first.to_string(),
            found: field.to_string(),
        });
    }
    Ok(segments)
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
    validate_segments(path, CAPACITY_PATH_MAX_SEGMENTS)
}

/// Build the owned object's `spec` by injecting `value` at `field` into a copy
/// of `template`.
///
/// `{"maxReplicas":20}` with `warmReplicas` and `10` yields
/// `{"maxReplicas":20,"warmReplicas":10}`. Missing intermediates are created;
/// existing ones are merged into rather than replaced, so a nested sibling in
/// the operator's template survives.
///
/// The result is applied by **server-side apply** as the complete spec 5-Spot
/// owns (ADR 0016 decision 4). That is sound precisely because there is no
/// second writer: 5-Spot created the object and the consumer's reconciler writes
/// only `status`. ADR 0011 had to use a merge patch instead, because there it
/// was writing into a stranger's object alongside the stranger's own field
/// manager.
///
/// # Errors
/// [`CapacityPathError::TemplateSetsCapacityField`] when the template already
/// sets `field` (one source of truth, or the template and the schedule fight on
/// every reconcile), [`CapacityPathError::NotAnObject`] when the template or an
/// intermediate is not a JSON object, plus anything
/// [`validate_capacity_field`] returns.
pub fn inject_capacity_field(
    template: &Value,
    field: &str,
    value: i64,
) -> Result<Value, CapacityPathError> {
    let segments = validate_capacity_field(field)?;

    let mut spec = template.clone();
    let Some(root) = spec.as_object_mut() else {
        return Err(CapacityPathError::NotAnObject {
            at: String::new(),
            field: field.to_string(),
        });
    };

    // Walk to the parent of the leaf, creating objects as needed. `segments` is
    // non-empty because validate_capacity_field rejects an empty path.
    let (leaf, parents) = segments
        .split_last()
        .expect("validate_capacity_field rejects an empty field");
    let mut node = root;
    for (depth, segment) in parents.iter().enumerate() {
        let entry = node
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        node = entry
            .as_object_mut()
            .ok_or_else(|| CapacityPathError::NotAnObject {
                at: format!(".{}", parents[..=depth].join(".")),
                field: field.to_string(),
            })?;
    }

    if node.contains_key(*leaf) {
        return Err(CapacityPathError::TemplateSetsCapacityField {
            field: field.to_string(),
        });
    }
    node.insert((*leaf).to_string(), Value::from(value));
    Ok(spec)
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
