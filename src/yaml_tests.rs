// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the yaml module
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct OneField {
        description: String,
    }

    #[derive(Serialize)]
    struct Nested {
        outer: Inner,
    }

    #[derive(Serialize)]
    struct Inner {
        text: String,
        number: u32,
    }

    /// A long description is exactly what a CRD doc comment produces, and it is
    /// the case `folded_wrap_chars` would otherwise rewrite.
    fn long_text() -> String {
        "a ".repeat(120).trim_end().to_string()
    }

    // ========================================================================
    // No trailing whitespace anywhere
    // ========================================================================

    #[test]
    fn test_no_line_ends_in_whitespace() {
        let value = OneField {
            description: "first line\n\nthird line after a blank one\n".to_string(),
        };
        let yaml = to_string(&value).expect("serializes");
        for line in yaml.lines() {
            assert_eq!(
                line,
                line.trim_end(),
                "trailing whitespace in {line:?} of:\n{yaml}"
            );
        }
    }

    #[test]
    fn test_blank_lines_are_fully_empty_not_indented() {
        // The emitter indents blank lines inside a block scalar; the wrapper
        // strips that so the file has no whitespace-only lines.
        let value = Nested {
            outer: Inner {
                text: "one\n\ntwo\n".to_string(),
                number: 7,
            },
        };
        let yaml = to_string(&value).expect("serializes");
        for line in yaml.lines() {
            assert!(
                !(line.trim().is_empty() && !line.is_empty()),
                "whitespace-only line {line:?} in:\n{yaml}"
            );
        }
    }

    // ========================================================================
    // Long strings stay on one line (no `>-` folding)
    // ========================================================================

    #[test]
    fn test_a_long_single_line_string_stays_on_one_line() {
        let value = OneField {
            description: long_text(),
        };
        let yaml = to_string(&value).expect("serializes");
        assert!(
            !yaml.contains(">-") && !yaml.contains(">\n"),
            "folded instead of kept on one line:\n{yaml}"
        );
        assert_eq!(yaml.lines().count(), 1, "wrapped across lines:\n{yaml}");
    }

    #[test]
    fn test_long_string_survives_a_round_trip_unchanged() {
        // Folding is lossy for leading/trailing spaces and re-wraps newlines as
        // spaces, so this is the property that matters, not the exact bytes.
        let original = long_text();
        let yaml = to_string(&OneField {
            description: original.clone(),
        })
        .expect("serializes");
        let back: serde_json::Value = serde_saphyr::from_str(&yaml).expect("parses back");
        assert_eq!(
            back["description"].as_str().expect("a string"),
            original.as_str()
        );
    }

    // ========================================================================
    // Round-trip fidelity
    // ========================================================================

    #[test]
    fn test_output_parses_back_to_the_same_value() {
        let yaml = to_string(&Nested {
            outer: Inner {
                text: "line one\n\nline three\n".to_string(),
                number: 42,
            },
        })
        .expect("serializes");
        let back: serde_json::Value = serde_saphyr::from_str(&yaml).expect("parses back");
        assert_eq!(back["outer"]["number"].as_u64(), Some(42));
        assert_eq!(
            back["outer"]["text"].as_str(),
            Some("line one\n\nline three\n")
        );
    }

    #[test]
    fn test_stripping_blank_line_indentation_preserves_line_count() {
        // The strip must not remove the newline itself, or a blank line would
        // vanish and the parsed scalar would change.
        let raw = "a:\n  |\n    one\n    \n    two\n";
        let stripped = strip_blank_line_indentation(raw);
        assert_eq!(raw.lines().count(), stripped.lines().count());
        assert!(stripped.contains("\n\n"), "blank line lost: {stripped:?}");
    }

    #[test]
    fn test_empty_input_is_unchanged_by_the_strip() {
        assert_eq!(strip_blank_line_indentation(""), "");
    }

    #[test]
    fn test_strip_leaves_a_final_line_without_newline_intact() {
        // split_inclusive yields a trailing fragment with no '\n'; it must not
        // be dropped.
        assert_eq!(strip_blank_line_indentation("a: 1\nb: 2"), "a: 1\nb: 2");
    }

    #[test]
    fn test_strip_removes_a_whitespace_only_final_line() {
        assert_eq!(strip_blank_line_indentation("a: 1\n   "), "a: 1\n");
    }
}
