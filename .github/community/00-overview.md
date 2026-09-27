<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 00 — Roadmap overview

> **Read this first.** All other roadmap documents assume the context here.

## What 5-Spot is

A cloud-native Kubernetes controller for **time-based scheduling of physical
machines** in k0smotron child clusters. Users create `ScheduledMachine` CRs
(`5spot.finos.org/v1beta1`); the controller adds and removes the referenced
CAPI `Machine` from the cluster according to a schedule, with timezone
support.

Whether a machine should be active right now is **not decided inline**: since
ADR 0009, `spec.schedule` is a required reference to an external
**spot-schedule provider** CR in the `spotschedules.5spot.finos.org` API
group, resolved through a duck-typed `status.active` contract and driven by an
event-driven dynamic watch. The former inline scheduler survives as the
first-party `TimeBasedSpotSchedule` provider, next to
`CapitalMarketsSchedule`; third parties can bring their own CRD that satisfies
the same contract.

## What's already done — the `v0.3.0` baseline

The tree today (crate `five_spot` 0.3.0) already ships:

- **`ScheduledMachine` at `v1beta1`** with the provider-reference activation
  model (ADR 0006/0009) and dynamic, discovery-based CAPI `Machine` API
  version handling — no hardcoded CAPI version.
- **Two first-party providers** (`spot-schedule-time-based`,
  `spot-schedule-capital-markets`), shipped in the main image as their own
  binaries.
- **Per-node Kata containerd config delivery** (`spec.kata`): the controller
  only *resolves* config (ADR 0002); the privileged
  `5spot-kata-config-agent` DaemonSet writes the drop-in and restarts k0s via
  `nsenter` (ADR 0003), to the **fixed** path
  `/etc/k0s/containerd.d/kata.toml` — the user-configurable destination was
  removed as a node-root-via-CRD vector (ADR 0005), with a deny-by-default
  `ValidatingAdmissionPolicy` fencing the two privileged agents (ADR 0004).
- **Supply-chain gates**: auto-VEX presubmission with a byte-exact CI diff
  gate (ADR 0008), digest-pinned base images on literal `FROM` lines
  (ADR 0010), signed commits, DCO.
- **ADRs 0001–0010** in `docs/adr/` and a FINOS CALM model in
  `docs/architecture/calm/architecture.json` with generated diagrams.

The full record of how the tree got here is roadmap 03 (the `v0.2.2` → `main`
release diff) and, for anything after 2026-07-22, `docs/adr/` plus
`.claude/CHANGELOG.md`.

## Reading order

| # | Document | What it is |
|---|---|---|
| 00 | This overview | Context every other doc assumes |
| [01](01-decisions.md) | Locked design decisions | Quick-reference digest; `docs/adr/` is canonical |
| [02](02-conventions.md) | Coding conventions | How code in this tree is written and tested |
| [03](03-release-diff-v0-2-2-to-main.md) | Release diff `v0.2.2` → `main` | Point-in-time record of the two big feature arcs |
| [04](04-dependency-internalization-matrix.md) | Dependency internalization matrix | Every direct dep, its used surface, internalization verdict |

## How to use these docs with Claude Code

Each detail doc is self-contained enough to be the active context for a
session. Suggested workflow:

1. Open the roadmap doc you're working on, plus `01-decisions.md` and
   `02-conventions.md`.
2. Treat `src/crd.rs` as the source of truth for type shapes — never invent
   CRD fields on the fly; if one is needed, change `src/crd.rs` first and
   regenerate (`regen-crds` skill, then `regen-api-docs` last).
3. Architecturally significant work follows **ADD**:
   `ADR → CALM → TDD → implement → docs`, in that order
   (`.claude/rules/architecture-driven-development.md`). A roadmap entry says
   *what* and *why*; it never substitutes for the ADR on *how*.
4. When a roadmap item's state changes, update the doc **and** the
   `ROADMAPS.md` status row in the same commit.

## Non-negotiables (the principles)

These are locked. If a tradeoff seems to argue against one of these, find a
different tradeoff — do not relax the principle.

1. **ADR before code.** Any architecturally significant change is recorded in
   `docs/adr/NNNN-title.md` and modeled in CALM *before* implementation
   (ADR 0001). Reversing a decision means a superseding ADR, never a quiet
   edit.
2. **Event-driven, never polling.** Controllers react through the Kubernetes
   watch API — including the dynamic watch on provider CRs. A `sleep`-based
   loop is always the wrong shape.
3. **`src/crd.rs` is the source of truth.** `deploy/crds/*.yaml` is generated
   output and is never hand-edited.
4. **Activation is a provider contract.** `spec.schedule` references a
   `spotschedules.5spot.finos.org` CR; there is no inline schedule window
   (ADR 0009). Reopening that needs a superseding ADR.
5. **No node-root via CRD.** The Kata drop-in path is a compile-time constant
   (ADR 0005); nothing user-configurable may reach the host filesystem.
6. **Controllers are stateless.** All state lives in Kubernetes objects; a
   restarted controller must rebuild its world from watches alone.
7. **Tests first, in `_tests.rs` files.** TDD is mandatory; every function —
   public and private — carries happy-path, negative, and error-path tests.
8. **This repo is public.** No real infrastructure identifiers
   (`.claude/rules/no-real-infrastructure.md`), no home directories or PII
   (`.claude/rules/no-pii.md`), ever — `example.com` / RFC 5737 placeholders
   only.

## Repository layout (actual)

```
5-spot/
├── Cargo.toml                   # single crate `five_spot`, several [[bin]] targets
├── ROADMAPS.md                  # status board — one row per roadmap
├── .github/community/           # ← you are here (roadmap docs)
├── docs/
│   ├── adr/                     # ADRs, NNNN-title.md — the decision log
│   ├── architecture/calm/       # FINOS CALM model + generated diagrams
│   └── src/                     # docs site, incl. security/threat-model.md
├── src/
│   ├── main.rs                  # controller entry; health/metrics servers
│   ├── crd.rs                   # ScheduledMachine + spotschedule types — source of truth
│   ├── constants.rs             # named constants (timing, HTTP statuses, labels)
│   ├── reconcilers/             # scheduled_machine, spot_schedule(_watch), child_*
│   ├── providers/               # time_based, capital_markets provider logic
│   └── bin/                     # crdgen, crddoc, agents, providers, auto-vex tools
├── deploy/
│   ├── crds/                    # generated via `make crds` — never hand-edited
│   └── admission/…              # ValidatingAdmissionPolicies (ADR 0004)
├── examples/                    # every CRD wired together; kept in sync with crd.rs
└── .vex/                        # VEX statements + auto-VEX snapshots (ADR 0008)
```

## Pre-flight checks before starting any work

Run these and make sure they all pass:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
make crds            # regenerated deploy/crds/ must leave `git diff` clean
make calm-validate
```

If any of those fail in `main`, fix that before starting new work.

## Versioning and stability

- `ScheduledMachine`: `v1beta1` (the `v1alpha1` → `v1beta1` move was breaking,
  made pre-release — roadmap 03).
- Provider CRDs (`spotschedules.5spot.finos.org`): `v1alpha1`.
- Evolution policy is ADR 0007: multiple served versions with
  `conversion.strategy: None` and **additive-only** fields; the first true
  breaking change triggers a conversion-webhook ADR before it lands.
