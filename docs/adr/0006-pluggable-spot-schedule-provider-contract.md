<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0006 — Pluggable spot-schedule provider contract via `spec.spotSchedule` and the `spotschedules.5spot.finos.org` API group

- **Status:** Accepted
- **Date:** 2026-06-13
- **Deciders:** Erick Bourgeois, 5-Spot team
- **Supersedes:** —
- **Related:** ADR [0007](./0007-crd-multi-version-and-conversion.md) (the
  multi-version/conversion story this contract depends on);
  `ScheduleSpec` + `evaluate_schedule` (`src/crd.rs`, `src/reconcilers/helpers.rs`);
  `ObjectReference`-style refs already in `src/crd.rs`;
  the reclaim-agent Node-watch (`rel-controller-workload-kube-api`) as the
  event-driven watch shape to mirror; contract page
  `docs/src/reference/spot-schedule-contract.md`.

## Context

A `ScheduledMachine` decides "should this machine exist right now?" solely from
the inline `spec.schedule` (`daysOfWeek` / `hoursOfDay` / `timezone` /
`enabled`), evaluated by `evaluate_schedule()` in `src/reconcilers/helpers.rs`.
That model is **closed**: every new activation semantic — exchange calendars,
statutory holidays and early closes, change-freeze windows, metric-driven
scale, a manual trading-desk drain — would force a new field on 5-Spot's CRD
and new branches in its reconciler. The motivating case is capital markets:
trading-floor compute must follow the *exchange session calendar*, which no
`daysOfWeek`/`hoursOfDay` expression can encode.

**Options weighed:**

1. **Grow `spec.schedule`** with calendar/holiday/PromQL sub-objects. Rejected:
   unbounded surface area, 5-Spot would own every scheduling dialect forever,
   and operators still couldn't express semantics we didn't anticipate.
2. **A plugin/script hook** (WASM, embedded expression language) evaluated
   in-process. Rejected: a sandbox and execution-safety burden inside a
   privileged controller, and still not a Kubernetes-native contract.
3. **A duck-typed external provider resource** that owns the active/inactive
   decision; 5-Spot consumes it generically. **Chosen.** This is the exact
   shape Cluster API uses for `infrastructureRef` / `bootstrap.configRef`
   (a contract over `apiVersion`/`kind`/`name` reading a well-known status
   field) — a pattern 5-Spot already participates in from the *other* side, so
   it is idiomatic here and familiar to operators.

The cost of option 3 is the one recorded below: a provider is an **untrusted
input** that can start and stop the machines that reference it, and an
unresolved/late/unhealthy provider must never flap machines.

## Decision

### 1. `spec.spotSchedule` reference on `ScheduledMachine`

`src/crd.rs` is the source of truth; CRD YAML and API docs are regenerated
(`regen-crds` → `regen-api-docs`), never hand-edited.

```rust
// on ScheduledMachineSpec — schedule becomes optional (see §3)
#[serde(skip_serializing_if = "Option::is_none")]
pub spot_schedule: Option<SpotScheduleRef>,

pub struct SpotScheduleRef {
    pub api_version: String, // "spotschedules.5spot.finos.org/<version>"
    pub kind: String,        // e.g. "CapitalMarketsSchedule"
    pub name: String,        // object in the SAME namespace as the SM
}
```

`SpotScheduleRef` keeps the existing ref conventions (`deny_unknown_fields`,
camelCase, schemars bounds). The referenced object **must live in the
`ScheduledMachine`'s own namespace** — no `namespace` field, no cross-namespace
references in this version.

### 2. The provider group and the duck-typed contract

The provider API group is **`spotschedules.5spot.finos.org`** — a sub-group of
the existing `5spot.finos.org` group. A CEL `XValidation` on
`spotSchedule.apiVersion` pins the **group** to exactly
`spotschedules.5spot.finos.org` (any served version is accepted, per ADR 0007);
references to any other group are rejected at admission.

5-Spot reads a **duck-typed `status`** and never the provider `spec`:

| Provider `status` field | Required | Meaning to 5-Spot |
|---|---|---|
| `active` (bool) | **yes** | the single source of truth: the machine should be up |
| `conditions[type=Ready]` | recommended | provider health; `Ready=False` ⇒ **unresolved**, *not* inactive |
| `observedGeneration` (int) | recommended | staleness detection |
| `lastTransitionTime` (string) | recommended | observability / transition metrics |

Providers implement `spec` however they like (exchange calendars, PromQL,
cron, a plain `enabled` toggle). The contract is published in
`docs/src/reference/spot-schedule-contract.md`.

### 3. Composition with `spec.schedule` (AND), and `killSwitch` precedence

`spec.schedule` becomes `Option<ScheduleSpec>`. A CEL `XValidation` requires
**at least one** of `schedule` / `spotSchedule` (existing SMs all set
`schedule`, so this is non-breaking). When **both** are present the machine is
active **iff the inline time window AND the provider both say active** — this
supports "the market is open **and** only 09:00–17:00 local". `killSwitch`
continues to override everything: `killSwitch=true` ⇒ inactive regardless of
schedule or provider. Precedence, highest first:

