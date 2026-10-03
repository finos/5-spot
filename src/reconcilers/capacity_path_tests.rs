// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::{CAPACITY_INACTIVE_VALUE, CAPACITY_PATH_MAX_SEGMENTS};
    use serde_json::json;

    // ========================================================================
    // validate_write_path - the happy path
    // ========================================================================

    #[test]
    fn test_write_path_accepts_the_canonical_consumer_field() {
        let segments = validate_write_path("spec.warmReplicas").expect("valid");
        assert_eq!(segments, vec!["spec", "warmReplicas"]);
    }

    #[test]
    fn test_write_path_accepts_deep_nesting_up_to_the_cap() {
        let path = "spec.a.b.c.d.e.f.g";
        let segments = validate_write_path(path).expect("8 segments is the cap");
        assert_eq!(segments.len(), CAPACITY_PATH_MAX_SEGMENTS);
    }

    #[test]
    fn test_write_path_accepts_digits_inside_a_segment() {
        assert!(validate_write_path("spec.pool2Replicas").is_ok());
    }

    // ========================================================================
    // validate_write_path - the rejections that make this a control
    // ========================================================================

    /// The single most important rejection: `metadata.` would let a CR author
    /// write `ownerReferences`, `finalizers` or labels on an object 5-Spot does
    /// not own, which is an elevation primitive, not a capacity knob.
    #[test]
    fn test_write_path_rejects_metadata_prefix() {
        for path in [
            "metadata.ownerReferences",
            "metadata.finalizers",
            "metadata.labels",
            "metadata.annotations",
            "metadata.name",
        ] {
            let error = validate_write_path(path).expect_err(path);
            assert!(
                matches!(error, CapacityPathError::WrongPrefix { .. }),
                "{path} must be rejected as a wrong prefix, got {error:?}"
            );
        }
    }

    /// A status is a controller's own report, not a knob.
    #[test]
    fn test_write_path_rejects_status_prefix() {
        let error = validate_write_path("status.claimed").expect_err("status is not writable");
        assert!(matches!(error, CapacityPathError::WrongPrefix { .. }));
    }

    /// `spec` alone has no field to write, and a bare prefix match must not be
    /// mistaken for one.
    #[test]
    fn test_write_path_rejects_bare_prefix_and_trailing_dot() {
        for path in ["spec", "spec.", "spec..", ".spec.x"] {
            assert!(
                validate_write_path(path).is_err(),
                "{path} must be rejected"
            );
        }
    }

    #[test]
    fn test_write_path_rejects_empty() {
        assert!(matches!(
            validate_write_path("").expect_err("empty"),
            CapacityPathError::Empty
        ));
    }

    /// No JSON Pointer escape, no array index, no traversal, no quoting. These
    /// are the shapes that would turn a field name into an expression.
    #[test]
    fn test_write_path_rejects_pointer_index_and_traversal_syntax() {
        for path in [
            "spec.items[0].count",
            "spec.items[*]",
            "spec/warmReplicas",
            "spec.../etc/passwd",
            "spec..warmReplicas",
            "spec.\"quoted\"",
            "spec.'quoted'",
            "spec.*",
            "spec.a b",
            "spec.a-b",
            "spec.a_b",
            "spec.$ref",
            "spec.~0",
            "spec.a\nb",
            "spec.a\tb",
        ] {
            assert!(
                validate_write_path(path).is_err(),
                "{path:?} must be rejected"
            );
        }
    }

    /// Kubernetes field names are camelCase; an uppercase first letter would be
    /// a type name, not a field, and admitting it widens the charset for no
    /// gain.
    #[test]
    fn test_write_path_rejects_segment_not_starting_lowercase() {
        for path in ["spec.WarmReplicas", "spec.2replicas", "Spec.warmReplicas"] {
            assert!(
                validate_write_path(path).is_err(),
                "{path} must be rejected"
            );
        }
    }

    #[test]
    fn test_write_path_rejects_more_segments_than_the_cap() {
        let path = "spec.a.b.c.d.e.f.g.h"; // 9
        let error = validate_write_path(path).expect_err("over the cap");
        assert!(
            matches!(
                error,
                CapacityPathError::TooManySegments { found: 9, max: 8 }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn test_write_path_rejects_over_length() {
        let long_segment = "a".repeat(300);
        let path = format!("spec.{long_segment}");
        assert!(matches!(
            validate_write_path(&path).expect_err("too long"),
            CapacityPathError::TooLong { .. }
        ));
    }

    // ========================================================================
    // validate_drained_path
    // ========================================================================

    #[test]
    fn test_drained_path_accepts_the_canonical_consumer_counter() {
        let segments = validate_drained_path("status.claimed").expect("valid");
        assert_eq!(segments, vec!["status", "claimed"]);
    }

    /// The drain path reads a consumer's own report, so `spec.` is the wrong
    /// half of the object: a spec value is what 5-Spot just wrote, and reading
    /// it back would make handback trivially and wrongly "complete".
    #[test]
    fn test_drained_path_rejects_spec_prefix() {
        let error = validate_drained_path("spec.warmReplicas").expect_err("spec is not a report");
        assert!(matches!(error, CapacityPathError::WrongPrefix { .. }));
    }

    #[test]
    fn test_drained_path_rejects_metadata_prefix() {
        assert!(validate_drained_path("metadata.labels").is_err());
    }

    #[test]
    fn test_drained_path_applies_the_same_charset_rules() {
        for path in ["status.items[0]", "status./x", "status.", "status"] {
            assert!(
                validate_drained_path(path).is_err(),
                "{path} must be rejected"
            );
        }
    }

    // ========================================================================
    // build_merge_patch
    // ========================================================================

    #[test]
    fn test_build_merge_patch_nests_one_level() {
        let patch = build_merge_patch("spec.warmReplicas", 10).expect("valid");
        assert_eq!(patch, json!({ "spec": { "warmReplicas": 10 } }));
    }

    #[test]
    fn test_build_merge_patch_nests_deeply() {
        let patch = build_merge_patch("spec.budget.cpu.cores", 16).expect("valid");
        assert_eq!(
            patch,
            json!({ "spec": { "budget": { "cpu": { "cores": 16 } } } })
        );
    }

    /// Zero is the inactive value and must serialise as a real `0`, not be
    /// elided: handback depends on the target actually receiving it.
    #[test]
    fn test_build_merge_patch_writes_an_explicit_zero() {
        let patch = build_merge_patch("spec.warmReplicas", CAPACITY_INACTIVE_VALUE).expect("valid");
        assert_eq!(patch, json!({ "spec": { "warmReplicas": 0 } }));
        assert_eq!(
            patch.pointer("/spec/warmReplicas").and_then(|v| v.as_i64()),
            Some(0)
        );
    }

    /// A merge patch touches exactly the named field. Anything else in the
    /// patch body would be 5-Spot writing a field nobody asked it to.
    #[test]
    fn test_build_merge_patch_contains_only_the_named_field() {
        let patch = build_merge_patch("spec.warmReplicas", 4).expect("valid");
        let top = patch.as_object().expect("object");
        assert_eq!(top.len(), 1, "only one top-level key");
        assert!(top.contains_key("spec"));
        let spec = top["spec"].as_object().expect("object");
        assert_eq!(spec.len(), 1, "only the one named field");
        assert!(
            !spec.contains_key("maxReplicas"),
            "must never touch a sibling field"
        );
    }

    /// An invalid path must fail before any patch is constructed: the validator
    /// is the gate, and build_merge_patch must not be a second way in.
    #[test]
    fn test_build_merge_patch_refuses_an_invalid_path() {
        for path in ["metadata.labels", "spec.items[0]", "", "status.claimed"] {
            assert!(
                build_merge_patch(path, 1).is_err(),
                "{path} must not produce a patch"
            );
        }
    }

    // ========================================================================
    // read_i64_at
    // ========================================================================

    #[test]
    fn test_read_i64_at_reads_a_nested_counter() {
        let object = json!({ "spec": { "warmReplicas": 10 }, "status": { "claimed": 3 } });
        assert_eq!(read_i64_at(&object, "status.claimed"), Some(3));
        assert_eq!(read_i64_at(&object, "spec.warmReplicas"), Some(10));
    }

    #[test]
    fn test_read_i64_at_reads_zero_as_zero_not_absent() {
        // The whole handback decision turns on telling 0 from "not reported".
        let object = json!({ "status": { "claimed": 0 } });
        assert_eq!(read_i64_at(&object, "status.claimed"), Some(0));
    }

    #[test]
    fn test_read_i64_at_none_when_absent_or_wrong_type() {
        let object = json!({ "status": { "claimed": "three", "phase": "Ready" } });
        assert_eq!(read_i64_at(&object, "status.claimed"), None);
        assert_eq!(read_i64_at(&object, "status.phase"), None);
        assert_eq!(read_i64_at(&object, "status.missing"), None);
        assert_eq!(read_i64_at(&object, "nothing.here"), None);
    }

    #[test]
    fn test_read_i64_at_none_on_a_non_object_intermediate() {
        let object = json!({ "status": 5 });
        assert_eq!(read_i64_at(&object, "status.claimed"), None);
    }

    #[test]
    fn test_read_i64_at_handles_an_unsigned_counter() {
        // Consumer counters are often uint32 on the wire (VirtualMachinePool's
        // status.claimed is), which serde reports as u64.
        let object = json!({ "status": { "claimed": u64::from(u32::MAX) } });
        assert_eq!(read_i64_at(&object, "status.claimed"), Some(4_294_967_295));
    }

    #[test]
    fn test_read_i64_at_empty_path_is_none() {
        let object = json!({ "status": { "claimed": 1 } });
        assert_eq!(read_i64_at(&object, ""), None);
    }

    // ========================================================================
    // Round-trip: what we write is what we read back
    // ========================================================================

    /// The write and read halves must agree on path semantics, or the
    /// controller would write one field and verify another.
    #[test]
    fn test_patch_then_read_round_trips() {
        for (path, value) in [
            ("spec.warmReplicas", 10_i64),
            ("spec.budget.cpu.cores", 16),
            ("spec.warmReplicas", CAPACITY_INACTIVE_VALUE),
        ] {
            let patch = build_merge_patch(path, value).expect("valid path");
            assert_eq!(
                read_i64_at(&patch, path),
                Some(value),
                "{path} must read back the value it was built with"
            );
        }
    }
}
