<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Spot Schedule Provider Contract

> **Status:** Phase 1 — the API types exist (`ScheduledMachine.spec.spotSchedule`
> on `v1beta1`, the `CapitalMarketsSchedule` reference provider CRD), but the
> controller-side **resolver and watch** land in later roadmap phases. The
> contract is **Accepted** in [ADR 0006](https://github.com/finos/5-spot). This
> page is the authoritative specification a provider author implements against.

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

A `ScheduledMachine` opts in by referencing a provider object in **its own
namespace**:

```yaml
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: trading-floor-rack-7
  namespace: capital-markets
spec:
  # spec.schedule is optional when spotSchedule is set (and vice versa);
  # at least one is required.
  spotSchedule:
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
| `conditions[type=Ready]` | recommended | condition | Provider health. `Ready=False` (or absent) ⇒ 5-Spot treats the reference as **unresolved**, *not* inactive (see [Unresolved behavior](#unresolved-behavior)). |
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

> **TODO (Phase 2/6):** finalize the exact `conditions` schema 5-Spot keys off
> (reason vocabulary, whether `observedGeneration` staleness is enforced).

## Composition with `spec.schedule`

`spec.schedule` and `spec.spotSchedule` are independent; **at least one** must
be set. When **both** are set, the machine is active **only if both agree** —
logical **AND** (e.g. "the market is open *and* it is 09:00–17:00 local").
`spec.killSwitch` always wins. Precedence, highest first:

```
killSwitch  >  (schedule AND spotSchedule)  >  schedule-only / spotSchedule-only
```

## Unresolved behavior

A reference is **Unresolved** when the provider CRD is not installed, the named
object is absent, it exposes no `status.active`, or its `Ready` condition is
`False`/missing. On Unresolved, 5-Spot:

- sets a `SpotScheduleResolved=False` condition on the `ScheduledMachine` with
  a precise reason (`ProviderCRDNotInstalled`, `ProviderNotFound`,
  `StatusActiveMissing`, `ProviderNotReady`),
- emits `fivespot_spot_schedule_resolution_errors_total`, and
- **holds the last known resolved state** — a running machine is **not** torn
  down because its provider briefly went unreadable. If the reference has
  **never** resolved, the fail-safe default is **inactive**.

A provider that misbehaves (or is compromised) can only flap the machines that
**reference it** — the same blast radius as editing `spec.schedule.enabled` —
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

## Reference provider: `CapitalMarketsSchedule`

The in-repo reference implementation (roadmap Phase 5) reconciles a declarative
exchange calendar (sessions, statutory holidays, early closes, timezone) into
`status.active`, requeuing at the next session/holiday boundary (event-driven —
a single timed requeue at the calendar transition, not a poll loop).

> **TODO (Phase 5/6):** link the `CapitalMarketsSchedule` API reference, the
> guide, and a worked minimal "hello-world" provider example here.

## See also

- [ADR 0006 — Pluggable spot-schedule provider contract](https://github.com/finos/5-spot)
- [ADR 0007 — CRD multi-version + conversion](https://github.com/finos/5-spot)
- [`ScheduledMachine` API reference](api.md)
- [Threat model](../security/threat-model.md)
