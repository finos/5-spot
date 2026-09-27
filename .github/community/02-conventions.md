<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 02 — Coding conventions

> Binding statements of these rules live in `.claude/CLAUDE.md` and
> `.claude/rules/`; this doc is the working digest a contributor (or a coding
> session) should keep open. Where the two disagree, the rule file wins.

## Code style

- `rustfmt` defaults; `clippy` clean with `-D warnings`. The `cargo-quality`
  gate (fmt + clippy + test) runs at the end of every task touching Rust —
  all three must pass before the work is done.
- **Early returns / guard clauses.** Handle edge cases first and return;
  as few `else` blocks as possible, happy path at the bottom.
- **No magic numbers.** Any numeric literal other than `0` or `1` is a named
  constant (`src/constants.rs` for the shared ones — timing, HTTP statuses,
  labels, conditions).
- **No repeated string literals.** A string appearing twice, or anywhere in
  an API contract, becomes a global constant.
- All public functions and types carry rustdoc with `# Arguments` and
  `# Errors` sections.

## Errors

- Library code: typed errors via `thiserror`. Binaries: `anyhow::Result`.
- **Never `unwrap()` in production code** — `?` or explicit handling.
- **Never ignore errors in finalizers**; a swallowed finalizer error blocks
  deletion forever.

## Logging

- `tracing` everywhere — never `println!`, never `log`.
- Every reconcile opens a span carrying the resource name and namespace; log
  reconciliation start and end.
- Level guidance: `error!` needs an operator; `warn!` recoverable, will
  retry; `info!` state transitions; `debug!` dev verbosity; `trace!`
  per-step detail.

## Reconciliation pattern

- **Event-driven, never polling.** `Controller::new(api, …)` with watches —
  including the dynamic watch on spot-schedule provider CRs and
  `Controller::reconcile_on` for child-cluster Node events. A timer loop
  around `api.list()` is always wrong.
- Finalizers for cleanup of anything external; `ownerReferences`
  (`controller: true`) on every child object so deletion cascades.
- Idempotent loops: patch, don't replace; expect and absorb 409s.
- Exponential backoff on errors; every returned `Action` uses the named
  requeue constants, never a bare number.
- Every k8s API call has timeout and retry logic.
- **Stateless**: nothing outside Kubernetes objects survives a restart.
- Status updates follow Kubernetes conventions: `metav1.Condition`-shaped
  conditions (`type`/`status`/`reason`/`message`/`lastTransitionTime`) and
  `observedGeneration` set from `metadata.generation`.

## CRD authoring

- **`src/crd.rs` is the source of truth.** Never hand-edit
  `deploy/crds/*.yaml`.
- The regeneration order is fixed: `regen-crds` skill (`make crds`) → update
  `examples/` → `regen-api-docs` (`make crddoc`) **last**.
- Before debugging any "field doesn't persist" weirdness against a cluster,
  verify deployed CRDs match the code (`verify-crd-sync` skill) — the API
  server happily returns 200 while dropping unknown fields.
- Doc comments in `src/crd.rs` land in the generated YAML, so prose changes
  there also require a regeneration.
- Mermaid/CALM labels: `"`, `<`, `>` break rendering — escape as `#quot;`,
  `#lt;`, `#gt;` in `architecture.json`, and keep `;` out of hand-written
  labels.

## Testing

- **TDD: the failing test is written first** (`tdd-workflow` skill,
  RED → GREEN → REFACTOR).
- **Unit tests live in separate `_tests.rs` files — never an inline
  `mod tests { … }` body.** No exceptions for any target: library modules,
  `src/bin/*.rs`, submodules alike. The source file carries only:

  ```rust
  #[cfg(test)]
  #[path = "foo_tests.rs"]
  mod tests;
  ```

  and `src/foo_tests.rs` holds
  `#[cfg(test)] mod tests { use super::super::*; … }`.
- Coverage bar: **every function, public and private**, with happy-path,
  negative, and error-path cases. Adding, changing, or deleting a function
  means adding, changing, or deleting its tests in the same change.
- Integration tests in `tests/` mock external services (k0smotron, CAPI) and
  exercise failure paths, finalizers, and create → update → delete flows.
  Tests requiring a real docker daemon stay in CI.
- Arrange-Act-Assert; table-driven where a reconciler has many state
  transitions.

## Searching the tree

- `ripgrep` first, always: `rg <pattern>`, and `rg -trs <pattern>` to search
  only Rust files.
- `openwolf find <name>` gives a ranked shortlist for symbols/files.

## Documentation discipline

- Docs ship **in the same change** as the code — never a follow-up PR.
- Every change gets a `.claude/CHANGELOG.md` entry with a mandatory
  `**Author:**` line and an impact checklist.
- If the work advanced a roadmap item: update the detail doc here **and** the
  `ROADMAPS.md` status row in the same commit, auditing the rest of the doc
  against the tree while in it.
- ADR index (`docs/adr/README.md`) stays current; superseded ADRs are marked,
  never rewritten.

## Commits

- Conventional commits: `feat:`, `fix:`, `docs:`, `chore:`, `ci:`, … with a
  scope where it helps (`chore(deps): …`).
- Every commit is **signed off and GPG-signed**: `git commit -s -S`.

## Public-repo sweeps (before finishing any task)

Two greps over the staged diff, one command each — the full commands and
placeholder tables live in the rules:

- real infrastructure / internal references:
  `.claude/rules/no-real-infrastructure.md`;
- home directories / PII: `.claude/rules/no-pii.md`.

If unsure whether a value is real, assume it is and replace it.
