// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! # Symbol-import reachability auto-VEX (CI tool, roadmap Phase 3)
//!
//! Reads a Grype JSON report, the `nm -D --undefined-only` output of
//! the release binary, and a curated CVE → [function names] mapping.
//! For each Grype CVE in the mapping whose listed functions are *all
//! absent* from the binary's symbol-import table, emits an OpenVEX
//! `not_affected + vulnerable_code_not_in_execute_path` statement.
//!
//! ## Usage
//!
//! ```bash
//! nm -D --undefined-only target/release/5spot > symbols.txt
//! cargo run --bin auto-vex-reachability -- \
//!     --grype-json grype.json \
//!     --binary-symbols symbols.txt \
//!     --affected-functions .vex/.affected-functions.json \
//!     --vex-dir .vex \
//!     --product-purl pkg:oci/5-spot \
//!     --id "https://github.com/finos/5-spot/actions/runs/123/auto-vex-reachability" \
//!     --author auto-vex-reachability \
//!     --output vex.auto-reachability.json
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use five_spot::auto_vex_reachability::{
    build_document, compute_reachability_vex, load_affected_functions_from_path,
    load_triaged_from_vex_dir, parse_nm_output, GrypeReport,
};

#[derive(Parser, Debug)]
#[command(
    name = "auto-vex-reachability",
    about = "Emit OpenVEX vulnerable_code_not_in_execute_path statements when affected functions aren't imported by the release binary"
)]
struct Cli {
    /// Grype JSON report (produced by `grype --output json`).
    #[arg(long)]
    grype_json: PathBuf,

    /// Path to the text output of `nm -D --undefined-only <binary>`.
    /// Each line is one undefined (imported) symbol.
    #[arg(long)]
    binary_symbols: PathBuf,

    /// JSON file mapping CVE id → list of public-API function names.
    /// Underscore-prefixed keys are treated as metadata and skipped.
    #[arg(long)]
    affected_functions: PathBuf,

    /// Directory of hand-authored `.vex/*.json`; CVEs already covered
    /// by one of these are skipped. Missing dir is OK.
    #[arg(long)]
    vex_dir: PathBuf,

    /// Product purl to attach to every emitted statement.
    #[arg(long)]
    product_purl: String,

    /// Canonical `@id` for the document (typically a CI run URL).
    #[arg(long)]
    id: String,

    /// Document-level author.
    #[arg(long, default_value = "auto-vex-reachability")]
    author: String,

    /// RFC-3339 UTC timestamp. Defaults to now() at process start.
    #[arg(long)]
    timestamp: Option<String>,

    /// Output path. Defaults to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("auto-vex-reachability: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let grype_bytes = std::fs::read(&cli.grype_json).map_err(|e| {
        anyhow::anyhow!(
            "failed to read --grype-json {}: {}",
            cli.grype_json.display(),
            e
        )
    })?;
    let grype: GrypeReport = serde_json::from_slice(&grype_bytes).map_err(|e| {
        anyhow::anyhow!(
            "failed to parse --grype-json {}: {}",
            cli.grype_json.display(),
            e
        )
    })?;

    let nm_text = std::fs::read_to_string(&cli.binary_symbols).map_err(|e| {
        anyhow::anyhow!(
            "failed to read --binary-symbols {}: {}",
            cli.binary_symbols.display(),
            e
        )
    })?;
    let imported_symbols = parse_nm_output(&nm_text);

    let affected = load_affected_functions_from_path(&cli.affected_functions).map_err(|e| {
        anyhow::anyhow!(
            "failed to load --affected-functions {}: {}",
            cli.affected_functions.display(),
            e
        )
    })?;

    let triaged: HashSet<String> = load_triaged_from_vex_dir(&cli.vex_dir).map_err(|e| {
        anyhow::anyhow!("failed to load --vex-dir {}: {}", cli.vex_dir.display(), e)
    })?;

    let timestamp = cli
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());

    let statements = compute_reachability_vex(
        &grype,
        &imported_symbols,
        &affected,
        &triaged,
        &cli.product_purl,
        &timestamp,
    );
    let doc = build_document(statements, &cli.id, &cli.author, &timestamp);
    let rendered = serde_json::to_string_pretty(&doc)? + "\n";

    match &cli.output {
        Some(path) => std::fs::write(path, &rendered)
            .map_err(|e| anyhow::anyhow!("failed to write --output {}: {}", path.display(), e))?,
        None => print!("{rendered}"),
    }

    eprintln!(
        "auto-vex-reachability: {} grype match(es), {} symbol(s) imported, \
         {} mapped CVE(s), {} statement(s) emitted ({} already triaged in {})",
        grype.matches.len(),
        imported_symbols.len(),
        affected.len(),
        doc.statements.len(),
        triaged.len(),
        cli.vex_dir.display(),
    );
    Ok(())
}
