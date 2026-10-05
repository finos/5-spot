<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0015: Internalize a dependency when its used surface is 500 lines or less

- **Status:** Accepted
- **Date:** 2026-10-05
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** Closes open thread O-003 in roadmap
  [01](../../.github/community/01-decisions.md); roadmap
  [04](../../.github/community/04-dependency-internalization-matrix.md) §2 is
  the per-dependency surface analysis this policy is applied to; ADR
  [0008](./0008-autovex-presubmission-gate.md) (auto-VEX, which is what stops
  reporting on internalized code); rule
  `.claude/rules/dependency-internalization.md`

## Context

A one-symbol dependency is not free. It still obliges the project to track the
crate's releases, triage its advisories, and wait on a maintainer for a fix it
could have made itself. In a regulated environment every version bump is also a
change to review. The preference is therefore to own small code rather than
track an upstream cadence for it.

Roadmap 04 already measured the used surface of every direct dependency and
removed six that were not used at all. It left a judgment call open (O-003,
"internalizing the thin tier") and declined to take it, on the grounds that
most of those trades are bad. Applying a flat line-count test to that matrix
shows **why** they are bad, and the reason is sharper than "bad trade":

**A thin surface over a thick, moving specification is not a small
dependency.** The surface measurement answers "how coupled are we", not "how
much code would we own". Four categories break the line-count test entirely:

- **Specifications and wire formats.** `serde_yaml`'s used surface is one call,
  `to_string`. The implementation is a conformant YAML emitter. Owning it means
  owning YAML's parsing CVEs and its spec drift. Same for `toml`,
  `serde_json`, and `prometheus`'s exposition format.
- **Reference data.** `chrono-tz` is one symbol, `Tz`, and that symbol *is* the
  IANA timezone database. DST rules change several times a year by government
  decree. A time-based scheduler is the worst possible place to hand-maintain
  them.
- **Cryptographic primitives.** `sha2` is two symbols. Hand-rolling SHA-256 is
  never correct, and in a regulated environment it is an audit question as well
  as a correctness one.
- **Procedural macros.** `serde`, `schemars`, `clap` and `thiserror` each show
  a surface of one or two *derive names*. There is no 500 lines to bring over:
  the implementation is a compiler plugin, and the alternative is hand-written
  impls at every call site forever. `schemars` additionally feeds CRD
  generation.

A fifth constraint emerged from measuring rather than reasoning: **dropping a
direct dependency does not necessarily remove the crate.** If another
dependency pulls it in, it is still compiled, still in the SBOM, and still ours
to upgrade. Internalizing it then buys nothing and costs duplicate code. This
has to be checked, not assumed: `http` appears 14 times in the production graph
because `kube` depends on it, so our 54 direct uses are irrelevant to whether
we could drop it. We could not.

The check itself has a trap. `cargo tree -e normal` reports the **host**
target, so on a macOS workstation it silently omits every
`[target.'cfg(target_os = "linux")'.dependencies]` entry. Measured that way
`nix` looks absent from the production graph and is in fact present: 225 crates
on `x86_64-unknown-linux-gnu` against 217 on macOS. The rule therefore requires
an explicit `--target`.

And a trade that argues *against* the policy at scale: internalized code leaves
the SBOM. `cargo audit`, `cargo deny` and the Grype scan in ADR 0008's pipeline
stop reporting on it, so a defect becomes something found by review rather than
by a scanner. That is acceptable for twenty well-tested lines and
unacceptable for a parser.

### Options

