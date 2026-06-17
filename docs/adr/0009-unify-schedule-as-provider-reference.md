<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0009 — Unify activation under `spec.schedule` as a provider reference; ship `TimeBasedSpotSchedule` as the core provider

- **Status:** Accepted
- **Date:** 2026-06-16
- **Deciders:** Erick Bourgeois
- **Supersedes:** Amends ADR [0006](./0006-pluggable-spot-schedule-provider-contract.md) §1 and §3; amends ADR [0007](./0007-crd-multi-version-and-conversion.md) (collapses `ScheduledMachine` to a single served version, pre-release)
- **Related:** `ScheduleSpec` / `evaluate_schedule` (`src/crd.rs`, `src/reconcilers/helpers.rs`); `CapitalMarketsSchedule` provider (`src/providers/capital_markets.rs`); spot-schedule contract (`docs/src/reference/spot-schedule-contract.md`)

## Context

ADR 0006 introduced a pluggable, duck-typed **spot-schedule provider** contract
(`spotschedules.5spot.finos.org`, consumed via `status.active`) alongside the
original inline `spec.schedule` time window. That left a `ScheduledMachine` with
**two different activation mechanisms**:

- `spec.schedule: Option<ScheduleSpec>` — an inline day/hour/timezone window
  evaluated **inside** the 5-Spot reconciler (`evaluate_schedule`), and
- `spec.spotSchedule: Option<SpotScheduleRef>` — a reference to an external
  provider object, resolved via the duck-typed contract,

composed with logical AND and gated by a CEL "at least one of" rule. The inline
path is privileged (special-cased in the reconciler) while every other
activation semantic is a first-class provider. That asymmetry is the problem:

- two code paths, two composition branches, two status surfaces, and a
  cross-field CEL invariant to maintain;
- the time-based scheduler — the most common case — is the *only* semantic that
  is **not** expressed through the public provider contract, so the contract is
  never dogfooded by the project's own default behaviour;
- operators learn two mental models (inline window vs. provider reference) for
  one question: "what decides if this machine is up?"

Nothing in the `5spot.finos.org` group has shipped a stable release, so the CRD
contract is free to change without a conversion/migration burden.

**Options weighed:**

1. **Keep both mechanisms** (status quo). Rejected: permanent two-path
   complexity and an un-dogfooded contract.
2. **`spec.schedule` as a tagged union** (`inline timeBased` *or* `providerRef`),
   inline evaluated in-process. Rejected: still two evaluation paths and a
   privileged inline form; the union keeps the asymmetry, just relocated.
3. **`spec.schedule` becomes a pure provider reference; the time window becomes a
   provider.** **Chosen.** Every machine references exactly one provider object;
   the built-in time scheduler is reified as a real provider CRD
   (`TimeBasedSpotSchedule`) with its own controller, exactly like
   `CapitalMarketsSchedule`. One contract, one code path, one mental model — and
   the core behaviour proves the contract.

## Decision

### 1. `spec.schedule` is a required provider reference

`spec.schedule` changes from `Option<ScheduleSpec>` to a **required**
`SpotScheduleRef` (`apiVersion` / `kind` / `name`, group pinned to
`spotschedules.5spot.finos.org`, same-namespace only — the shape ADR 0006 §1
defined for `spotSchedule`). `spec.spotSchedule` is **removed**; it has merged
into `spec.schedule`. There is no inline time window on `ScheduledMachine` any
more.

Consequently:

- The "at least one of `schedule`/`spotSchedule`" CEL rule (ADR 0006 §3) is
  replaced by `schedule` simply being a required field.
- Composition collapses: `should_be_active` is now **the single provider
  verdict** (hold-last-state when unresolved, fail-inactive when never resolved
  — ADR 0006 §4 unchanged), with `killSwitch` still terminal/overriding. The
  two-axis `schedule AND spotSchedule` composition is gone.
- The administrative master switch that was `spec.schedule.enabled` is promoted
  to a **new top-level `spec.enabled: bool`** (default `true`) on
  `ScheduledMachine`. This deliberately decouples "is this machine
  administratively enabled?" (an SM-level operator decision, the `Disabled`
  phase) from "does the provider say active right now?" (the provider's
  `status.active`) — two concerns the inline model conflated in one
  `schedule.enabled` flag. `is_enabled()` reads `spec.enabled`.
