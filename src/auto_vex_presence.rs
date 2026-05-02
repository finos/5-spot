// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Presence-based auto-VEX generation (roadmap Phase 2).
//!
//! Given a Grype JSON report, one or more CycloneDX SBOMs, and the set of
//! already-triaged CVE identifiers from `.vex/*.json`, compute an OpenVEX
//! document containing `not_affected + component_not_present` statements
//! for every Grype finding whose affected purl is absent from every SBOM.
//!
//! The rule is deliberately narrow: `component_not_present` is the one
//! OpenVEX justification with a purely mechanical definition — the SBOM
//! is the definition of what's in the product. Everything else stays
//! human-authored.
//!
//! This module contains pure logic only; I/O is driven by the thin CLI
//! wrapper in `src/bin/auto_vex_presence.rs`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────────
// Grype JSON schema (only the fields this module needs)
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GrypeReport {
    pub matches: Vec<GrypeMatch>,
}

#[derive(Debug, Deserialize)]
pub struct GrypeMatch {
    pub vulnerability: GrypeVuln,
    pub artifact: GrypeArtifact,
}

#[derive(Debug, Deserialize)]
pub struct GrypeVuln {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct GrypeArtifact {
    pub purl: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────
// CycloneDX SBOM schema (only the fields this module needs)
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Sbom {
    pub components: Option<Vec<SbomComponent>>,
}

#[derive(Debug, Deserialize)]
pub struct SbomComponent {
    pub purl: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────
// OpenVEX document schema (subset we emit — same shape vexctl merge reads)
// ─────────────────────────────────────────────────────────────────────────

/// OpenVEX v0.2.0 context URI.
const OPENVEX_CONTEXT: &str = "https://openvex.dev/ns/v0.2.0";

/// OpenVEX document spec version.
const OPENVEX_VERSION: u32 = 1;

/// Fixed impact-statement attached to every auto-generated statement. The
/// string is audit-visible so reviewers can distinguish machine-authored
/// suppressions from human-authored ones at a glance.
const AUTO_IMPACT_STATEMENT: &str =
    "Auto-derived by auto-vex-presence: the vulnerable component's package \
     URL is not present in any release SBOM, so the CVE cannot be exploited \
     against this product.";

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    #[serde(rename = "@context")]
    pub context: String,
    #[serde(rename = "@id")]
    pub id: String,
    pub author: String,
    pub timestamp: String,
    pub version: u32,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Statement {
    pub vulnerability: Vuln,
    pub products: Vec<Product>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justification: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_statement: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Vuln {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Product {
    #[serde(rename = "@id")]
    pub id: String,
}

// ─────────────────────────────────────────────────────────────────────────
// Core logic
// ─────────────────────────────────────────────────────────────────────────

/// Compute the set of auto-VEX statements to emit.
///
/// Emits one `not_affected + component_not_present` statement per unique
/// Grype CVE whose affected `purl` is not found in any SBOM and is not
/// already covered by a hand-authored `.vex/*.json` statement.
///
/// Output is sorted by CVE identifier so byte-identical inputs produce
/// byte-identical outputs (important for diffable CI artifacts).
pub fn compute_presence_vex(
    grype: &GrypeReport,
    sboms: &[Sbom],
    already_triaged: &HashSet<String>,
    product_purl: &str,
    statement_timestamp: &str,
) -> Vec<Statement> {
    let sbom_purls: HashSet<&str> = sboms
        .iter()
        .flat_map(|s| s.components.iter().flatten())
        .filter_map(|c| c.purl.as_deref())
        .collect();

    // BTreeMap keyed on CVE id → deterministic sorted output + implicit de-dup.
    let mut emitted: BTreeMap<String, Statement> = BTreeMap::new();
    for m in &grype.matches {
        let cve = &m.vulnerability.id;
        if already_triaged.contains(cve) {
            continue;
        }
        if emitted.contains_key(cve) {
            continue;
        }
        let artifact_purl = match m.artifact.purl.as_deref() {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };
        if sbom_purls.contains(artifact_purl) {
            continue;
        }
        emitted.insert(
            cve.clone(),
            Statement {
                vulnerability: Vuln { name: cve.clone() },
                products: vec![Product {
                    id: product_purl.to_string(),
                }],
                status: "not_affected".to_string(),
                justification: Some("component_not_present".to_string()),
                impact_statement: Some(AUTO_IMPACT_STATEMENT.to_string()),
                timestamp: statement_timestamp.to_string(),
            },
        );
    }
    emitted.into_values().collect()
}

/// Wrap a set of statements in the OpenVEX envelope. `@id`, `author`, and
/// `timestamp` are supplied by the caller (typically from the CI context).
pub fn build_document(
    statements: Vec<Statement>,
    id: &str,
    author: &str,
    timestamp: &str,
) -> Document {
    Document {
        context: OPENVEX_CONTEXT.to_string(),
        id: id.to_string(),
        author: author.to_string(),
        timestamp: timestamp.to_string(),
        version: OPENVEX_VERSION,
        statements,
    }
}

/// Read every non-dotfile `*.json` file in `vex_dir` and return the set
/// of `vulnerability.name` values across all statements. Non-existent
/// directories are treated as "nothing triaged yet" (permissive) so
/// that fresh repos work without special-casing. Malformed JSON
/// surfaces as an error so bad files don't silently become unsuppressed
/// findings.
///
/// Dot-prefixed files (e.g., `.affected-functions.json`, the curated
/// CVE → function mapping consumed by Phase 3) are skipped — they
/// match the `*.json` extension but are sidecar config, not OpenVEX
/// statements. Same convention shell globs use.
pub fn load_triaged_from_vex_dir(vex_dir: &Path) -> std::io::Result<HashSet<String>> {
    let mut triaged = HashSet::new();
    let entries = match std::fs::read_dir(vex_dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(triaged),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        // Skip dot-prefixed files (sidecar config, not statements).
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
        {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let doc: Document = serde_json::from_slice(&bytes).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {}", path.display(), e),
            )
        })?;
        for stmt in doc.statements {
            triaged.insert(stmt.vulnerability.name);
        }
    }
    Ok(triaged)
}

#[cfg(test)]
#[path = "auto_vex_presence_tests.rs"]
mod tests;
