// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! YAML output for the manifests 5-Spot generates (the CRDs in `deploy/crds/`).
//!
//! One serializer configuration for every caller, so generated files share a
//! style: long strings stay on one line (no `>-` folding), and no line ends in
//! whitespace.
//!
//! ## Why this module exists
//!
//! `serde_yaml` is deprecated and its `unsafe-libyaml` backend is unmaintained
//! (RUSTSEC-2024-0449), which `cargo deny` fails on. `serde-saphyr` is a
//! safe-Rust replacement, but its defaults differ from `serde_yaml`'s in two
//! ways that would churn every generated manifest:
//!
//! 1. strings longer than `folded_wrap_chars` (80 by default) are rewritten as
//!    folded block scalars (`>-`), so a long CRD description would be rewrapped
//!    across lines;
//! 2. blank lines inside a block scalar are written with the block's
//!    indentation, leaving trailing whitespace on otherwise-empty lines.
//!
//! Both are handled here rather than at each call site, so there is exactly one
//! place that decides what generated YAML looks like. The same wrapper, with the
//! same two fixes, is used in the sibling `bindy` and `banlieue` projects.

use serde::Serialize;

/// Serialize `value` to YAML in 5-Spot's manifest style.
///
/// Long single-line strings are kept on one line, and the indentation the
/// emitter writes on blank lines inside block scalars is removed. A line made
/// only of indentation reads back as an empty line, so the parsed value is
/// unchanged by that removal.
///
/// # Arguments
/// * `value` - any `Serialize` value; in practice a `CustomResourceDefinition`
///
/// # Errors
/// Returns the serializer's error if `value` cannot be represented as YAML.
pub fn to_string<T: Serialize>(value: &T) -> Result<String, serde_saphyr::SerializeError> {
    // `SerializerOptions` is #[non_exhaustive], so it cannot be built with a
    // struct expression from outside the crate; mutate the default instead.
    let mut options = serde_saphyr::SerializerOptions::default();
    // Never fold a long scalar onto continuation lines. `serde_yaml` did not
    // fold, and folding would rewrite every long CRD description.
    options.folded_wrap_chars = usize::MAX;
    let yaml = serde_saphyr::to_string_with_options(value, options)?;
    Ok(strip_blank_line_indentation(&yaml))
}

/// Remove the whitespace from lines that contain nothing else.
///
/// Keeps the line terminator so line counts are unchanged; only the spaces on
/// an otherwise-empty line go away.
fn strip_blank_line_indentation(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        if body.trim().is_empty() {
            out.push_str(newline);
            continue;
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
#[path = "yaml_tests.rs"]
mod tests;
