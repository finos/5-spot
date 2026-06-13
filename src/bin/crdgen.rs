// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # CRD YAML generator
//!
//! Offline tool that serialises the 5-Spot Custom Resource Definitions to YAML
//! and writes them under `deploy/crds/`, the committed artifacts cluster
//! operators apply without a Rust toolchain.
//!
//! It emits **two** files:
//! - `deploy/crds/scheduledmachine.yaml` — the `ScheduledMachine` CRD, a
//!   **multi-version** object merging the frozen, deprecated `v1alpha1` and the
//!   storage `v1beta1` (ADR 0007). Exactly one version is `storage: true`. A
//!   spec-level CEL `x-kubernetes-validations` rule requiring at least one of
//!   `spec.schedule` / `spec.spotSchedule` is injected into the `v1beta1`
//!   schema (ADR 0006).
//! - `deploy/crds/capitalmarketsschedule.yaml` — the reference spot-schedule
//!   provider CRD (`spotschedules.5spot.finos.org`, ADR 0006).
//!
//! ## Usage
//!
//! ```bash
//! cargo run --bin crdgen
//! ```
//!
//! The Rust types in `src/crd.rs` are the **single source of truth**. Always
//! re-run this binary after any change to `src/crd.rs` and commit the updated
//! YAML alongside the code change.

use std::path::Path;

use five_spot::crd::{v1alpha1, CapitalMarketsSchedule, ScheduledMachine};
use kube::core::crd::merge_crds;
use kube::CustomResourceExt;
use serde_json::{json, Value};

/// Storage (and current) version of `ScheduledMachine`.
const SCHEDULED_MACHINE_STORAGE_VERSION: &str = "v1beta1";

/// Generate the 5-Spot CRD YAML files under `deploy/crds/`.
///
/// # Panics
/// Panics if a CRD cannot be serialised, the multi-version merge fails, or a
/// file cannot be written — each indicates a programming/build error, not a
/// runtime condition.
fn main() {
    let crds_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/crds");

    // ScheduledMachine: merge the storage v1beta1 with the frozen v1alpha1 into
    // a single multi-version CRD, then inject the schedule/spotSchedule CEL.
    let merged = merge_crds(
        vec![ScheduledMachine::crd(), v1alpha1::ScheduledMachine::crd()],
        SCHEDULED_MACHINE_STORAGE_VERSION,
    )
    .expect("merge ScheduledMachine v1alpha1 + v1beta1 CRDs");

    let mut sm_json =
        serde_json::to_value(&merged).expect("serialise ScheduledMachine CRD to JSON");
    inject_schedule_xor_validation(&mut sm_json, SCHEDULED_MACHINE_STORAGE_VERSION);
    write_yaml(&crds_dir.join("scheduledmachine.yaml"), &sm_json);

    // CapitalMarketsSchedule: reference spot-schedule provider (single version).
    let cms = CapitalMarketsSchedule::crd();
    let cms_json =
        serde_json::to_value(&cms).expect("serialise CapitalMarketsSchedule CRD to JSON");
    write_yaml(&crds_dir.join("capitalmarketsschedule.yaml"), &cms_json);
}

/// Inject the spec-level CEL rule requiring at least one of `spec.schedule` /
/// `spec.spotSchedule` into the named version's schema (ADR 0006).
///
/// The rule is expressed as a CRD root-`spec` `x-kubernetes-validations` entry
/// because it spans two optional sibling fields — neither a field-level schema
/// nor a single-field constraint can express it. The kube-derive schema is
/// otherwise purely structural; this is the one cross-field invariant.
///
/// # Panics
/// Panics if the CRD JSON does not contain the expected
/// `spec.versions[name].schema.openAPIV3Schema.properties.spec` path — that
/// would mean the kube-derive output shape changed and this generator needs
/// updating.
fn inject_schedule_xor_validation(crd: &mut Value, version: &str) {
    let versions = crd
        .get_mut("spec")
        .and_then(|s| s.get_mut("versions"))
        .and_then(Value::as_array_mut)
        .expect("CRD spec.versions array");

    let target = versions
        .iter_mut()
        .find(|v| v.get("name").and_then(Value::as_str) == Some(version))
        .unwrap_or_else(|| panic!("CRD has no version named {version}"));

    let spec_schema = target
        .get_mut("schema")
        .and_then(|s| s.get_mut("openAPIV3Schema"))
        .and_then(|s| s.get_mut("properties"))
        .and_then(|p| p.get_mut("spec"))
        .and_then(Value::as_object_mut)
        .expect("openAPIV3Schema.properties.spec object");

    spec_schema.insert(
        "x-kubernetes-validations".to_string(),
        json!([
            {
                "rule": "has(self.schedule) || has(self.spotSchedule)",
                "message": "at least one of spec.schedule or spec.spotSchedule must be set",
            }
        ]),
    );
}

/// Serialise a CRD JSON `Value` to YAML and write it to `path`.
///
/// # Panics
/// Panics if YAML serialisation or the file write fails.
fn write_yaml(path: &Path, crd: &Value) {
    let yaml = serde_yaml::to_string(crd).expect("serialise CRD to YAML");
    std::fs::write(path, yaml).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    eprintln!("✓ wrote {}", path.display());
}
