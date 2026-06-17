// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # CRD YAML generator
//!
//! Offline tool that serialises the 5-Spot Custom Resource Definitions to YAML
//! on **stdout**. The caller decides where the output goes — the binary makes no
//! assumptions about the repository layout (mirrors `crddoc`). The committed
//! artifacts under `deploy/crds/` are produced by `make crds`, which redirects
//! one CRD per file.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --bin crdgen                       # all CRDs as a multi-doc stream
//! cargo run --bin crdgen scheduledmachine      # just one, to stdout
//! cargo run --bin crdgen | kubectl apply -f -  # pipe straight to a cluster
//! ```
//!
//! Accepted selectors (case-insensitive): `scheduledmachine`,
//! `timebasedspotschedule`, `capitalmarketsschedule`. With no selector every CRD
//! is emitted, separated by the YAML document marker `---`.
//!
//! The Rust types in `src/crd.rs` are the **single source of truth**. Re-run
//! `make crds` after any change to `src/crd.rs` and commit the refreshed YAML.

use clap::{Parser, ValueEnum};
use five_spot::crd::{CapitalMarketsSchedule, ScheduledMachine, TimeBasedSpotSchedule};
use kube::CustomResourceExt;

/// The CRDs this tool can emit. Each selector's `value` matches the committed
/// `deploy/crds/<selector>.yaml` filename and the Makefile's `CRD_SELECTORS`.
#[derive(Copy, Clone, Debug, ValueEnum)]
enum CrdSelector {
    #[value(name = "scheduledmachine")]
    ScheduledMachine,
    #[value(name = "timebasedspotschedule")]
    TimeBasedSpotSchedule,
    #[value(name = "capitalmarketsschedule")]
    CapitalMarketsSchedule,
}

impl CrdSelector {
    /// All selectors, in the canonical emit order used by the no-arg stream.
    const ALL: [CrdSelector; 3] = [
        CrdSelector::ScheduledMachine,
        CrdSelector::TimeBasedSpotSchedule,
        CrdSelector::CapitalMarketsSchedule,
    ];

    /// Serialise this CRD to a YAML string.
    fn render(self) -> String {
        let yaml = match self {
            CrdSelector::ScheduledMachine => serde_yaml::to_string(&ScheduledMachine::crd()),
            CrdSelector::TimeBasedSpotSchedule => {
                serde_yaml::to_string(&TimeBasedSpotSchedule::crd())
            }
            CrdSelector::CapitalMarketsSchedule => {
                serde_yaml::to_string(&CapitalMarketsSchedule::crd())
            }
        };
        yaml.expect("serialise CRD to YAML")
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "crdgen",
    about = "Serialise 5-Spot CRDs to stdout (the caller owns where output goes)"
)]
struct Cli {
    /// Which CRD to emit. Omit to emit all CRDs as a `---`-separated multi-doc
    /// YAML stream (e.g. `crdgen | kubectl apply -f -`).
    #[arg(value_enum)]
    crd: Option<CrdSelector>,
}

fn main() {
    let cli = Cli::parse();

    // A single selector → just that CRD.
    if let Some(selector) = cli.crd {
        print!("{}", selector.render());
        return;
    }

    // No selector → every CRD as a multi-document YAML stream.
    for (i, selector) in CrdSelector::ALL.iter().enumerate() {
        if i > 0 {
            println!("---");
        }
        print!("{}", selector.render());
    }
}
