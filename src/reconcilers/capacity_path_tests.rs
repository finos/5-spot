// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::{
        CAPACITY_FIELD_MAX_SEGMENTS, CAPACITY_INACTIVE_VALUE, CAPACITY_PATH_MAX_SEGMENTS,
    };
    use serde_json::{json, Value};

    // ========================================================================
    // validate_capacity_field - the happy path
    // ========================================================================

    #[test]
    fn test_capacity_field_accepts_the_canonical_consumer_knob() {
        let segments = validate_capacity_field("warmReplicas").expect("valid");
        assert_eq!(segments, vec!["warmReplicas"]);
    }

    #[test]
    fn test_capacity_field_accepts_a_nested_knob_up_to_the_cap() {
        let path = "a.b.c.d.e.f.g";
        let segments = validate_capacity_field(path).expect("7 segments is the cap");
        assert_eq!(segments.len(), CAPACITY_FIELD_MAX_SEGMENTS);
    }

    #[test]
    fn test_capacity_field_accepts_digits_inside_a_segment() {
        assert!(validate_capacity_field("pool2Replicas").is_ok());
    }

    // ========================================================================
    // validate_capacity_field - the rejections
    // ========================================================================

    /// ADR 0016 decision 3: the field is relative to the owned object's `spec`,
    /// so a `spec.` prefix is a caller mistake that would produce
    /// `spec.spec.warmReplicas`. It is rejected as an ordinary charset-valid
    /// path with a reserved first segment, not as a security control: there is
    /// nothing dangerous left for it to reach.
    #[test]
    fn test_capacity_field_rejects_a_root_segment_that_is_already_implicit() {
        for path in ["spec.warmReplicas", "metadata.labels", "status.claimed"] {
            let error = validate_capacity_field(path).expect_err(path);
            assert!(
                matches!(error, CapacityPathError::ReservedRootSegment { .. }),
                "{path} must be rejected as a reserved root segment, got {error:?}"
            );
        }
    }

    #[test]
    fn test_capacity_field_rejects_empty_and_malformed_dots() {
        for path in ["", "warmReplicas.", ".warmReplicas", "a..b"] {
            assert!(
                validate_capacity_field(path).is_err(),
                "{path:?} must be rejected"
            );
        }
    }

    /// No JSON Pointer escape, no array index, no traversal, no quoting. These
    /// cannot reach anything dangerous any more, but they would still construct
    /// a nonsense field name on an object 5-Spot creates, and an allowlist is
    /// cheaper to reason about than an argument about which are harmless.
    #[test]
    fn test_capacity_field_rejects_pointer_index_and_traversal_syntax() {
        for path in [
            "items[0].count",
            "items[*]",
            "warm/replicas",
            "../etc/passwd",
            "\"quoted\"",
            "'quoted'",
            "*",
            "a b",
            "a-b",
            "a_b",
            "$ref",
            "~0",
            "a\nb",
            "a\tb",
        ] {
            assert!(
                validate_capacity_field(path).is_err(),
                "{path:?} must be rejected"
            );
        }
    }

    #[test]
    fn test_capacity_field_rejects_segment_not_starting_lowercase() {
        for path in ["WarmReplicas", "2replicas", "a.B"] {
            assert!(
                validate_capacity_field(path).is_err(),
                "{path} must be rejected"
            );
        }
    }

    #[test]
    fn test_capacity_field_rejects_more_segments_than_the_cap() {
        let path = "a.b.c.d.e.f.g.h"; // 8, one over
        let error = validate_capacity_field(path).expect_err("over the cap");
        assert!(
            matches!(
                error,
                CapacityPathError::TooManySegments { found: 8, max: 7 }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn test_capacity_field_rejects_over_length() {
        let long_segment = "a".repeat(300);
        assert!(matches!(
            validate_capacity_field(&long_segment).expect_err("too long"),
            CapacityPathError::TooLong { .. }
        ));
    }

    /// The field cap is one below the path cap because `spec` occupies the
    /// first position in the object actually constructed. If the two ever drift
    /// the controller would build an object deeper than the schema admits.
    #[test]
    fn test_capacity_field_cap_leaves_room_for_the_implicit_spec_root() {
        assert_eq!(CAPACITY_FIELD_MAX_SEGMENTS + 1, CAPACITY_PATH_MAX_SEGMENTS);
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
    // inject_capacity_field
    // ========================================================================

    #[test]
    fn test_inject_sets_a_top_level_knob_and_preserves_the_template() {
        let template = json!({ "maxReplicas": 20, "readiness": "GuestReady" });
        let spec = inject_capacity_field(&template, "warmReplicas", 10).expect("valid");
        assert_eq!(
            spec,
            json!({ "maxReplicas": 20, "readiness": "GuestReady", "warmReplicas": 10 })
        );
    }

    #[test]
    fn test_inject_creates_missing_intermediate_objects() {
        let template = json!({ "maxReplicas": 20 });
        let spec = inject_capacity_field(&template, "scale.warm", 4).expect("valid");
        assert_eq!(spec, json!({ "maxReplicas": 20, "scale": { "warm": 4 } }));
    }

    /// Injecting must not flatten a nested sibling that already exists.
    #[test]
    fn test_inject_merges_into_an_existing_intermediate() {
        let template = json!({ "scale": { "max": 20, "policy": "fast" } });
        let spec = inject_capacity_field(&template, "scale.warm", 4).expect("valid");
        assert_eq!(
            spec,
            json!({ "scale": { "max": 20, "policy": "fast", "warm": 4 } })
        );
    }

    /// Zero is the inactive value. It must land as a real `0` rather than be
    /// elided, because window close depends on the owned object receiving it.
    #[test]
    fn test_inject_writes_an_explicit_zero() {
        let spec =
            inject_capacity_field(&json!({}), "warmReplicas", CAPACITY_INACTIVE_VALUE).expect("ok");
        assert_eq!(
            spec.pointer("/warmReplicas").and_then(Value::as_i64),
            Some(0)
        );
    }

    /// One source of truth (ADR 0016 decision 3). A template that sets the knob
    /// itself would fight the schedule on every reconcile, and the loser would
    /// be whichever wrote last.
    #[test]
    fn test_inject_refuses_a_template_that_already_sets_the_knob() {
        let template = json!({ "warmReplicas": 99, "maxReplicas": 20 });
        let error = inject_capacity_field(&template, "warmReplicas", 10)
            .expect_err("the template must not set the knob");
        assert!(
            matches!(error, CapacityPathError::TemplateSetsCapacityField { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn test_inject_refuses_a_template_that_sets_a_nested_knob() {
        let template = json!({ "scale": { "warm": 99 } });
        assert!(matches!(
            inject_capacity_field(&template, "scale.warm", 10).expect_err("nested collision"),
            CapacityPathError::TemplateSetsCapacityField { .. }
        ));
    }

    /// A non-object template is not a spec, and silently replacing it would
    /// discard the operator's input.
    #[test]
    fn test_inject_refuses_a_non_object_template_or_intermediate() {
        assert!(inject_capacity_field(&json!([1, 2]), "warmReplicas", 1).is_err());
        assert!(inject_capacity_field(&json!("nope"), "warmReplicas", 1).is_err());
        // `scale` is a scalar, so `scale.warm` cannot be created under it.
        assert!(inject_capacity_field(&json!({ "scale": 5 }), "scale.warm", 1).is_err());
    }

    /// An invalid field must fail before any object is constructed: the
    /// validator is the gate and this must not be a second way in.
    #[test]
    fn test_inject_refuses_an_invalid_field() {
        for field in ["spec.warmReplicas", "items[0]", "", "a-b"] {
            assert!(
                inject_capacity_field(&json!({}), field, 1).is_err(),
                "{field} must not produce a spec"
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

    /// The inject and read halves must agree on path semantics, or the
    /// controller would write one field and verify another.
    #[test]
    fn test_inject_then_read_round_trips() {
        for (field, value) in [
            ("warmReplicas", 10_i64),
            ("budget.cpu.cores", 16),
            ("warmReplicas", CAPACITY_INACTIVE_VALUE),
        ] {
            let spec = inject_capacity_field(&json!({}), field, value).expect("valid field");
            assert_eq!(
                read_i64_at(&spec, field),
                Some(value),
                "{field} must read back the value it was built with"
            );
        }
    }
}
