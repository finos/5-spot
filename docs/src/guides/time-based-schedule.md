<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Guide: TimeBasedSpotSchedule provider

`TimeBasedSpotSchedule` is the **default, first-party spot-schedule provider**
(ADR 0009). It is the reified former inline `ScheduledMachine.spec.schedule`: a
declarative day-of-week / hour-of-day window in a configured timezone. It
publishes a duck-typed `status.active` that a
[`ScheduledMachine.spec.schedule`](../reference/spot-schedule-contract.md)
consumes. A machine bound to it is up while the current time falls inside the
window, down otherwise.

If you previously wrote an inline `spec.schedule` with `daysOfWeek` /
`hoursOfDay` / `timezone`, that window now lives on a `TimeBasedSpotSchedule`
object, and the `ScheduledMachine` references it by name.

## How it works

The `spot-schedule-time-based` controller watches `TimeBasedSpotSchedule`
objects and, for each, computes `status.active` from the `spec` window in the
configured timezone, then **requeues once at the next window boundary** (the next
open or close). It is event-driven — there is no polling interval, and it makes
no network calls: the window lives entirely in `spec`, which operators keep
current via GitOps.

Evaluation each tick:

1. If `spec.enabled` is `false`, `status.active` is always `false` (the window is
   ignored — the provider's own toggle).
2. Otherwise the instant must fall inside the window — a `daysOfWeek` **and**
   `hoursOfDay` match in `spec.timezone`.

Transition detection is **hour-granular**, so the requeue lands within an hour of
the true boundary for whole-hour-offset timezones; the `active` value always
self-corrects on the next reconcile.

## Install

The `TimeBasedSpotSchedule` CRD ships under `deploy/crds/`. Install it and the
provider controller:

```bash
kubectl apply -f deploy/crds/timebasedspotschedule.yaml
kubectl apply -k deploy/spot-schedule-providers/time-based/
```

!!! important "The provider is a separate Deployment — and it is required"
    The provider is its **own controller** (`spot-schedule-time-based`), distinct
    from the 5-Spot controller. It is the only thing that writes
    `TimeBasedSpotSchedule.status.active`, so **if it is not running, no
    `ScheduledMachine` that references a `TimeBasedSpotSchedule` ever activates**
    (5-Spot treats an absent/`StatusActiveMissing` verdict as unresolved). It runs
    as a `Deployment` in `5spot-system`, separate from `deploy/deployment/`.

    The provider binary ships **inside the main 5-Spot image** — the Deployment uses
    the same `ghcr.io/finos/5-spot` image as the controller and selects the provider
    with `command: ["/spot-schedule-time-based"]` (the image is multi-binary: `/5spot`
    plus each first-party provider). `make set-image-version VERSION=…` pins its tag
    alongside the controller at release.

The provider runs with a least-privilege ClusterRole: `get;list;watch` on
`timebasedspotschedules` and `update;patch` on **only** their `/status`
subresource — it never writes the spec (operators own the window) and reads
nothing else.

## Author a window

```yaml
apiVersion: spotschedules.5spot.finos.org/v1alpha1
kind: TimeBasedSpotSchedule
metadata:
  name: business-hours
  namespace: default
spec:
  daysOfWeek: ["mon-fri"]
  hoursOfDay: ["9-17"]
  timezone: America/New_York
  enabled: true
```

### Day format

- Single day: `mon`, `tue`, `wed`, `thu`, `fri`, `sat`, `sun`
- Range: `mon-fri`, `sat-sun`
- Mixed: `mon-wed,fri`

### Hour format

- Single hour: `9`, `14`, `22`
- Range: `9-17` (inclusive of both start and end)
- Mixed: `0-9,17-23`

Then reference it from a machine (see
[`examples/timebasedspotschedule.yaml`](../reference/spot-schedule-contract.md)
and `examples/scheduledmachine-basic.yaml`):

```yaml
spec:
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: business-hours   # same namespace as the ScheduledMachine
```

## Observe

- `kubectl get timebasedspotschedule business-hours -o yaml` (short name `tbss`)
  shows `status.active`, `status.nextTransitionTime`, and the `Ready` condition
  (reason `WindowOpen` / `WindowClosed`).
- Metrics `fivespot_time_based_active` and
  `fivespot_time_based_transitions_total` — see
  [monitoring](../operations/monitoring.md).

## See also

- [Spot Schedules concept](../concepts/spot-schedule.md) — how providers fit in
- [Spot Schedule Provider Contract](../reference/spot-schedule-contract.md) — the spec
- [CapitalMarketsSchedule provider](capital-markets-schedule.md) — the exchange-calendar provider
- [Create Your Own Provider](create-your-own-provider.md) — build a different provider
