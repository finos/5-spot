// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! Post-generation fix-ups for the CRDs 5-Spot ships (`deploy/crds/`).
//!
//! `crdgen` renders every CRD through [`prepared`], so the YAML in
//! `deploy/crds/` is what the apiserver keeps, without warnings.

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
    CustomResourceDefinition, JSONSchemaProps, JSONSchemaPropsOrArray, JSONSchemaPropsOrBool,
    JSONSchemaPropsOrStringArray,
};

/// Integer formats the apiserver keeps (`type: integer`).
const INTEGER_FORMATS: [&str; 2] = ["int32", "int64"];

/// Number formats the apiserver keeps (`type: number`).
const NUMBER_FORMATS: [&str; 2] = ["float", "double"];

/// String formats the apiserver keeps, without dashes: it compares formats
/// with their dashes removed, so `date-time` matches `datetime`. Mirrors
/// `supportedVersionedFormats` in apiextensions-apiserver's
/// `pkg/apiserver/validation/formats.go` (the 1.0 set plus the 1.34 names).
const STRING_FORMATS: [&str; 26] = [
    "bsonobjectid",
    "uri",
    "email",
    "hostname",
    "ipv4",
    "ipv6",
    "cidr",
    "mac",
    "uuid",
    "uuid3",
    "uuid4",
    "uuid5",
    "isbn",
    "isbn10",
    "isbn13",
    "creditcard",
    "ssn",
    "hexcolor",
    "rgbcolor",
    "byte",
    "password",
    "date",
    "duration",
    "datetime",
    "k8sshortname",
    "k8slongname",
];

/// Every CRD 5-Spot ships, post-processed by [`prepared`], in `crdgen`'s
/// emit order.
#[must_use]
pub fn all_crds() -> Vec<CustomResourceDefinition> {
    use crate::crd::{
        CapitalMarketsSchedule, ScheduledCapacity, ScheduledMachine, TimeBasedSpotSchedule,
    };
    use kube::CustomResourceExt;

    vec![
        prepared(ScheduledMachine::crd()),
        prepared(ScheduledCapacity::crd()),
        prepared(TimeBasedSpotSchedule::crd()),
        prepared(CapitalMarketsSchedule::crd()),
    ]
}

/// Apply the post-generation fix-ups to a CRD before it is serialized.
///
/// Today that is [`strip_unsupported_formats`]; every consumer of a
/// generated CRD goes through here so a new fix-up reaches all of them.
#[must_use]
pub fn prepared(mut crd: CustomResourceDefinition) -> CustomResourceDefinition {
    strip_unsupported_formats(&mut crd);
    crd
}

/// Remove every `format` the apiserver does not recognise from each
/// version's schema.
///
/// `schemars` names Rust's unsigned integers `uint8` to `uint64`. Kubernetes
/// knows none of them: it drops them from the stored schema, so they
/// validate nothing, and warns on apply (`unrecognized format "uint8"`).
/// The bounds that matter (`minimum: 0`, `maximum`) are separate keywords
/// and stay.
pub fn strip_unsupported_formats(crd: &mut CustomResourceDefinition) {
    for version in &mut crd.spec.versions {
        if let Some(root) = version
            .schema
            .as_mut()
            .and_then(|s| s.open_api_v3_schema.as_mut())
        {
            strip_unsupported_schema_formats(root);
        }
    }
}

/// Whether the apiserver keeps `format` on a `type: string` (or untyped)
/// schema. Dashes are ignored, as the apiserver ignores them.
#[must_use]
pub fn is_supported_string_format(format: &str) -> bool {
    STRING_FORMATS.contains(&format.replace('-', "").as_str())
}

/// [`strip_unsupported_formats`] for one schema and everything under it:
/// properties, items, additional properties and items, the combinators,
/// pattern properties, definitions and schema dependencies.
pub fn strip_unsupported_schema_formats(schema: &mut JSONSchemaProps) {
    if let Some(format) = schema.format.as_deref() {
        let keep = match schema.type_.as_deref() {
            Some("integer") => INTEGER_FORMATS.contains(&format),
            Some("number") => NUMBER_FORMATS.contains(&format),
            None | Some("string") => is_supported_string_format(format),
            Some(_) => false,
        };
        if !keep {
            schema.format = None;
        }
    }

    let maps = [
        schema.properties.as_mut(),
        schema.pattern_properties.as_mut(),
        schema.definitions.as_mut(),
    ];
    for child in maps.into_iter().flatten().flat_map(|m| m.values_mut()) {
        strip_unsupported_schema_formats(child);
    }
    let lists = [
        schema.all_of.as_mut(),
        schema.any_of.as_mut(),
        schema.one_of.as_mut(),
    ];
    for child in lists.into_iter().flatten().flat_map(|v| v.iter_mut()) {
        strip_unsupported_schema_formats(child);
    }
    match schema.items.as_mut() {
        Some(JSONSchemaPropsOrArray::Schema(child)) => strip_unsupported_schema_formats(child),
        Some(JSONSchemaPropsOrArray::Schemas(children)) => {
            children
                .iter_mut()
                .for_each(strip_unsupported_schema_formats);
        }
        None => {}
    }
    for child in [
        schema.additional_properties.as_mut(),
        schema.additional_items.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        if let JSONSchemaPropsOrBool::Schema(child) = child {
            strip_unsupported_schema_formats(child);
        }
    }
    if let Some(child) = schema.not.as_mut() {
        strip_unsupported_schema_formats(child);
    }
    for dependency in schema.dependencies.iter_mut().flat_map(|m| m.values_mut()) {
        if let JSONSchemaPropsOrStringArray::Schema(child) = dependency {
            strip_unsupported_schema_formats(child);
        }
    }
}

#[cfg(test)]
#[path = "crd_schema_tests.rs"]
mod tests;
