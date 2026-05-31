<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0001 — Adopt Architecture Driven Development (ADD)

- **Status:** Accepted
- **Date:** 2026-05-31
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** `.claude/rules/architecture-driven-development.md`; CALM model at `docs/architecture/calm/architecture.json`; the existing TDD discipline in `.claude/CLAUDE.md`.

## Context

5-Spot already enforces strong **Test-Driven Development** (tests first, separate
`_tests.rs` files, `cargo-quality` as a hard gate) and already maintains a
**FINOS CALM** architecture model with `make calm-validate` / `make
calm-diagrams`. What it lacked was an explicit, ordered methodology tying the
*decision*, the *model*, and the *code* together — and a durable home for the
"why A over B" reasoning behind architecturally significant changes.

Several recent changes illustrate the gap. The RBAC anti-escalation design (VAP
`authorizer` checks for the requesting user + a controller-side
`SelfSubjectAccessReview`), the embedded-`metadata` policy (reject
`name`/`namespace`, allow reserved-prefix-checked `labels`/`annotations`), and
the child-cluster Node-routing model were all real architectural decisions with
non-obvious trade-offs. They were captured in the changelog and code comments,
but there was no first-class record of the decision and the alternatives
weighed — so the next contributor must re-derive the reasoning from the diff.

A sibling project (banlieue) adopted **Architecture Driven Development**:
decisions are recorded as ADRs and modeled in CALM *before* code, on top of the
existing TDD loop. The order is fixed:

```
ADR  →  CALM  →  TDD  →  implement  →  docs
```

The alternative — staying TDD-only and relying on the changelog plus code
comments — keeps the process lighter, but leaves architectural intent scattered
and reconstructed rather than stated. Given 5-Spot operates in a regulated
environment where changes must be auditable and traceable to a rationale, a
first-class decision log is worth the modest per-change overhead.

## Decision

We adopt **Architecture Driven Development (ADD)** as the governing methodology
for 5-Spot. For any architecturally significant change, contributors complete
the steps in order — **ADR → CALM → TDD → implement → docs** — before the next
step begins:

1. **ADR** — record the decision in `docs/adr/NNNN-title.md` (Status / Context /
   Decision / Consequences), from `docs/adr/template.md`, indexed in
   `docs/adr/README.md`.
2. **CALM** — reflect it in `docs/architecture/calm/architecture.json`;
   `make calm-validate` + `make calm-diagrams` must pass. *Process-only decisions
   have no CALM impact and say so.*
3. **TDD** — failing tests first, then minimum implementation; `cargo-quality`
   gate; CRD changes regenerate via `regen-crds` → `regen-api-docs`.
4. **Docs** — CHANGELOG (`**Author:**`) + affected `docs/src/`; `sync-docs` clean.

ADRs and CALM diagrams are first-class deliverables, equal to code and tests.
The full rule lives in `.claude/rules/architecture-driven-development.md` and is
referenced as the governing methodology from `.claude/CLAUDE.md`. ADD applies to
new CRDs/CRD-field contract changes, controllers/reconcilers/binaries, changes
to the CAPI interaction, deploy/admission/GitOps topology, and cross-cutting
security/RBAC/scheduling concerns. Typos, isolated bug fixes, and behavior-
preserving refactors remain TDD-only. **When unsure, write the ADR.**

## Consequences

- **Easier:** architectural intent is recorded once, at decision time, with the
  alternatives weighed — auditable and traceable, which suits the regulated
  context. New contributors read `docs/adr/` instead of re-deriving from diffs.
- **Harder / slower:** a modest per-change overhead for significant work (write
  the ADR, touch CALM). Mitigated by scoping ADD to *architecturally
  significant* changes and keeping ADRs short.
- **Ruled out:** silently making a significant decision in code-only form. If
  it's worth a "why A over B," it gets an ADR.
- **Retroactive ADRs:** existing significant decisions (RBAC anti-escalation,
  embedded-metadata policy, child-cluster routing, `release:published` docs
  trigger) may be back-filled as ADRs over time; not required immediately.
- **CALM impact:** **none.** This is a process decision, not a change to the
  running system's topology, so the CALM model is unchanged. (This ADR is itself
  an instance of the "process-only → no CALM" rule it establishes.)
