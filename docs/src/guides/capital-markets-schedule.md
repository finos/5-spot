<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Guide: CapitalMarketsSchedule provider

`CapitalMarketsSchedule` is the **reference spot-schedule provider** (ADR 0006).
It models an exchange calendar — trading sessions, statutory holidays, and
early-close days — and publishes a duck-typed `status.active` that a
[`ScheduledMachine.spec.schedule`](../reference/spot-schedule-contract.md)
consumes. A machine bound to it follows the market: up while the exchange is in
session, down otherwise.

## How it works

The `spot-schedule-capital-markets` controller watches `CapitalMarketsSchedule`
objects and, for each, computes `status.active` from the `spec` calendar in the
configured timezone, then **requeues once at the next calendar boundary** (the
next session open/close, holiday, or early close). It is event-driven — there is
no polling interval, and it makes no network calls: the calendar lives entirely
in `spec`, which operators keep current via GitOps.

Evaluation order each tick:

1. A date listed in `spec.holidays` closes the market for the whole day.
2. Otherwise the instant must fall inside some `spec.sessions[*]` window — a
   `daysOfWeek` **and** `hoursOfDay` match (same range syntax as
   `TimeBasedSpotSchedule.spec`).
3. An entry in `spec.earlyCloses` for that date closes the market after its
   `closeHour` (the market is active *through the end of* `closeHour`).

Transition detection is **hour-granular**, so the requeue lands within an hour
of the true boundary for whole-hour-offset exchange timezones (NYSE, LSE, TSE,
TSX); the `active` value always self-corrects on the next reconcile.

## Install

The `CapitalMarketsSchedule` CRD ships under `deploy/crds/`. Install it and the
provider controller:

```bash
kubectl apply -f deploy/crds/capitalmarketsschedule.yaml
kubectl apply -k deploy/spot-schedule-providers/capital-markets/
```

The provider runs with a least-privilege ClusterRole: `get;list;watch` on
`capitalmarketsschedules` and `update;patch` on **only** their `/status`
subresource — it never writes the spec (operators own the calendar) and reads
nothing else.

## Author a calendar

```yaml
apiVersion: spotschedules.5spot.finos.org/v1alpha1
kind: CapitalMarketsSchedule
metadata:
  name: nyse-equities
  namespace: capital-markets
spec:
  timezone: America/New_York
  sessions:
    - daysOfWeek: ["mon-fri"]
      hoursOfDay: ["9-16"]
  holidays:
    - "2026-01-01"   # New Year's Day
    - "2026-12-25"   # Christmas Day
  earlyCloses:
    - date: "2026-11-27"   # day after Thanksgiving
      closeHour: 13
```

Then reference it from a machine (see
[`examples/scheduledmachine-spot-schedule.yaml`](../reference/spot-schedule-contract.md)):

```yaml
spec:
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: CapitalMarketsSchedule
    name: nyse-equities   # same namespace as the ScheduledMachine
```

## Observe

- `kubectl get capitalmarketsschedule nyse-equities -o yaml` shows
  `status.active`, `status.nextTransitionTime`, and the `Ready` condition
  (reason `SessionOpen` / `SessionClosed`).
- Metrics `fivespot_capital_markets_active` and
  `fivespot_capital_markets_transitions_total` — see
  [monitoring](../operations/monitoring.md).

## See also

- [Spot Schedules concept](../concepts/spot-schedule.md) — how providers fit in
- [Spot Schedule Provider Contract](../reference/spot-schedule-contract.md) — the spec
- [TimeBasedSpotSchedule provider](time-based-schedule.md) — the default first-party provider
- [Create Your Own Provider](create-your-own-provider.md) — build a different provider
