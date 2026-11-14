// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
// Tests for the crd_schema module
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::crd::ScheduledMachine;
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
        CustomResourceDefinition, JSONSchemaProps, JSONSchemaPropsOrArray, JSONSchemaPropsOrBool,
        JSONSchemaPropsOrStringArray,
    };
    use kube::CustomResourceExt;
    use std::collections::BTreeMap;

    fn typed(type_: &str, format: &str) -> JSONSchemaProps {
        JSONSchemaProps {
            type_: Some(type_.into()),
            format: Some(format.into()),
            ..JSONSchemaProps::default()
        }
    }

    fn format_after(type_: &str, format: &str) -> Option<String> {
        let mut schema = typed(type_, format);
        strip_unsupported_schema_formats(&mut schema);
        schema.format
    }

    /// Every format a schema declares, with its type, collected recursively.
    fn formats(schema: &JSONSchemaProps, out: &mut Vec<(String, String)>) {
        if let Some(f) = &schema.format {
            out.push((schema.type_.clone().unwrap_or_default(), f.clone()));
        }
        let children = schema
            .properties
            .iter()
            .flat_map(|m| m.values())
            .chain(schema.items.iter().flat_map(|i| match i {
                JSONSchemaPropsOrArray::Schema(s) => std::slice::from_ref(s.as_ref()),
                JSONSchemaPropsOrArray::Schemas(v) => v.as_slice(),
            }))
            .chain(schema.additional_properties.iter().filter_map(|a| match a {
                JSONSchemaPropsOrBool::Schema(s) => Some(s.as_ref()),
                JSONSchemaPropsOrBool::Bool(_) => None,
            }))
            .chain(schema.any_of.iter().flatten())
            .chain(schema.all_of.iter().flatten())
            .chain(schema.one_of.iter().flatten());
        for child in children {
            formats(child, out);
        }
    }

    /// The unsupported formats in every version of `crd`, as `Kind: type/format`.
    fn unsupported(crd: &CustomResourceDefinition) -> Vec<String> {
        let mut seen = Vec::new();
        for version in &crd.spec.versions {
            let root = version
                .schema
                .as_ref()
                .and_then(|s| s.open_api_v3_schema.as_ref())
                .expect("schema");
            let mut found = Vec::new();
            formats(root, &mut found);
            for (type_, format) in found {
                let ok = match type_.as_str() {
                    "integer" => ["int32", "int64"].contains(&format.as_str()),
                    "number" => ["float", "double"].contains(&format.as_str()),
                    "string" | "" => is_supported_string_format(&format),
                    _ => false,
                };
                if !ok {
                    seen.push(format!("{}: {type_}/{format}", crd.spec.names.kind));
                }
            }
        }
        seen
    }

    // ---- strip_unsupported_schema_formats ----

    /// Integers keep only `int32` and `int64`: the apiserver drops schemars'
    /// unsigned formats, and warns about them on apply.
    #[test]
    fn integer_formats_keep_only_int32_and_int64() {
        for kept in ["int32", "int64"] {
            assert_eq!(format_after("integer", kept).as_deref(), Some(kept));
        }
        for dropped in ["uint8", "uint16", "uint32", "uint64", "uint", "float"] {
            assert_eq!(format_after("integer", dropped), None, "{dropped}");
        }
    }

    #[test]
    fn number_formats_keep_only_float_and_double() {
        for kept in ["float", "double"] {
            assert_eq!(format_after("number", kept).as_deref(), Some(kept));
        }
        assert_eq!(format_after("number", "int64"), None);
    }

    /// Strings keep the formats Kubernetes knows, spelled with or without
    /// dashes, and lose the rest.
    #[test]
    fn string_formats_keep_the_kubernetes_set() {
        for kept in [
            "date-time",
            "datetime",
            "uuid",
            "byte",
            "ipv4",
            "k8s-short-name",
        ] {
            assert_eq!(format_after("string", kept).as_deref(), Some(kept));
        }
        for dropped in ["uint32", "json", "int64"] {
            assert_eq!(format_after("string", dropped), None, "{dropped}");
        }
    }

    #[test]
    fn other_types_lose_any_format() {
        assert_eq!(format_after("boolean", "int32"), None);
        assert_eq!(format_after("object", "date-time"), None);
    }

    /// An untyped schema is judged as a string, as the apiserver does.
    #[test]
    fn untyped_schemas_keep_string_formats_only() {
        let mut kept = JSONSchemaProps {
            format: Some("date-time".into()),
            ..JSONSchemaProps::default()
        };
        strip_unsupported_schema_formats(&mut kept);
        assert_eq!(kept.format.as_deref(), Some("date-time"));

        let mut dropped = JSONSchemaProps {
            format: Some("uint64".into()),
            ..JSONSchemaProps::default()
        };
        strip_unsupported_schema_formats(&mut dropped);
        assert_eq!(dropped.format, None);
    }

    /// A schema with no format is left alone.
    #[test]
    fn a_schema_without_a_format_is_unchanged() {
        let mut schema = JSONSchemaProps {
            type_: Some("integer".into()),
            minimum: Some(0.0),
            ..JSONSchemaProps::default()
        };
        let before = schema.clone();
        strip_unsupported_schema_formats(&mut schema);
        assert_eq!(schema, before);
    }

    /// Nested schemas are cleaned too, and a *property* named `format` is a
    /// field, not the keyword: it stays.
    #[test]
    fn nested_schemas_are_cleaned_and_a_property_named_format_stays() {
        let mut root = JSONSchemaProps {
            type_: Some("object".into()),
            properties: Some(BTreeMap::from([
                ("format".to_string(), typed("string", "date-time")),
                (
                    "list".to_string(),
                    JSONSchemaProps {
                        type_: Some("array".into()),
                        items: Some(JSONSchemaPropsOrArray::Schema(Box::new(typed(
                            "integer", "uint64",
                        )))),
                        ..JSONSchemaProps::default()
                    },
                ),
                (
                    "tuple".to_string(),
                    JSONSchemaProps {
                        type_: Some("array".into()),
                        items: Some(JSONSchemaPropsOrArray::Schemas(vec![typed(
                            "integer", "uint16",
                        )])),
                        additional_items: Some(JSONSchemaPropsOrBool::Schema(Box::new(typed(
                            "integer", "uint8",
                        )))),
                        ..JSONSchemaProps::default()
                    },
                ),
                (
                    "map".to_string(),
                    JSONSchemaProps {
                        type_: Some("object".into()),
                        additional_properties: Some(JSONSchemaPropsOrBool::Schema(Box::new(
                            typed("integer", "uint32"),
                        ))),
                        pattern_properties: Some(BTreeMap::from([(
                            "^x".to_string(),
                            typed("integer", "uint32"),
                        )])),
                        ..JSONSchemaProps::default()
                    },
                ),
                (
                    "either".to_string(),
                    JSONSchemaProps {
                        any_of: Some(vec![typed("integer", "uint8"), typed("string", "uuid")]),
                        all_of: Some(vec![typed("integer", "uint8")]),
                        one_of: Some(vec![typed("integer", "uint8")]),
                        not: Some(Box::new(typed("integer", "uint8"))),
                        ..JSONSchemaProps::default()
                    },
                ),
                (
                    "deps".to_string(),
                    JSONSchemaProps {
                        dependencies: Some(BTreeMap::from([(
                            "a".to_string(),
                            JSONSchemaPropsOrStringArray::Schema(Box::new(typed(
                                "integer", "uint64",
                            ))),
                        )])),
                        definitions: Some(BTreeMap::from([(
                            "d".to_string(),
                            typed("integer", "uint64"),
                        )])),
                        ..JSONSchemaProps::default()
                    },
                ),
            ])),
            ..JSONSchemaProps::default()
        };
        strip_unsupported_schema_formats(&mut root);
        let props = root.properties.as_ref().unwrap();
        assert!(props.contains_key("format"), "a field named format stays");
        assert_eq!(props["format"].format.as_deref(), Some("date-time"));

        let Some(JSONSchemaPropsOrArray::Schema(item)) = &props["list"].items else {
            panic!("items");
        };
        assert_eq!(item.format, None);

        let Some(JSONSchemaPropsOrArray::Schemas(items)) = &props["tuple"].items else {
            panic!("tuple items");
        };
        assert_eq!(items[0].format, None);
        let Some(JSONSchemaPropsOrBool::Schema(extra)) = &props["tuple"].additional_items else {
            panic!("additionalItems");
        };
        assert_eq!(extra.format, None);

        let Some(JSONSchemaPropsOrBool::Schema(value)) = &props["map"].additional_properties else {
            panic!("additionalProperties");
        };
        assert_eq!(value.format, None);
        assert_eq!(
            props["map"].pattern_properties.as_ref().unwrap()["^x"].format,
            None
        );

        let either = &props["either"];
        let any = either.any_of.as_ref().unwrap();
        assert_eq!(any[0].format, None);
        assert_eq!(any[1].format.as_deref(), Some("uuid"));
        assert_eq!(either.all_of.as_ref().unwrap()[0].format, None);
        assert_eq!(either.one_of.as_ref().unwrap()[0].format, None);
        assert_eq!(either.not.as_ref().unwrap().format, None);

        let deps = &props["deps"];
        let JSONSchemaPropsOrStringArray::Schema(dep) = &deps.dependencies.as_ref().unwrap()["a"]
        else {
            panic!("dependencies");
        };
        assert_eq!(dep.format, None);
        assert_eq!(deps.definitions.as_ref().unwrap()["d"].format, None);
    }

    // ---- is_supported_string_format ----

    #[test]
    fn string_format_check_ignores_dashes() {
        assert!(is_supported_string_format("date-time"));
        assert!(is_supported_string_format("datetime"));
        assert!(is_supported_string_format("k8s-long-name"));
    }

    #[test]
    fn string_format_check_rejects_unknown_names() {
        assert!(!is_supported_string_format("uint64"));
        assert!(!is_supported_string_format(""));
        assert!(!is_supported_string_format("Date-Time"));
    }

    // ---- strip_unsupported_formats / prepared ----

    /// `ScheduledMachine` is the CRD that carried `uint8` before this fix: the
    /// raw output still does, which is what makes the guard below meaningful.
    #[test]
    fn the_raw_crd_still_carries_an_unsupported_format() {
        assert!(!unsupported(&ScheduledMachine::crd()).is_empty());
    }

    #[test]
    fn strip_unsupported_formats_cleans_every_version() {
        let mut crd = ScheduledMachine::crd();
        strip_unsupported_formats(&mut crd);
        assert_eq!(unsupported(&crd), Vec::<String>::new());
    }

    /// A CRD with no schema is left alone rather than panicking.
    #[test]
    fn strip_unsupported_formats_tolerates_a_version_without_a_schema() {
        let mut crd = ScheduledMachine::crd();
        for version in &mut crd.spec.versions {
            version.schema = None;
        }
        let before = crd.clone();
        strip_unsupported_formats(&mut crd);
        assert_eq!(crd, before);
    }

    /// `prepared` keeps the supported formats (`int64`) it is given.
    #[test]
    fn prepared_keeps_supported_formats() {
        let yaml = crate::yaml::to_string(&prepared(ScheduledMachine::crd())).expect("yaml");
        assert!(yaml.contains("format: int64"));
        assert!(!yaml.contains("format: uint"));
    }

    // ---- all_crds ----

    #[test]
    fn all_crds_lists_the_four_kinds_in_order() {
        let kinds: Vec<String> = all_crds().into_iter().map(|c| c.spec.names.kind).collect();
        assert_eq!(
            kinds,
            [
                "ScheduledMachine",
                "ScheduledCapacity",
                "TimeBasedSpotSchedule",
                "CapitalMarketsSchedule",
            ]
        );
    }

    /// The regression guard: no CRD 5-Spot ships carries a format the
    /// apiserver would drop and warn about (`unrecognized format "uint8"`).
    #[test]
    fn no_shipped_crd_carries_an_unsupported_format() {
        let seen: Vec<String> = all_crds().iter().flat_map(unsupported).collect();
        assert!(seen.is_empty(), "unsupported formats: {seen:?}");
    }
}