| | Option | For | Against |
| --- | --- | --- | --- |
| A | **Keep every dependency; upgrade as upstream releases.** | No code owned, scanners cover everything. | Pays release-tracking and advisory-triage cost even for crates whose used surface is a dozen lines, and blocks on upstream for fixes we could make. |
| B | **Flat rule: internalize anything whose used surface is ≤500 lines.** | Simple, mechanical, no case-by-case argument. | Applied literally it internalizes SHA-256 and the IANA timezone database, and it cannot be applied at all to derive macros. It mistakes coupling for size. |
| C | **The 500-line test, with excluded categories and a production-graph check.** *Chosen.* | Keeps B's intent and bite where the code really is small and stable, and names the four categories where the measurement lies. | Needs judgment at the boundary, so the exclusion list has to be maintained as dependencies change. |
| D | **Vendor with `cargo vendor` instead of rewriting.** | No behaviour risk; upstream code verbatim. | Carries third-party licences and notices into an Apache-2.0 FINOS repo, and still leaves us maintaining a fork with none of the understanding that writing it gives. |

## Decision

**A dependency is internalized when its used surface can be replaced in 500
lines or fewer, it is not in an excluded category, we can test it at least as
well as upstream for the surface we use, and dropping it actually removes the
crate from the production graph.** All four conditions, measured not estimated.

The excluded categories are specifications and wire formats, cryptographic
primitives, reference data, date and time arithmetic, procedural macros, and
the platform itself (`kube`, `k8s-openapi`, `tokio`, `futures`, `tracing`).

Each internalization gets an ADR recording the measured numbers. Reimplement
against the underlying API rather than vendoring, so no third-party licence is
carried; where code must be copied, preserve its notice and say so. The module
doc states the behavioural contract and `_tests.rs` pins every property, because
upstream's edge cases are the specification and losing them silently is the
failure mode.

Full rule, including the category table and the requirements checklist:
`.claude/rules/dependency-internalization.md`.

### First application: `tokio-stream`

Measured, not estimated:

| | |
| --- | --- |
| Symbols used | `wrappers::ReceiverStream::new`, one |
| Upstream crate | 3,037 LOC, of which the file we used is 107 |
| Replacement | **19 LOC** (`src/stream.rs`), 5 tests |
| Excluded category | no |
| Production graph | `cargo tree -e normal`: 1 occurrence before, **0 after** |
| Transitive deps removed | none; the crate has none of its own |

It survives only as a dev-dependency through `tower-test`, so it is out of the
shipped binary and the production SBOM. The five tests pin the properties
`Controller::reconcile_on` depends on, and which a careless reimplementation
would lose: send order, draining buffered items *before* the end rather than
dropping them with the sender, ending once every sender is gone, `Pending`
rather than end-of-stream on an open empty channel, and `None` being terminal
and repeatable.

## Consequences

**Easier.** Trivially thin dependencies stop costing release-tracking and
advisory triage. The decision is now a measurement plus a category check rather
than an argument each time, and roadmap 04's matrix is the input to it.

**Harder / newly ruled out.**

- Internalized code is invisible to dependency scanning. That is the standing
  cost of this policy and the main reason the 500-line bound is low.
- The exclusion list is a maintenance burden: a new dependency has to be
  classified, and the categories are judgments rather than facts. "When unsure,
  keep the dependency" is the tie-break, which is the opposite default from
  ADD's "when unsure, write the ADR".
- Option A's property, that scanners cover everything we ship, is genuinely
  lost at the margin. The mitigation is the low bound and the test requirement,
  not a scanner.
- Applying the policy to the rest of roadmap 04's thin tier is **not**
  implied. `kube-lease-manager` (~150 to 250 lines plus edge cases), `warp`
  (two tiny servers, but the filter machinery is the replacement cost) and
  `nix` (swapping one dependency for `libc` is lateral) each need their own
  measurement and ADR, and two of the three look like bad trades on the
  production-graph test alone.

**CALM impact: none, process-only.** CALM models the running system: the
controller, the CRDs, the CAPI and provider resources and the flows between
them. A dependency policy changes none of those. No diagram or node changes.

**Threat model impact: yes, small.** §3 lists supply chain as an asset and §7
records the auto-VEX and `cargo audit` controls. Internalization narrows what
those controls see, which is a posture change even though it reduces the
dependency count. A full pass is due with this ADR.
