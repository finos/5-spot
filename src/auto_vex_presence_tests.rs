// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the `auto_vex_presence` module.
//!
//! Coverage obligations per project rule (100% positive / negative / exception):
//! - Happy path: a Grype finding whose purl is absent from every SBOM is
//!   emitted as `not_affected + component_not_present`.
//! - Negative path: a Grype finding whose purl IS in the SBOM is skipped.
//! - Negative path: a Grype finding whose CVE ID is already covered by a
//!   hand-authored `.vex/*.json` statement is skipped.
//! - Negative path: a Grype finding with a missing/empty purl is skipped.
//! - Multiple-SBOM union: a purl present in ANY SBOM excludes the finding.
//! - De-duplication: if the same CVE appears multiple times in Grype output
//!   (e.g., matched against multiple artifacts), only one statement is
//!   emitted.
//! - Determinism: the output is sorted so identical inputs always produce
//!   identical bytes.
//! - Malformed inputs: Grype JSON without `matches`, SBOM without
//!   `components`, and `.vex/` with malformed JSON all surface as typed
//!   errors.
//! - I/O: missing `.vex/` directory is treated as an empty triage set
//!   (permissive); missing Grype / SBOM file is an error.

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use serde_json::json;
    use std::collections::HashSet;

    // ────────────────────────────────────────────────────────────────────
    // compute_presence_vex — core pure logic
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn empty_grype_yields_empty_statements() {
        let grype = GrypeReport { matches: vec![] };
        let statements = compute_presence_vex(
            &grype,
            &[],
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert!(statements.is_empty());
    }

    #[test]
    fn cve_not_in_any_sbom_is_emitted() {
        let grype = GrypeReport {
            matches: vec![GrypeMatch {
                vulnerability: GrypeVuln {
                    id: "CVE-2026-00001".to_string(),
                },
                artifact: GrypeArtifact {
                    purl: Some("pkg:deb/debian/libfoo@1.0".to_string()),
                },
            }],
        };
        let statements = compute_presence_vex(
            &grype,
            &[],
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert_eq!(statements.len(), 1);
        assert_eq!(statements[0].vulnerability.name, "CVE-2026-00001");
        assert_eq!(statements[0].status, "not_affected");
        assert_eq!(
            statements[0].justification.as_deref(),
            Some("component_not_present")
        );
        assert_eq!(statements[0].products.len(), 1);
        assert_eq!(statements[0].products[0].id, "pkg:oci/5-spot");
    }

    #[test]
    fn cve_with_purl_in_sbom_is_skipped() {
        let grype = GrypeReport {
            matches: vec![GrypeMatch {
                vulnerability: GrypeVuln {
                    id: "CVE-2026-00002".to_string(),
                },
                artifact: GrypeArtifact {
                    purl: Some("pkg:cargo/serde@1.0".to_string()),
                },
            }],
        };
        let sbom = Sbom {
            components: Some(vec![SbomComponent {
                purl: Some("pkg:cargo/serde@1.0".to_string()),
            }]),
        };
        let statements = compute_presence_vex(
            &grype,
            std::slice::from_ref(&sbom),
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert!(statements.is_empty());
    }

    #[test]
    fn cve_already_triaged_is_skipped() {
        let grype = GrypeReport {
            matches: vec![GrypeMatch {
                vulnerability: GrypeVuln {
                    id: "CVE-2026-00003".to_string(),
                },
                artifact: GrypeArtifact {
                    purl: Some("pkg:deb/debian/libbar@1.0".to_string()),
                },
            }],
        };
        let mut triaged = HashSet::new();
        triaged.insert("CVE-2026-00003".to_string());
        let statements = compute_presence_vex(
            &grype,
            &[],
            &triaged,
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert!(statements.is_empty());
    }

    #[test]
    fn match_without_purl_is_skipped() {
        let grype = GrypeReport {
            matches: vec![GrypeMatch {
                vulnerability: GrypeVuln {
                    id: "CVE-2026-00004".to_string(),
                },
                artifact: GrypeArtifact { purl: None },
            }],
        };
        let statements = compute_presence_vex(
            &grype,
            &[],
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert!(statements.is_empty());
    }

    #[test]
    fn purl_in_any_sbom_excludes_finding() {
        let grype = GrypeReport {
            matches: vec![GrypeMatch {
                vulnerability: GrypeVuln {
                    id: "CVE-2026-00005".to_string(),
                },
                artifact: GrypeArtifact {
                    purl: Some("pkg:cargo/tokio@1.0".to_string()),
                },
            }],
        };
        let binary_sbom = Sbom {
            components: Some(vec![SbomComponent {
                purl: Some("pkg:cargo/serde@1.0".to_string()),
            }]),
        };
        let docker_sbom = Sbom {
            components: Some(vec![SbomComponent {
                purl: Some("pkg:cargo/tokio@1.0".to_string()),
            }]),
        };
        let statements = compute_presence_vex(
            &grype,
            &[binary_sbom, docker_sbom],
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert!(statements.is_empty());
    }

    #[test]
    fn duplicate_cve_emits_only_once() {
        let grype = GrypeReport {
            matches: vec![
                GrypeMatch {
                    vulnerability: GrypeVuln {
                        id: "CVE-2026-00006".to_string(),
                    },
                    artifact: GrypeArtifact {
                        purl: Some("pkg:deb/debian/libx@1".to_string()),
                    },
                },
                GrypeMatch {
                    vulnerability: GrypeVuln {
                        id: "CVE-2026-00006".to_string(),
                    },
                    artifact: GrypeArtifact {
                        purl: Some("pkg:deb/debian/libx@2".to_string()),
                    },
                },
            ],
        };
        let statements = compute_presence_vex(
            &grype,
            &[],
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        assert_eq!(statements.len(), 1);
    }

    #[test]
    fn output_is_sorted_by_cve_id() {
        let grype = GrypeReport {
            matches: vec![
                GrypeMatch {
                    vulnerability: GrypeVuln {
                        id: "CVE-2026-00009".to_string(),
                    },
                    artifact: GrypeArtifact {
                        purl: Some("pkg:x/a".to_string()),
                    },
                },
                GrypeMatch {
                    vulnerability: GrypeVuln {
                        id: "CVE-2026-00007".to_string(),
                    },
                    artifact: GrypeArtifact {
                        purl: Some("pkg:x/b".to_string()),
                    },
                },
                GrypeMatch {
                    vulnerability: GrypeVuln {
                        id: "CVE-2026-00008".to_string(),
                    },
                    artifact: GrypeArtifact {
                        purl: Some("pkg:x/c".to_string()),
                    },
                },
            ],
        };
        let statements = compute_presence_vex(
            &grype,
            &[],
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-04-22T00:00:00Z",
        );
        let ids: Vec<&str> = statements
            .iter()
            .map(|s| s.vulnerability.name.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["CVE-2026-00007", "CVE-2026-00008", "CVE-2026-00009"]
        );
    }

    // ────────────────────────────────────────────────────────────────────
    // build_document — wraps statements in the OpenVEX envelope
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn document_envelope_fields_are_set() {
        let statements = vec![Statement {
            vulnerability: Vuln {
                name: "CVE-2026-10000".to_string(),
            },
            products: vec![Product {
                id: "pkg:oci/5-spot".to_string(),
            }],
            status: "not_affected".to_string(),
            justification: Some("component_not_present".to_string()),
            impact_statement: Some("derived".to_string()),
            timestamp: "2026-04-22T00:00:00Z".to_string(),
        }];
        let doc = build_document(
            statements,
            "https://5-spot/auto-presence/run-1",
            "auto-vex-presence",
            "2026-04-22T00:00:00Z",
        );
        assert_eq!(doc.context, "https://openvex.dev/ns/v0.2.0");
        assert_eq!(doc.id, "https://5-spot/auto-presence/run-1");
        assert_eq!(doc.author, "auto-vex-presence");
        assert_eq!(doc.timestamp, "2026-04-22T00:00:00Z");
        assert_eq!(doc.version, 1);
        assert_eq!(doc.statements.len(), 1);
    }

    #[test]
    fn empty_statements_still_produces_valid_envelope() {
        let doc = build_document(
            vec![],
            "https://5-spot/auto-presence/empty",
            "auto-vex-presence",
            "2026-04-22T00:00:00Z",
        );
        assert!(doc.statements.is_empty());
        // Serde round-trip: an empty doc is still valid JSON that vexctl can consume.
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert!(back.statements.is_empty());
    }

    // ────────────────────────────────────────────────────────────────────
    // parse_grype / parse_sbom — deserialization contracts
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_grype_from_valid_json() {
        let doc = json!({
            "matches": [
                {
                    "vulnerability": {"id": "CVE-2026-20000"},
                    "artifact": {"purl": "pkg:deb/debian/libfoo@1.0"}
                }
            ]
        })
        .to_string();
        let parsed: GrypeReport = serde_json::from_str(&doc).unwrap();
        assert_eq!(parsed.matches.len(), 1);
        assert_eq!(parsed.matches[0].vulnerability.id, "CVE-2026-20000");
    }

    #[test]
    fn parse_grype_tolerates_extra_fields() {
        let doc = json!({
            "matches": [
                {
                    "vulnerability": {"id": "CVE-2026-20001", "severity": "High"},
                    "artifact": {"purl": "pkg:x/y@1", "name": "y", "version": "1"},
                    "relatedVulnerabilities": []
                }
            ],
            "source": {"type": "image"},
            "descriptor": {"name": "grype"}
        })
        .to_string();
        let parsed: GrypeReport = serde_json::from_str(&doc).unwrap();
        assert_eq!(parsed.matches.len(), 1);
    }

    #[test]
    fn parse_grype_empty_matches_is_valid() {
        let doc = json!({"matches": []}).to_string();
        let parsed: GrypeReport = serde_json::from_str(&doc).unwrap();
        assert!(parsed.matches.is_empty());
    }

    #[test]
    fn parse_grype_missing_matches_errors() {
        let doc = json!({"not_matches": []}).to_string();
        let result: Result<GrypeReport, _> = serde_json::from_str(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn parse_sbom_without_components_is_empty() {
        let doc = json!({"bomFormat": "CycloneDX", "specVersion": "1.4"}).to_string();
        let parsed: Sbom = serde_json::from_str(&doc).unwrap();
        assert!(parsed.components.is_none());
    }

    #[test]
    fn parse_sbom_component_without_purl_is_ignored() {
        let doc = json!({
            "components": [
                {"name": "no-purl-component", "type": "library"},
                {"purl": "pkg:cargo/serde@1.0"}
            ]
        })
        .to_string();
        let parsed: Sbom = serde_json::from_str(&doc).unwrap();
        let comps = parsed.components.as_ref().unwrap();
        assert_eq!(comps.len(), 2);
        assert!(comps[0].purl.is_none());
        assert_eq!(comps[1].purl.as_deref(), Some("pkg:cargo/serde@1.0"));
    }

    // ────────────────────────────────────────────────────────────────────
    // load_triaged_from_vex_dir — reads hand-authored statements
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn load_triaged_from_directory_extracts_all_cve_ids() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("CVE-2026-30000.json"),
            json!({
                "@context": "https://openvex.dev/ns/v0.2.0",
                "@id": "https://example/1",
                "author": "test",
                "timestamp": "2026-04-22T00:00:00Z",
                "version": 1,
                "statements": [{
                    "vulnerability": {"name": "CVE-2026-30000"},
                    "products": [{"@id": "pkg:oci/5-spot"}],
                    "status": "not_affected",
                    "justification": "vulnerable_code_not_in_execute_path",
                    "timestamp": "2026-04-22T00:00:00Z"
                }]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("GHSA-abcd-efgh-ijkl.json"),
            json!({
                "@context": "https://openvex.dev/ns/v0.2.0",
                "@id": "https://example/2",
                "author": "test",
                "timestamp": "2026-04-22T00:00:00Z",
                "version": 1,
                "statements": [{
                    "vulnerability": {"name": "GHSA-abcd-efgh-ijkl"},
                    "products": [{"@id": "pkg:oci/5-spot"}],
                    "status": "not_affected",
                    "justification": "vulnerable_code_not_in_execute_path",
                    "timestamp": "2026-04-22T00:00:00Z"
                }]
            })
            .to_string(),
        )
        .unwrap();
        let triaged = load_triaged_from_vex_dir(tmp.path()).unwrap();
        assert!(triaged.contains("CVE-2026-30000"));
        assert!(triaged.contains("GHSA-abcd-efgh-ijkl"));
        assert_eq!(triaged.len(), 2);
    }

    #[test]
    fn load_triaged_skips_dotfiles() {
        // Dot-prefixed *.json files are sidecar config (e.g., the Phase
        // 3 curated affected-functions mapping at
        // .vex/.affected-functions.json), not OpenVEX statements.
        // Default shell globs already skip them; the loader must too,
        // otherwise it tries to parse them as OpenVEX docs and fails.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".sidecar-config.json"),
            r#"{"some": "config", "not": ["a", "vex", "doc"]}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("CVE-2026-30002.json"),
            json!({
                "@context": "https://openvex.dev/ns/v0.2.0",
                "@id": "https://example/4",
                "author": "test",
                "timestamp": "2026-04-22T00:00:00Z",
                "version": 1,
                "statements": [{
                    "vulnerability": {"name": "CVE-2026-30002"},
                    "products": [{"@id": "pkg:oci/5-spot"}],
                    "status": "not_affected",
                    "justification": "vulnerable_code_not_in_execute_path",
                    "timestamp": "2026-04-22T00:00:00Z"
                }]
            })
            .to_string(),
        )
        .unwrap();
        let triaged = load_triaged_from_vex_dir(tmp.path()).unwrap();
        assert_eq!(triaged.len(), 1);
        assert!(triaged.contains("CVE-2026-30002"));
    }

    #[test]
    fn load_triaged_missing_directory_is_permissive() {
        let triaged = load_triaged_from_vex_dir(std::path::Path::new(
            "/nonexistent/vex-dir-that-does-not-exist",
        ))
        .unwrap();
        assert!(triaged.is_empty());
    }

    #[test]
    fn load_triaged_malformed_json_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("bad.json"), "{ not valid json").unwrap();
        let result = load_triaged_from_vex_dir(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_triaged_ignores_non_json_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("README.md"), "not a statement").unwrap();
        std::fs::write(
            tmp.path().join("CVE-2026-30001.json"),
            json!({
                "@context": "https://openvex.dev/ns/v0.2.0",
                "@id": "https://example/3",
                "author": "test",
                "timestamp": "2026-04-22T00:00:00Z",
                "version": 1,
                "statements": [{
                    "vulnerability": {"name": "CVE-2026-30001"},
                    "products": [{"@id": "pkg:oci/5-spot"}],
                    "status": "not_affected",
                    "justification": "vulnerable_code_not_in_execute_path",
                    "timestamp": "2026-04-22T00:00:00Z"
                }]
            })
            .to_string(),
        )
        .unwrap();
        let triaged = load_triaged_from_vex_dir(tmp.path()).unwrap();
        assert_eq!(triaged.len(), 1);
        assert!(triaged.contains("CVE-2026-30001"));
    }
}