```
killSwitch  >  (schedule AND spotSchedule)  >  schedule-only / spotSchedule-only
```

### 4. Unresolved references never flap machines

A reference is **Unresolved** when the provider CRD is not installed, the named
object is absent, it has no `status.active`, or it carries a `Ready` condition
whose status is **not `True`**. `Ready` is *recommended, not required*: a
provider that **omits** it has its `status.active` taken as authoritative —
only a *present, non-`True`* `Ready` marks the reference unresolved (this
resolves the §2 table's "`Ready=False` ⇒ unresolved" reading: absent ⇒
authoritative). On Unresolved, 5-Spot:

- sets a `SpotScheduleResolved=False` condition on the `ScheduledMachine` with
  a precise reason (`ProviderCRDNotInstalled`, `ProviderNotFound`,
  `StatusActiveMissing`, `ProviderNotReady`),
- increments `fivespot_spot_schedule_resolution_errors_total`, and
- **holds the last known resolved state** — it does **not** tear down a running
  machine because its provider went briefly unreadable. If the reference has
  **never** resolved, the fail-safe default is **inactive** (no machine is
  created on the strength of a reference we cannot read).

This "hold-last-state, fail-inactive-when-never-resolved" rule is the sharpest
edge of the contract and is tested on both branches.

### 5. Event-driven dynamic watch (no polling)

The controller maintains an in-memory **reverse index**
`(group, version-agnostic kind, namespace, name) → {ScheduledMachine keys}`,
updated on SM apply/delete. For each distinct **GVK** referenced, it lazily
starts a dynamic watch (`Api<DynamicObject>` resolved via `kube::discovery`)
and maps provider events back through the index to `reconcile_on` the affected
`ScheduledMachine`s. Streams are stopped when the last referencing SM goes
away. The index is **rebuilt from the SM list on controller start** — nothing
is stored outside Kubernetes (5-Spot stays stateless). This mirrors the
existing reclaim-agent Node-watch and honours the project's
event-driven-not-polling rule; provider `status.active` transitions drive SM
reconciles at watch latency, matching the inline-schedule path.

Hard edges handled (and tested): provider CRD installed *after* SMs reference
it (discovery retry with backoff, surfaced as `Unresolved` meanwhile), provider
CRD deleted while watched (stream error → re-resolve), controller restart
(index rebuilt on boot).

### 6. Security boundary — providers are untrusted inputs

- Controller RBAC gains **`get;list;watch` only** on
  `spotschedules.5spot.finos.org` `*` — **no write** to any provider resource.
- A reference provider's own controller gets `update;patch` on **its own**
  kind's `/status` subresource only (a separate Role, not 5-Spot's ClusterRole).
- A compromised or buggy provider can flap the machines that *reference* it —
  the **same blast radius as a malicious edit to `spec.schedule.enabled`**, and
  bounded to SMs that opted in by naming it. Mitigations: same-namespace-only
  references, RBAC on the provider CRDs, audit via the `SpotScheduleResolved`
  condition + transition metrics, and a transition-rate alert. This actor and
  these mitigations are added to `docs/src/security/threat-model.md`.

## Consequences

- **Easier:** activation semantics evolve **out of tree** — capital-markets
  calendars, PromQL emitters, change-freeze gates ship as provider CRDs without
  touching 5-Spot's CRD or reconciler; 5-Spot gains **no** new write ability
  (read-only on providers); the inline `spec.schedule` stays the simple path.
- **Harder / new burden:** a dynamic multi-GVK watch manager + reverse index to
  build and test; a published, versioned **public contract** (ADR 0007) we must
  not break; provider-flapping is a new failure mode requiring debounce-style
  thinking (a per-SM `minimumStateDuration` is noted as possible future work,
  not adopted here).
- **Operator contract:** the referenced provider object **must pre-exist** in
  the SM's namespace and report `status.active`; absence/unreadiness is a
  loud `SpotScheduleResolved=False` status, never a silent machine teardown.
- **Ruled out (this version):** cross-namespace / cross-cluster references;
  multiple `spotSchedule` refs per SM (no AND/OR ref-lists); 5-Spot ever
  writing provider status; a provider SDK/conformance kit; the Prometheus
  emitter implementation itself (a planned *consumer* of this contract).
- **CALM impact:** **updated.** New external node
  `service-spot-schedule-provider`; a `data-asset-spot-schedule-cr`; a
  controller→provider **watch** relationship
  (`rel-controller-spot-schedule-watch`, read-only, group-pinned, least
  privilege control); and a flow `flow-spot-schedule-activation`
  (provider `status.active` transition → SM reconcile → activate/deactivate via
  the existing schedule flows). `make calm-validate` + `make calm-diagrams`
  before implementation.
