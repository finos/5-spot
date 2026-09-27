<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Roadmaps

High-level index of 5-Spot's roadmap documents. Full detail for each one lives
in [`.github/community/`](.github/community/) — this file tracks what each one
is and its current status; the task lists and rationale live in the linked doc.

Per [`.claude/rules/architecture-driven-development.md`](.claude/rules/architecture-driven-development.md),
architecturally significant work in any roadmap below still goes
**ADR → CALM → TDD → implement → docs**, in that order — a roadmap entry
describes *what* and *why*, it does not skip the ADR for *how*.

## Status legend

| Symbol | Meaning |
|---|---|
| ✅ | Done — implemented, tested, in the codebase today |
| 🔶 | In progress — some of it exists, not complete |
| ⛔ | Not started |
| 📄 | Reference doc — not a phase with a completion state |

## Index

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [00](.github/community/00-overview.md) | Roadmap overview | 📄 | **Read this first.** What 5-Spot is, the `v0.3.0` baseline, the non-negotiables, repository layout, pre-flight checks, and versioning policy — the context every other roadmap doc assumes |
| [01](.github/community/01-decisions.md) | Locked design decisions | 📄 | Quick-reference digest of the locked decisions (toolchain, kube-rs, API groups, provider-reference activation, CAPI handling, Kata delivery, admission, CRD evolution, supply chain, errors, observability, testing, publish-safety), each citing its ADR — `docs/adr/` stays canonical. Ends with the open-decisions table (`hyper`/`tower` removal, conversion-webhook shape, thin-dep internalization) |
| [02](.github/community/02-conventions.md) | Coding conventions | 📄 | Working digest of how code is written here: style (early returns, no magic numbers), errors, logging, the event-driven reconciliation pattern, CRD authoring order, `_tests.rs` testing rules, docs discipline, commit rules, and the pre-finish publish-safety sweeps |
| [03](.github/community/03-release-diff-v0-2-2-to-main.md) | Release diff `v0.2.2` → `main` | 📄 | Point-in-time record of two feature arcs — per-node Kata containerd config delivery (ADR 0002/0003, with ADR 0005 closing the node-root-via-CRD path and ADR 0004 adding the agent pod-security VAP) and the pluggable spot-schedule provider architecture (ADR 0006/0007/0009), plus the `v1alpha1` → `v1beta1` move. Generated 2026-07-22; superseded for anything later by `docs/adr/` and `.claude/CHANGELOG.md` |
| [04](.github/community/04-dependency-internalization-matrix.md) | Dependency internalization matrix | 🔶 | Every direct dependency, its actually-used API surface, and whether it could be maintained internally. Audited 2026-09-23: four of the six unused crates are gone (`regex`, `lazy_static`, `async-trait`, `hyper-util`) and direct runtime deps are 31 → 25. **Outstanding: `hyper` — still declared with `features = ["full"]` — and `tower`, both with zero non-comment references under `src/`** |

## Recorded elsewhere, deliberately

Not everything that shapes this project is a roadmap:

| Subject | Record |
|---|---|
| Architectural decisions | [`docs/adr/`](docs/adr/) — canonical, one decision per ADR ([index](docs/adr/README.md)) |
| Security posture — assets, boundaries, STRIDE, residual risks | [`docs/src/security/threat-model.md`](docs/src/security/threat-model.md) |
| What changed and why, per task | [`.claude/CHANGELOG.md`](.claude/CHANGELOG.md) |
| Unremediated security findings | **Not in this repo.** [Private vulnerability reporting](https://github.com/finos/5-spot/security/advisories/new), per [`SECURITY.md`](SECURITY.md) |
