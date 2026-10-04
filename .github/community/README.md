<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Community / roadmap documents

Detailed roadmap and reference documents for 5-Spot. The one-screen status
board is [`ROADMAPS.md`](../../ROADMAPS.md) at the repo root — this directory
holds the detail each row links to.

## Reading order

| # | Document | Status | What it is |
|---|---|---|---|
| [00](00-overview.md) | Roadmap overview | 📄 | **Read this first.** What 5-Spot is, the `v0.3.0` baseline, the non-negotiables, repo layout, and pre-flight checks every other doc assumes |
| [01](01-decisions.md) | Locked design decisions | 📄 | Quick-reference digest of the locked decisions, each citing its ADR — `docs/adr/` stays canonical. Ends with the open-decisions table |
| [02](02-conventions.md) | Coding conventions | 📄 | How code in this tree is written: style, errors, logging, the reconciliation pattern, CRD authoring, testing, docs discipline, commit rules |
| [03](03-release-diff-v0-2-2-to-main.md) | Release diff `v0.2.2` → `main` | 📄 | What shipped across the kata-config-agent and spot-schedule-provider arcs, with the ADRs behind each. A point-in-time record, generated 2026-07-22 |
| [04](04-dependency-internalization-matrix.md) | Dependency internalization matrix | ✅ | Every direct dependency, the API surface actually used, and whether it could be maintained internally. Closed 2026-09-27: all six unused crates removed, runtime deps 31 → 22; §§2–3 stay as the internalization reference |
| [05](05-schedule-gated-capacity.md) | Schedule-gated capacity | ✅ | The non-handover pattern (ADR 0011): a host stays in the cluster and a bounded slice of it is conceded on a schedule, through a `ScheduledCapacity` CRD and a separate controller identity that patches one field on a foreign API group. Closed 2026-10-04, verified against a live cluster |

## Conventions

Filenames are **lowercase with hyphens**, prefixed by a **zero-padded
two-digit number, contiguous from `00`** — the number is a position in the
reading order, not a category. `README.md` is the sole uppercase name here.
Inserting or retiring a document renumbers the run and fixes every reference in
the same commit. The full rule, including what a renumber has to touch, is
[`.claude/rules/documentation.md`](../../.claude/rules/documentation.md).

Architecturally significant work described in any document here still goes
**ADR → CALM → TDD → implement → docs**, per
[`.claude/rules/architecture-driven-development.md`](../../.claude/rules/architecture-driven-development.md).
A roadmap entry describes *what* and *why*; it does not skip the ADR for *how*.

## What does not live here

Roadmap documents in this directory are **public** — this is a public
repository. Anything that would be unsafe or unhelpful to publish stays in the
maintainer's private directory outside the repo (`~/dev/roadmaps/5-spot/`):

- Unremediated security findings. Those go through
  [private vulnerability reporting](https://github.com/finos/5-spot/security/advisories/new)
  per [`SECURITY.md`](../../SECURITY.md); the published posture lives in
  [`docs/src/security/threat-model.md`](../../docs/src/security/threat-model.md).
- Real infrastructure identifiers — hostnames, addresses, cluster snapshots,
  internal registry paths. See the internal-references rule in
  [`.claude/CLAUDE.md`](../../.claude/CLAUDE.md).
