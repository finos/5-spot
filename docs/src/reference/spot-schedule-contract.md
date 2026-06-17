<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Spot Schedule Provider Contract

> **Status:** Stable contract, **Accepted** in
> [ADR 0006](https://github.com/finos/5-spot). The `CapitalMarketsSchedule`
> reference provider ships in-repo (see the
> [provider guide](../guides/capital-markets-schedule.md)). This page is the
> authoritative specification a provider author implements against — implement
> the `status` contract below and 5-Spot will consume your provider.

A **spot-schedule provider** is any Kubernetes custom resource in the
`spotschedules.5spot.finos.org` API group that tells 5-Spot whether a
[`ScheduledMachine`](api.md) should be **active** (its machine should exist) or
**inactive** (its machine should be torn down) right now.

5-Spot owns *where the decision lives* (`status.active`); the provider owns
*how the decision is computed* — an exchange calendar, a PromQL expression, a
change-freeze window, or a plain manual toggle. 5-Spot **never reads the
provider `spec`** and **never writes the provider object**.

This is the same duck-typed contract pattern Cluster API uses for
`infrastructureRef` / `bootstrap.configRef`.

## Referencing a provider

Every `ScheduledMachine` references **exactly one** provider object in **its own
namespace** via the required `spec.schedule`:

```yaml
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: trading-floor-rack-7
  namespace: capital-markets
spec:
  # spec.schedule is a required provider reference.
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: CapitalMarketsSchedule
    name: nyse-equities-session     # must live in namespace: capital-markets
```

- `apiVersion` — the group **must** be `spotschedules.5spot.finos.org`
  (CEL-enforced at admission; any *served* version is accepted, see
  [Versioning](#versioning)). Other groups are rejected.
- `kind` — the provider kind (e.g. `CapitalMarketsSchedule`).
- `name` — the provider object, **same namespace** as the `ScheduledMachine`.
  Cross-namespace references are not supported.

## The status contract a provider must satisfy

5-Spot reads only the provider's `status`:

| `status` field | Required | Type | Meaning |
|---|:--:|---|---|
| `active` | **yes** | bool | **The decision.** `true` ⇒ the referencing machine should be up; `false` ⇒ it should be down. |
| `conditions[type=Ready]` | recommended | condition | Provider health. A **present** `Ready` whose status is *not* `True` ⇒ 5-Spot treats the reference as **unresolved**, *not* inactive (see [Unresolved behavior](#unresolved-behavior)). An **absent** `Ready` ⇒ `status.active` is taken as authoritative. Set `Ready=True` when your `active` is current, and `Ready=False` to say "don't trust my `active` right now" without flapping the machine. |
| `observedGeneration` | recommended | int64 | The `metadata.generation` the status reflects; lets 5-Spot detect stale status. |
| `lastTransitionTime` | recommended | RFC 3339 string | When `active` last flipped; used for observability and flap detection. |

A minimal conformant `status`:

```yaml
status:
  active: true
  observedGeneration: 4
  lastTransitionTime: "2026-06-13T13:30:00Z"
  conditions:
    - type: Ready
      status: "True"
      reason: SessionOpen
      message: "NYSE equities regular session open"
      lastTransitionTime: "2026-06-13T13:30:00Z"
      observedGeneration: 4
```

5-Spot keys off **only** `status.active` and the `Ready` condition's `status`
field (`"True"` / `"False"` / `"Unknown"`). The condition `reason` / `message`
are free-form and surfaced for observability; pick any CamelCase `reason`. The
`observedGeneration` is recommended for your own staleness detection but 5-Spot
does not currently reject stale status on it.

## The provider verdict, `spec.enabled`, and `killSwitch`

There is no composition: the single referenced provider's `status.active` **is**
the activation decision. Two switches sit above it. `spec.enabled` (default
`true`) is the administrative master switch — setting it `false` holds the
machine in the `Disabled` lifecycle phase regardless of what the provider says.
`spec.killSwitch` is a terminal teardown and always wins. Precedence, highest
first:

```
killSwitch  >  spec.enabled=false (Disabled)  >  provider status.active
```

## Unresolved behavior

A reference is **Unresolved** when the provider CRD is not installed, the named
object is absent, it exposes no `status.active`, or it carries a `Ready`
condition whose status is *not* `True` (an absent `Ready` is **not**
unresolved — `active` is then authoritative). On Unresolved, 5-Spot:

- sets a `SpotScheduleResolved=False` condition on the `ScheduledMachine` with
  a precise reason (`ProviderCRDNotInstalled`, `ProviderNotFound`,
  `StatusActiveMissing`, `ProviderNotReady`),
- emits `fivespot_spot_schedule_resolution_errors_total`, and
- **holds the last known resolved state** — a running machine is **not** torn
  down because its provider briefly went unreadable. If the reference has
  **never** resolved, the fail-safe default is **inactive**.

A provider that misbehaves (or is compromised) can only flap the machines that
**reference it** — the same blast radius as editing `spec.enabled` —
and only within its own namespace. See the
[threat model](../security/threat-model.md).

## Versioning

Per [ADR 0007](https://github.com/finos/5-spot), provider CRDs are
**multi-version-capable**: a CRD may serve several versions
(`v1alpha1`, later `v1beta1`/`v1`) with exactly one storage version and
`conversion.strategy: None`. 5-Spot resolves providers by **group + kind**, not
a pinned `apiVersion`, so any *served* version of a referenced provider works.

Contract evolution is **additive-only** while conversion is `None`: new
**optional** status fields may be added; an existing status field is never
renamed, retyped, or removed without a superseding ADR (which would introduce a
conversion webhook). Providers should therefore tolerate unknown fields and not
rely on 5-Spot reading anything beyond the table above.

## Reference providers

The **default, first-party** provider is `TimeBasedSpotSchedule` — the reified
former inline schedule, computing `status.active` from `daysOfWeek` /
`hoursOfDay` / `timezone` windows. It is the provider most `ScheduledMachine`s
reference. See the
[TimeBasedSpotSchedule provider guide](../guides/time-based-schedule.md).

The in-repo `CapitalMarketsSchedule` reference implementation reconciles a
declarative exchange calendar (sessions, statutory holidays, early closes,
timezone) into `status.active`, requeuing at the next session/holiday boundary
(event-driven — a single timed requeue at the calendar transition, not a poll
loop). See the
[CapitalMarketsSchedule provider guide](../guides/capital-markets-schedule.md)
for install + authoring, and
[`examples/capitalmarketsschedule.yaml`](https://github.com/finos/5-spot/blob/main/examples/capitalmarketsschedule.yaml).

## Implementing your own provider

The entire contract is `status.active` plus an optional `Ready` condition — any
namespaced CRD in the `spotschedules.5spot.finos.org` group whose controller
writes that status is a valid provider. For a complete, copy-pasteable
walkthrough (CRD, controller, RBAC, deploy, reference, verify) building a minimal
`ManualSchedule` toggle, see the
[**Create Your Own Provider** guide](../guides/create-your-own-provider.md). The
[`CapitalMarketsSchedule`](https://github.com/finos/5-spot/tree/main/src/providers/capital_markets.rs)
controller is the same shape with a calendar instead of a toggle.

## See also

- [Create Your Own Provider](../guides/create-your-own-provider.md) — build a provider step by step
- [TimeBasedSpotSchedule provider guide](../guides/time-based-schedule.md) — the default first-party provider
- [CapitalMarketsSchedule provider guide](../guides/capital-markets-schedule.md) — the reference provider
- [ADR 0006 — Pluggable spot-schedule provider contract](https://github.com/finos/5-spot)
- [ADR 0007 — CRD multi-version + conversion](https://github.com/finos/5-spot)
- [`ScheduledMachine` API reference](api.md)
- [Threat model](../security/threat-model.md)
