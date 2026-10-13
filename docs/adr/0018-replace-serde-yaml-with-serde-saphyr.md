<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0018: Replace `serde_yaml` with `serde-saphyr`, behind one output-style wrapper

- **Status:** Accepted
- **Date:** 2026-10-13
- **Deciders:** Erick Bourgeois
- **Supersedes:** (none)
- **Related:** ADR [0015](./0015-dependency-internalization-policy.md) (dependency policy), roadmap [04](../../.github/community/04-dependency-internalization-matrix.md), `src/yaml.rs`, `src/bin/crdgen.rs`

## Context

`serde_yaml` is deprecated upstream and its backend, `unsafe-libyaml`, is
unmaintained. That is **RUSTSEC-2024-0449**, and it fails the `cargo-deny` job
in `build.yaml`, which runs `check --all-features` with no `continue-on-error`
and a `deny.toml` carrying `ignore = []`. The advisory is not theoretical for
us in one specific sense: YAML parsing is exactly the kind of surface where an
unmaintained C-derived parser is a liability, and 5-Spot's YAML path produces
the CRDs operators apply to a cluster.

The project's dependency policy (ADR 0015) puts YAML squarely in the
"specifications and wire formats" category: never internalize it, because the
used surface is one call over a conformant emitter. So the choice was between
suppressing the advisory and replacing the crate.

Three things made replacement cheap:

1. **The surface really is one call.** `serde_yaml::to_string` in
   `src/bin/crdgen.rs`, plus `from_str` in two test files and one
   multi-document parse.
2. **The replacement is already in the graph.** `serde-saphyr 0.0.29` arrives
   transitively through `kube-client 4.2.0`, so this adds a direct dependency on
   a crate family already compiled into the production build rather than a new
   one.
3. **Two sibling projects had already done it.** `bindy` and `banlieue` both run
   `serde-saphyr = "1.3"` with the same wrapper shape, so the migration had a
   known-good reference rather than being designed here.

The obstacle is that `serde-saphyr`'s defaults differ from `serde_yaml`'s in two
ways that would churn every generated manifest:

- strings longer than `folded_wrap_chars` (80 by default) are rewritten as
  folded block scalars (`>-`), so a long CRD description would be rewrapped
  across lines;
- blank lines inside a block scalar are written carrying the block's
  indentation, leaving trailing whitespace on otherwise-empty lines.

Alternatives weighed:

- **Add RUSTSEC-2024-0449 to `deny.toml`'s `ignore`.** Rejected: it keeps an
  unmaintained C-derived YAML parser in the artifact that writes cluster
  manifests, and converts a fixable finding into permanent noise. The `ignore`
  list should hold things we have judged and accepted, not things we have not
  got round to.
- **Configure the serializer at each call site.** Rejected: four call sites in
  `crdgen` today, and nothing would stop the fifth from differing. Generated
  output style is a property of the project, not of a call site.
- **Accept the reformatting and commit the folded output.** Rejected: it churns
  every long description in four committed CRDs for no benefit, and folding is
  lossy for leading and trailing spaces, so it is a semantic risk as well as a
  diff.

## Decision

We depend on `serde-saphyr` and remove `serde_yaml`.

Every YAML value 5-Spot *emits* goes through a single wrapper,
`five_spot::yaml::to_string`, which:

- sets `folded_wrap_chars` to `usize::MAX`, so a long scalar is never folded
  onto continuation lines;
- strips the indentation from lines that contain nothing else, so no emitted
  line ends in whitespace.

Deserialization uses `serde_saphyr::from_str`, or `from_multiple` for a
multi-document stream, with `serde_json::Value` as the dynamic value type since
`serde-saphyr` exposes none of its own.

`SerializerOptions` is `#[non_exhaustive]`, so the wrapper mutates
`Default::default()` rather than using a struct expression. That is a constraint
of the crate, not a style choice.

## Consequences

**Easier.** `cargo deny` passes: `advisories ok, bans ok, licenses ok,
sources ok`. The YAML emitter is safe Rust. There is exactly one place that
decides what generated YAML looks like, and it is the same shape as the one in
`bindy` and `banlieue`, so a reader moving between the three projects finds the
same wrapper.

**Harder.** Anyone emitting YAML must remember to use the wrapper rather than
`serde_saphyr::to_string` directly, or they silently get folded output and
trailing whitespace. Nine tests in `src/yaml_tests.rs` pin the three properties
that matter (no trailing whitespace, no folding, round-trip fidelity), so a
regression in the wrapper fails loudly, but a *bypass* of the wrapper would not.

**Ruled out.** Suppressing RUSTSEC-2024-0449. If the advisory reappears through
another path, the answer is to fix the path.

**Known wart.** Two `serde-saphyr` versions are in the graph: our direct 1.3.0
and 0.0.29 via `kube-client`. `bans.multiple-versions` is `warn`, so this is
tolerated rather than accepted silently; it resolves when `kube` moves forward.

**Verification.** The generated CRDs changed only in quote style, for example
`- 'True'` becoming `- "True"` and a single-quoted description with embedded
double quotes becoming a double-quoted one with escapes. All four CRDs were
parsed before and after and compared structurally: **every one is semantically
identical**, and the `"True"` enum member remains a string rather than becoming
a boolean, which was the one case worth checking. `make crds` and `make crddoc`
are idempotent. `cargo fmt`, `clippy -D warnings` and 869 tests pass.

**CALM impact:** none. No node, relationship, interface or flow changes. This
replaces the library that serializes an existing artifact; the artifact, its
consumers and its contract are unchanged.