- **Emergency-reclaim loop-breaker repoints to `spec.enabled`.** The
  process-match reclaim flow previously patched `spec.schedule.enabled=false`
  (then `Disabled`) to stop the next schedule window re-adding a reclaimed node.
  It now patches **`spec.enabled=false`**. Patching the *provider* object was
  rejected: a provider can be referenced by many SMs, so disabling it would flap
  every machine that shares it — the loop-breaker must stay SM-scoped. The 7-step
  replay-safe ordering (write-disable before annotation-clear) is unchanged.

### 2. `TimeBasedSpotSchedule` — the core provider, shipped in-repo

A new provider CRD `TimeBasedSpotSchedule` in `spotschedules.5spot.finos.org`,
mirroring `CapitalMarketsSchedule`:

```rust
// src/crd.rs — spec carries today's inline window fields
pub struct TimeBasedSpotScheduleSpec {
    pub days_of_week: Vec<String>,  // "mon-fri"
    pub hours_of_day: Vec<String>,  // "9-17"
    pub timezone: String,           // IANA, default UTC
    pub enabled: bool,              // master on/off, default true
}
// status satisfies the ADR 0006 duck-typed contract:
//   active, conditions[Ready], observedGeneration,
//   lastTransitionTime, nextTransitionTime
```

A standalone controller (`src/providers/time_based.rs` +
`src/bin/spot-schedule-time-based`) evaluates the window in the configured
timezone — the logic lifted from `evaluate_schedule` — publishes `status.active`,
and requeues once at the next day/hour boundary (event-driven, no poll), exactly
as the `CapitalMarketsSchedule` controller does. The day/hour parsing
(`parse_day_ranges` / `parse_hour_ranges`) is shared.

### 3. `ScheduledMachine` collapses to a single served version (pre-release)

ADR 0007 kept `5spot.finos.org/v1alpha1` (frozen, inline-schedule-required)
served beside `v1beta1` under `conversion: None`, relying on `v1beta1` being a
**superset** of `v1alpha1`. The new shape breaks that superset relationship
(inline `schedule` fields no longer exist on the SM). Because nothing has
released, we **drop the `v1alpha1` `ScheduledMachine` module** and serve a single
`v1beta1` with the new shape. ADR 0007's multi-version + `None`-conversion
machinery remains the project's tool for **post-release** evolution; it is simply
not exercised for this pre-release break. (`TimeBasedSpotSchedule` and
`CapitalMarketsSchedule` remain `spotschedules.5spot.finos.org/v1alpha1`.)

## Consequences

- **Easier:** one activation path and one mental model; the public provider
  contract is dogfooded by 5-Spot's own default scheduler; the reconciler loses
  the inline-evaluate branch, the AND-composition, and the cross-field CEL rule.
- **Harder / new burden:** the simplest "weekdays 09:00–17:00" machine now needs
  **two objects** — a `ScheduledMachine` and the `TimeBasedSpotSchedule` it
  references (the cost the user accepted for uniformity). A new provider CRD,
  controller binary, RBAC Role, and Deployment are added and must be installed
  for the default behaviour to work at all.
- **Breaking (intended):** existing `spec.schedule` inline manifests and
  `spec.spotSchedule` manifests no longer apply; `5spot.finos.org/v1alpha1`
  `ScheduledMachine` is removed. Acceptable pre-release; called out loudly in the
  CHANGELOG and docs.
- **Unchanged:** the duck-typed `status.active` contract, the Unresolved /
  hold-last-state / fail-inactive rules (ADR 0006 §4), the event-driven dynamic
  watch (§5), and the read-only same-namespace provider security boundary (§6).
- **CALM impact:** **updated.** `TimeBasedSpotSchedule` is added as a shipped
  provider node/data-asset alongside `CapitalMarketsSchedule`; the former inline
  `flow-schedule-evaluation` is folded into `flow-spot-schedule-activation`
  (provider `status.active` → SM reconcile). `make calm-validate` +
  `make calm-diagrams` before implementation.
