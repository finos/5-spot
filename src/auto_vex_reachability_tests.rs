// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the `auto_vex_reachability` module.
//!
//! Coverage obligations per project rule (100% positive / negative /
//! exception):
//! - parse_nm_output: empty input, single-line, multi-line, lines with
//!   GLIBC version suffixes, mixed whitespace, blank/comment lines.
//! - load_affected_functions_from_path: valid file, empty `{}`, file with
//!   non-array values (e.g., `_comment` arrays / strings) silently
//!   ignored, missing file errors, malformed JSON errors.
//! - compute_reachability_vex: empty grype, CVE not in mapping skipped,
//!   CVE with all affected fns absent from symbols emitted,
//!   CVE with any affected fn imported NOT emitted,
//!   CVE already triaged skipped,
//!   duplicate CVE de-duped, deterministic sort by CVE id.

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use serde_json::json;
    use std::collections::HashSet;

    // ────────────────────────────────────────────────────────────────────
    // parse_nm_output
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_nm_empty_input_yields_empty_set() {
        let symbols = parse_nm_output("");
        assert!(symbols.is_empty());
    }

    #[test]
    fn parse_nm_single_undefined_symbol() {
        let symbols = parse_nm_output("                 U malloc\n");
        assert!(symbols.contains("malloc"));
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn parse_nm_strips_glibc_version_suffix() {
        // Real `nm -D --undefined-only` output suffixes symbols with
        // @GLIBC_x.y when the binary requests a specific glibc minimum.
        // We strip that — the affected-functions mapping uses bare
        // function names.
        let raw = "                 U __libc_start_main@GLIBC_2.34\n\
                                    U scanf@GLIBC_2.7\n";
        let symbols = parse_nm_output(raw);
        assert!(symbols.contains("__libc_start_main"));
        assert!(symbols.contains("scanf"));
        assert!(!symbols.iter().any(|s| s.contains('@')));
    }

    #[test]
    fn parse_nm_handles_blank_and_comment_lines() {
        let raw = "\n\
                   # comment line\n\
                   \n\
                                    U malloc\n\
                   \n";
        let symbols = parse_nm_output(raw);
        assert_eq!(symbols.len(), 1);
        assert!(symbols.contains("malloc"));
    }

    #[test]
    fn parse_nm_handles_mixed_whitespace() {
        // Tabs, multiple spaces, leading column spacings — all tolerated.
        let raw = "\t\tU\tabort\n  U  free\n                 U calloc\n";
        let symbols = parse_nm_output(raw);
        assert!(symbols.contains("abort"));
        assert!(symbols.contains("free"));
        assert!(symbols.contains("calloc"));
        assert_eq!(symbols.len(), 3);
    }

    #[test]
    fn parse_nm_skips_non_undefined_lines() {
        // Belt-and-suspenders: even though we'd invoke nm with
        // --undefined-only, a defensive parser ignores any line whose
        // type column isn't 'U'.
        let raw = "0000000000001234 T some_local_function\n\
                   0000000000005678 D some_data\n\
                                    U malloc\n";
        let symbols = parse_nm_output(raw);
        assert_eq!(symbols.len(), 1);
        assert!(symbols.contains("malloc"));
    }

    // ────────────────────────────────────────────────────────────────────
    // load_affected_functions_from_path
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn load_affected_functions_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("aff.json");
        std::fs::write(
            &path,
            json!({
                "CVE-2026-00001": ["foo", "bar"],
                "CVE-2026-00002": ["baz"]
            })
            .to_string(),
        )
        .unwrap();
        let m = load_affected_functions_from_path(&path).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m.get("CVE-2026-00001").unwrap(), &vec!["foo", "bar"]);
        assert_eq!(m.get("CVE-2026-00002").unwrap(), &vec!["baz"]);
    }

    #[test]
    fn load_affected_functions_empty_object_is_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("aff.json");
        std::fs::write(&path, "{}").unwrap();
        let m = load_affected_functions_from_path(&path).unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn load_affected_functions_ignores_non_array_values() {
        // _comment: [...] / _meta: "foo" / unrelated keys are skipped
        // silently — the file is allowed to carry sidecar metadata
        // alongside CVE entries without breaking parsing.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("aff.json");
        std::fs::write(
            &path,
            json!({
                "_comment": ["this file is curated by hand"],
                "_meta": "version 1",
                "CVE-2026-00003": ["fn1"]
            })
            .to_string(),
        )
        .unwrap();
        let m = load_affected_functions_from_path(&path).unwrap();
        assert_eq!(m.len(), 1);
        assert!(m.contains_key("CVE-2026-00003"));
    }

    #[test]
    fn load_affected_functions_ignores_array_with_non_string_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("aff.json");
        std::fs::write(
            &path,
            json!({
                "CVE-2026-00004": ["valid"],
                "_meta_versions": [1, 2, 3]
            })
            .to_string(),
        )
        .unwrap();
        let m = load_affected_functions_from_path(&path).unwrap();
        assert_eq!(m.len(), 1);
        assert!(m.contains_key("CVE-2026-00004"));
    }

    #[test]
    fn load_affected_functions_missing_file_errors() {
        let result = load_affected_functions_from_path(std::path::Path::new(
            "/nonexistent/affected-functions.json",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn load_affected_functions_malformed_json_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("aff.json");
        std::fs::write(&path, "{ invalid").unwrap();
        let result = load_affected_functions_from_path(&path);
        assert!(result.is_err());
    }

    // ────────────────────────────────────────────────────────────────────
    // compute_reachability_vex
    // ────────────────────────────────────────────────────────────────────

    fn aff(pairs: &[(&str, &[&str])]) -> std::collections::BTreeMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    v.iter().map(|s| (*s).to_string()).collect(),
                )
            })
            .collect()
    }

    fn syms(s: &[&str]) -> HashSet<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    fn grype_with(matches: &[&str]) -> GrypeReport {
        GrypeReport {
            matches: matches
                .iter()
                .map(|cve| GrypeMatch {
                    vulnerability: GrypeVuln {
                        id: (*cve).to_string(),
                    },
                    artifact: GrypeArtifact { purl: None },
                })
                .collect(),
        }
    }

    #[test]
    fn empty_grype_yields_empty_statements() {
        let m = aff(&[("CVE-2026-00001", &["foo"])]);
        let s = syms(&["bar"]);
        let out = compute_reachability_vex(
            &grype_with(&[]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        assert!(out.is_empty());
    }

    #[test]
    fn cve_not_in_mapping_is_skipped() {
        // The mapping is the gate: no entry → we don't auto-derive.
        let m = aff(&[]);
        let s = syms(&[]);
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-99998"]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        assert!(out.is_empty());
    }

    #[test]
    fn cve_with_all_affected_fns_absent_is_emitted() {
        let m = aff(&[("CVE-2026-00010", &["glob", "fnmatch"])]);
        let s = syms(&["malloc", "free"]);
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-00010"]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].vulnerability.name, "CVE-2026-00010");
        assert_eq!(out[0].status, "not_affected");
        assert_eq!(
            out[0].justification.as_deref(),
            Some("vulnerable_code_not_in_execute_path")
        );
        assert_eq!(out[0].products[0].id, "pkg:oci/5-spot");
        // The impact statement should be machine-derivable: which fns
        // were checked and confirmed absent.
        let impact = out[0].impact_statement.as_deref().unwrap_or_default();
        assert!(impact.contains("glob"));
        assert!(impact.contains("fnmatch"));
    }

    #[test]
    fn cve_with_any_affected_fn_imported_is_not_emitted() {
        let m = aff(&[("CVE-2026-00011", &["glob", "fnmatch"])]);
        let s = syms(&["fnmatch", "malloc"]);
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-00011"]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        // fnmatch IS imported → reachable (via that entry point) → we
        // don't claim not_affected. Stays for human triage.
        assert!(out.is_empty());
    }

    #[test]
    fn cve_already_triaged_is_skipped() {
        let m = aff(&[("CVE-2026-00012", &["nis_local_principal"])]);
        let s = syms(&[]);
        let mut triaged = HashSet::new();
        triaged.insert("CVE-2026-00012".to_string());
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-00012"]),
            &s,
            &m,
            &triaged,
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        assert!(out.is_empty());
    }

    #[test]
    fn duplicate_cve_emits_only_once() {
        let m = aff(&[("CVE-2026-00013", &["scanf"])]);
        let s = syms(&[]);
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-00013", "CVE-2026-00013"]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn output_is_sorted_by_cve_id() {
        let m = aff(&[
            ("CVE-2026-00021", &["fn_a"]),
            ("CVE-2026-00019", &["fn_b"]),
            ("CVE-2026-00020", &["fn_c"]),
        ]);
        let s = syms(&[]);
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-00021", "CVE-2026-00019", "CVE-2026-00020"]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        let names: Vec<&str> = out.iter().map(|s| s.vulnerability.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["CVE-2026-00019", "CVE-2026-00020", "CVE-2026-00021"]
        );
    }

    #[test]
    fn empty_affected_functions_array_is_treated_as_no_evidence() {
        // If the mapping has CVE-X with an empty function list, that
        // means "we don't know which functions are affected" — we must
        // NOT emit not_affected, because we have no evidence to back it.
        let m = aff(&[("CVE-2026-00022", &[])]);
        let s = syms(&[]);
        let out = compute_reachability_vex(
            &grype_with(&["CVE-2026-00022"]),
            &s,
            &m,
            &HashSet::new(),
            "pkg:oci/5-spot",
            "2026-05-02T00:00:00Z",
        );
        assert!(out.is_empty());
    }
}
