// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Symbol-import-based auto-VEX (roadmap Phase 3).
//!
//! Given a Grype JSON report, the dynamic-symbol imports of the release
//! binary (`nm -D --undefined-only`), a curated CVE → [function names]
//! mapping, and the set of already-triaged CVEs from `.vex/*.json`,
//! emit `not_affected + vulnerable_code_not_in_execute_path` statements
//! for each Grype CVE whose mapping function names are *all absent*
//! from the binary's imports.
//!
//! ## Why symbol-imports, not LLVM-IR call graphs
//!
//! The original Phase 3 roadmap targeted Rust LLVM-IR call-graph
//! reachability against RustSec advisories carrying `affected.functions`.
//! At the time of implementation `cargo audit` reports zero open
//! vulnerabilities for this codebase — every CVE in `.vex/` is a
//! base-image glibc/zlib finding from Grype scanning the Docker image,
//! none of which are tracked by RustSec. The Rust call-graph approach
//! addresses zero current findings.
//!
//! The mechanical equivalent for our actual data: check whether the
//! Rust binary's dynamic-symbol-import table (the public C API entry
//! points it actually links against) contains any of the affected
//! library functions. If none → the binary cannot reach those code
//! paths through documented entry points. The Security team verifies
//! by re-running `nm` against the same release-attested binary and
//! diffing.
//!
//! Pure logic only; I/O is driven by the CLI wrapper in
//! `src/bin/auto_vex_reachability.rs`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

// Re-export the OpenVEX shape and Grype shape from auto_vex_presence so
// callers (tests, the bin) can access them through this module without
// having to reach into both. These types are stable across phases.
pub use crate::auto_vex_presence::{
    build_document, load_triaged_from_vex_dir, Document, GrypeArtifact, GrypeMatch, GrypeReport,
    GrypeVuln, Product, Statement, Vuln,
};

/// Fixed impact-statement template for auto-derived statements. The
/// affected-function list is interpolated so the audit trail records
/// exactly which symbols the analyzer checked. Helps the Security
/// team verify the claim by reading the statement alone.
const AUTO_IMPACT_PREFIX: &str =
    "Auto-derived by auto-vex-reachability: the Rust controller binary's \
     dynamic symbol-import table contains none of the documented entry \
     points for this CVE";

/// Parse `nm -D --undefined-only <binary>` text output into a set of
/// imported symbol names, with `@VERSION` GLIBC suffixes stripped.
///
/// Defensively skips any line whose type column isn't `U`, in case the
/// caller passed full `nm -D` output instead of `--undefined-only`.
pub fn parse_nm_output(input: &str) -> HashSet<String> {
    let mut symbols = HashSet::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Find the type column. nm output is `<addr-or-spaces> <T> <name>`
        // where <T> is a single character ('U' = undefined). Tokenize
        // and look for a 1-char token followed by the symbol name.
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let (type_char_idx, _) = match tokens
            .iter()
            .enumerate()
            .find(|(_, t)| t.len() == 1 && t.chars().next().unwrap().is_ascii_alphabetic())
        {
            Some(p) => p,
            None => continue,
        };
        if tokens[type_char_idx] != "U" {
            continue;
        }
        let name = match tokens.get(type_char_idx + 1) {
            Some(n) => n,
            None => continue,
        };
        // Strip `@VERSION` suffix (e.g., `scanf@GLIBC_2.7` → `scanf`).
        let bare = match name.find('@') {
            Some(idx) => &name[..idx],
            None => name,
        };
        if !bare.is_empty() {
            symbols.insert(bare.to_string());
        }
    }
    symbols
}

/// Read the CVE → [function names] mapping from a JSON file. Keys whose
/// value is not an array of strings are silently skipped, so the file
/// can carry sidecar metadata (`_comment: [...]`, `_meta: "..."`) next
/// to real entries without breaking parsing.
pub fn load_affected_functions_from_path(
    path: &Path,
) -> std::io::Result<BTreeMap<String, Vec<String>>> {
    let bytes = std::fs::read(path)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{}: {}", path.display(), e),
        )
    })?;
    let obj = match raw.as_object() {
        Some(o) => o,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: top level must be a JSON object", path.display()),
            ));
        }
    };
    let mut out = BTreeMap::new();
    for (k, v) in obj {
        // Underscore-prefixed keys are metadata (`_comment`, `_meta`, etc.)
        // and never identifier names — skip them. Even if such a key's
        // value happens to be an array of strings, those strings are
        // free-form prose, not function names.
        if k.starts_with('_') {
            continue;
        }
        let arr = match v.as_array() {
            Some(a) => a,
            None => continue,
        };
        let fns: Option<Vec<String>> = arr
            .iter()
            .map(|item| item.as_str().map(|s| s.to_string()))
            .collect();
        if let Some(fns) = fns {
            out.insert(k.clone(), fns);
        }
    }
    Ok(out)
}

/// Core reachability check. Emits one statement per unique Grype CVE
/// where (a) the CVE is in the mapping, (b) the mapping's function
/// list is non-empty, (c) NONE of the listed functions appear in
/// `imported_symbols`, and (d) the CVE isn't already in
/// `already_triaged`. Output is sorted by CVE id for deterministic
/// CI artifacts.
pub fn compute_reachability_vex(
    grype: &GrypeReport,
    imported_symbols: &HashSet<String>,
    affected_functions: &BTreeMap<String, Vec<String>>,
    already_triaged: &HashSet<String>,
    product_purl: &str,
    statement_timestamp: &str,
) -> Vec<Statement> {
    let mut emitted: BTreeSet<String> = BTreeSet::new();
    let mut statements: BTreeMap<String, Statement> = BTreeMap::new();
    for m in &grype.matches {
        let cve = &m.vulnerability.id;
        if already_triaged.contains(cve) {
            continue;
        }
        if emitted.contains(cve) {
            continue;
        }
        let fns = match affected_functions.get(cve) {
            Some(f) if !f.is_empty() => f,
            _ => continue,
        };
        // If ANY listed function is imported, abandon — the binary may
        // reach the affected code path, leave for human triage.
        if fns.iter().any(|f| imported_symbols.contains(f)) {
            continue;
        }
        emitted.insert(cve.clone());
        let impact = format!(
            "{} ({}). The dynamic symbol-import table was inspected via \
             `nm -D --undefined-only` against the release artifact; \
             none of the listed entry points were found.",
            AUTO_IMPACT_PREFIX,
            fns.join(", ")
        );
        statements.insert(
            cve.clone(),
            Statement {
                vulnerability: Vuln { name: cve.clone() },
                products: vec![Product {
                    id: product_purl.to_string(),
                }],
                status: "not_affected".to_string(),
                justification: Some("vulnerable_code_not_in_execute_path".to_string()),
                impact_statement: Some(impact),
                timestamp: statement_timestamp.to_string(),
            },
        );
    }
    statements.into_values().collect()
}

#[cfg(test)]
#[path = "auto_vex_reachability_tests.rs"]
mod tests;
