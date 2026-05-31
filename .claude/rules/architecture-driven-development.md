# Architecture Driven Development (ADD)

> **ADD is the governing methodology for 5-Spot.** Architecture is designed,
> recorded, and visualized **before** code is written. ADRs and CALM diagrams
> are first-class deliverables — equal in importance to the code and the tests.

ADD layers *on top of* the existing TDD discipline; it does not replace it. The
order is fixed:

```
ADR  →  CALM  →  TDD  →  implement  →  docs
```

## The ADD cycle

For any **architecturally significant** change, complete each step before
starting the next:

### 1. ADR — decide and record (FIRST)

Write or update an Architecture Decision Record in
`docs/adr/NNNN-title.md` (lowercase-hyphen, zero-padded sequential number)
using `docs/adr/template.md`, with the standard sections:

- **Status** — Proposed → Accepted (→ Superseded by NNNN)
- **Context** — the forces, constraints, and the problem being solved
- **Decision** — what we will do, stated plainly
- **Consequences** — trade-offs, follow-ups, what this rules out

ADRs are kept **in the repo** (unlike roadmaps, which live outside it at
`~/dev/roadmaps/`). One decision per ADR. If a change reverses an earlier ADR,
mark the old one *Superseded* and link forward. Keep the index in
`docs/adr/README.md` current.

### 2. CALM — model and visualize

Update the FINOS CALM architecture model
(`docs/architecture/calm/architecture.json`) to reflect the decision: nodes,
relationships, interfaces, controls, and flows. Then:

```sh
make calm-validate     # architecture conforms to the meta-schema (hard gate)
make calm-diagrams     # regenerate Mermaid diagrams into docs/src/architecture/
```

The architecture must be modeled and the diagrams must render cleanly **before**
implementation begins. A change that isn't reflected in CALM isn't designed yet.

> **Process-only decisions** (e.g. adopting a methodology, CI policy) have **no
> CALM impact** — CALM models the running system (controller, `ScheduledMachine`
> CRD, CAPI/provider resources, flows), not the development process. Say so
> explicitly in the ADR's Consequences and skip step 2.

### 3. TDD — red / green / refactor

Only now write code, **tests first**, per the `tdd-workflow` skill: failing test
→ minimum implementation → refactor. Tests go in separate `_tests.rs` files
(`src/foo.rs` → `#[cfg(test)] mod foo_tests;` → `src/foo_tests.rs`). After any
`.rs` change, run the `cargo-quality` skill (fmt + clippy + test — all must pass).

CRD shape changes start in `src/crd.rs` (the source of truth), then regenerate:
`regen-crds` skill (`make crds`) → update `examples/` → `regen-api-docs`
(`make crddoc`, LAST). Never hand-edit `deploy/crds/*.yaml`.

### 4. Docs

Update `.claude/CHANGELOG.md` (with `**Author:**`) and any affected `docs/src/`
pages / examples. Run the `sync-docs` skill to verify docs match the code.

## When does ADD apply?

**Full ADR + CALM** (architecturally significant):

- New CRDs / CRD fields that change a contract, new controllers, reconcilers, or binaries
- Changes to how 5-Spot interacts with CAPI (Machine / bootstrap / infrastructure contract, allowed API groups)
- New deploy / admission / GitOps topology (e.g. ValidatingAdmissionPolicy posture, child-cluster routing)
- Cross-cutting concerns: security boundaries, RBAC posture, failure domains, scheduling semantics
- Any decision where "why A over B" is worth recording

**ADR only, no CALM** (process / policy, no system-topology change):

- Methodology, CI/CD policy, repository conventions

**TDD only** (no ADR/CALM needed):

- Typos, comment/doc tweaks, formatting
- Isolated bug fixes with no architectural impact
- Mechanical refactors that preserve behavior and structure

> When unsure whether a change is "architectural," **write the ADR.** A short,
> slightly-redundant ADR costs little; an undocumented architectural decision
> costs the next person a re-derivation.

## Checklist (paste into the work)

- [ ] ADR written/updated in `docs/adr/NNNN-*.md` (Status/Context/Decision/Consequences); index in `docs/adr/README.md` updated
- [ ] CALM model updated; `make calm-validate` passes; `make calm-diagrams` renders — **or** ADR states "no CALM impact (process-only)"
- [ ] Tests written **first**, then implementation (TDD); CRD changes regenerated via `regen-crds` → `regen-api-docs`
- [ ] `cargo-quality` passes (fmt + clippy + test)
- [ ] CHANGELOG (`**Author:**`) + docs updated; `sync-docs` clean
