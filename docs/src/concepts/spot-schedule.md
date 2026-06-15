<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Spot Schedules (pluggable providers)

A `ScheduledMachine` answers one question every reconcile: *should this machine
exist right now?* The built-in [`spec.schedule`](schedules.md) answers it from a
fixed time window (`daysOfWeek` / `hoursOfDay` / `timezone`). **Spot schedules**
let an *external* resource answer it instead — so activation can follow anything
an operator can model: an exchange calendar, a PromQL signal, a change-freeze
window, or a plain manual toggle.

This is the design recorded in
[ADR 0006](https://github.com/finos/5-spot/blob/main/docs/adr/0006-pluggable-spot-schedule-provider-contract.md).

## Why a duck-typed provider, not more `spec.schedule` fields

Every new activation semantic could be a new field on `spec.schedule`. That path
never ends — 5-Spot would have to own every scheduling dialect (holiday lists,
half-days, PromQL, freeze calendars …) forever, and operators still couldn't
express anything 5-Spot didn't anticipate.

Instead 5-Spot defines **where the decision lives** — a boolean `status.active`
on a provider object — and lets the provider own **how it's computed**. 5-Spot
reads only that boolean (and a recommended `Ready` condition); it never reads the
provider's `spec` and never writes the provider object. This is exactly the
duck-typed contract Cluster API uses for `infrastructureRef` /
`bootstrap.configRef`, which 5-Spot already participates in from the other side.

New activation semantics therefore ship **out of tree** as a provider CRD +
controller, with no change to 5-Spot. The full provider contract is in the
[Spot Schedule Provider Contract](../reference/spot-schedule-contract.md); the
[`CapitalMarketsSchedule`](../guides/capital-markets-schedule.md) provider is the
in-repo reference.

## Referencing a provider

`spec.spotSchedule` is an object reference — `apiVersion` / `kind` / `name` — to
a provider object in the **same namespace** as the `ScheduledMachine`:

```yaml
spec:
  spotSchedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: CapitalMarketsSchedule
    name: nyse-equities
```

The group **must** be `spotschedules.5spot.finos.org` (enforced at the CRD
schema, the admission policy, and at runtime). Cross-namespace references are a
deliberate non-goal — a provider can only influence machines in its own
namespace.

## Composition with `spec.schedule` (AND)

`spec.schedule` and `spec.spotSchedule` are independent, and **at least one**
must be set. When both are set the machine is active **only if both agree** —
logical **AND**:

```
killSwitch  >  (schedule AND spotSchedule)  >  schedule-only / spotSchedule-only
```

That composition expresses "the market is open **and** it is 09:00–17:00 local":
the provider gates the *days the exchange trades*, the inline schedule gates the
*hours the desk staffs*. `spec.killSwitch` always wins.

## Event-driven, fail-safe

5-Spot **watches** referenced providers: a provider's `status.active` flip wakes
the referencing machines at watch latency, not on a polling timer (the dynamic
watch manager runs one watch stream per referenced provider kind). See
[`flow-spot-schedule-activation`](../architecture/flows.md).

A provider that can't be resolved — its CRD isn't installed, the object is gone,
it exposes no `status.active`, or it reports `Ready` ≠ `True` — never flaps a
machine. 5-Spot **holds the last known state** (and fails *inactive* only if the
reference never resolved at all), surfacing a `SpotScheduleResolved=False`
condition and a `fivespot_spot_schedule_resolution_errors_total` metric instead.
A misbehaving provider's blast radius is exactly that of editing
`spec.schedule.enabled` — bounded to the same-namespace machines that named it
(see the [threat model](../security/threat-model.md)).

## See also

- [Spot Schedule Provider Contract](../reference/spot-schedule-contract.md) — implement a provider
- [CapitalMarketsSchedule provider guide](../guides/capital-markets-schedule.md) — the reference provider
- [ScheduledMachine](scheduled-machine.md) — the consuming resource
- [Schedule Configuration](schedules.md) — the inline `spec.schedule` axis
