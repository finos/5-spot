// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
//! # CRD YAML generator
//!
//! Offline tool that serialises the `ScheduledMachine` Custom Resource Definition
//! to YAML and writes it to `stdout`.  The output is committed to
//! `deploy/crds/scheduledmachine.yaml` so that cluster operators can apply the
//! CRD without running `cargo`.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml
//! ```
//!
//! The Rust types in `src/crd.rs` are the **single source of truth**.  Always
//! re-run this binary after any change to `src/crd.rs` and commit the updated
//! YAML alongside the code change.

use five_spot::crd::ScheduledMachine;
use kube::CustomResourceExt;

/// Serialise the `ScheduledMachine` CRD to YAML and print it to `stdout`.
///
/// # Panics
/// Panics if the CRD cannot be serialised (this indicates a programming error
/// in the schemars/kube-derive annotations, not a runtime condition).
fn main() {
    let crd = ScheduledMachine::crd();
    println!("{}", serde_yaml::to_string(&crd).unwrap());
}
