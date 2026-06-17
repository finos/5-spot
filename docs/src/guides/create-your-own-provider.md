<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Guide: Create your own spot-schedule provider

A **spot-schedule provider** is any namespaced CRD in the
`spotschedules.5spot.finos.org` API group whose controller publishes a
duck-typed `status.active` boolean. A
[`ScheduledMachine.spec.schedule`](../concepts/spot-schedule.md) that
references it then follows that boolean — up when `active: true`, down when
`false`. 5-Spot reads **only** your `status` (never your `spec`) and never writes
your object, so you are free to model activation however you like: a PromQL
signal, a change-freeze calendar, a ticketing-system gate, or a plain manual
toggle.

This guide builds the smallest possible provider — a **`ManualSchedule`** with a
single `spec.enabled` flag — end to end. The in-repo
[`CapitalMarketsSchedule`](capital-markets-schedule.md) is the same shape with an
exchange calendar instead of a toggle; read its
[source](https://github.com/finos/5-spot/tree/main/src/providers/capital_markets.rs)
for a complete Rust reference.

Read the [provider contract](../reference/spot-schedule-contract.md) first — this
guide is the *how-to*; that page is the authoritative *spec*.

## 1. Define the CRD

Your CRD must be **namespaced**, live in the `spotschedules.5spot.finos.org`
group, and enable the **`status` subresource** (so the controller can `PATCH`
`/status`). The `status.active` boolean is the only field 5-Spot requires; a
`Ready` condition is recommended.

```yaml
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: manualschedules.spotschedules.5spot.finos.org
spec:
  group: spotschedules.5spot.finos.org
  scope: Namespaced
  names:
    kind: ManualSchedule
    plural: manualschedules
    singular: manualschedule
    shortNames: ["ms"]
  versions:
    - name: v1alpha1
      served: true
      storage: true
      subresources:
        status: {} # REQUIRED so the controller can write /status
      additionalPrinterColumns:
        - name: Active
          type: boolean
          jsonPath: .status.active
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              properties:
                enabled:
                  type: boolean
                  description: When true, the schedule is active.
              required: ["enabled"]
            status:
              type: object
              properties:
                active:
                  type: boolean
                observedGeneration:
                  type: integer
                  format: int64
                lastTransitionTime:
                  type: string
                conditions:
                  type: array
                  items:
                    type: object
                    properties:
                      type: { type: string }
                      status: { type: string }
                      reason: { type: string }
                      message: { type: string }
                      lastTransitionTime: { type: string }
```

## 2. Write the controller

A provider controller is a normal Kubernetes controller: **watch** your kind and,
for each object, **patch its `/status`** so `status.active` reflects your logic.
Any language / framework works (controller-runtime, kube-rs, kopf, a shell loop
with `kubectl` — your call). The reconcile for `ManualSchedule` is a one-liner:
copy `spec.enabled` into `status.active`.

The only network write you make is the status patch:

```jsonc
// PATCH (merge) /apis/spotschedules.5spot.finos.org/v1alpha1/
//   namespaces/<ns>/manualschedules/<name>/status
{
  "status": {
    "active": <spec.enabled>,
    "observedGeneration": <metadata.generation>,
    "lastTransitionTime": "<RFC3339, bumped only when active flips>",
    "conditions": [
      { "type": "Ready", "status": "True",
        "reason": "Reconciled", "message": "manual toggle applied",
        "lastTransitionTime": "<RFC3339>" }
    ]
  }
}
```

Pseudo-reconcile:

```text
on event (add/update) of ManualSchedule obj:
    active = obj.spec.enabled
    patch obj /status with:
        active, observedGeneration = obj.metadata.generation,
        Ready = True,
        lastTransitionTime = now if active changed else keep prior
```

A provider that has work to schedule (e.g. "re-evaluate at the next calendar
boundary") should **requeue at that instant** rather than poll — see how
`CapitalMarketsSchedule` returns `Action::requeue(next_transition - now)`. A pure
toggle like `ManualSchedule` needs no requeue at all: it only reacts to spec
edits.

### `Ready` semantics

`Ready` is *recommended, not required*:

- Omit `Ready`, or set it `True` → your `status.active` is taken as
  **authoritative**.
- Set `Ready` to anything other than `True` → 5-Spot treats the reference as
  **unresolved** and **holds the machine's last known state** (it does not flip
  the machine). Use this to say "my `active` is stale right now — don't trust
  it" without flapping machines.

## 3. Grant least privilege

Your provider's ServiceAccount needs **only** read on your kind and write on its
status — nothing else. It must never touch `ScheduledMachine` or any other
5-Spot resource.

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: manual-schedule-provider
  namespace: 5spot-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: manual-schedule-provider
rules:
  - apiGroups: ["spotschedules.5spot.finos.org"]
    resources: ["manualschedules"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["spotschedules.5spot.finos.org"]
    resources: ["manualschedules/status"]
    verbs: ["get", "update", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: manual-schedule-provider
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: manual-schedule-provider
subjects:
  - kind: ServiceAccount
    name: manual-schedule-provider
    namespace: 5spot-system
```

(A ClusterRole is used because provider objects live in the tenant namespaces
where the `ScheduledMachine`s that reference them live.)

## 4. Deploy the controller

Run it like any controller — a hardened `Deployment` is ideal (non-root,
read-only root filesystem, all capabilities dropped, seccomp `RuntimeDefault`).
See
[`deploy/spot-schedule-providers/capital-markets/deployment.yaml`](https://github.com/finos/5-spot/blob/main/deploy/spot-schedule-providers/capital-markets/deployment.yaml)
for a template you can copy.

## 5. Reference it from a ScheduledMachine

```yaml
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: trading-desk-rack
  namespace: trading           # provider object lives here too
spec:
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: ManualSchedule
    name: desk-override
  # ... clusterName, bootstrapSpec, infrastructureSpec ...
```

Then flip the toggle:

```bash
kubectl -n trading patch manualschedule desk-override \
  --type=merge -p '{"spec":{"enabled":true}}'
```

## 6. Verify

```bash
# Your provider published its decision:
kubectl -n trading get manualschedule desk-override -o jsonpath='{.status.active}{"\n"}'

# 5-Spot resolved it — the SM carries a SpotScheduleResolved condition:
kubectl -n trading get scheduledmachine trading-desk-rack \
  -o jsonpath='{.status.spotSchedule}{"\n"}'
```

If the reference can't be resolved (CRD not installed, object missing, no
`status.active`, or `Ready` ≠ `True`), the `ScheduledMachine` reports
`SpotScheduleResolved=False` with a reason and 5-Spot **holds last state** —
watch `fivespot_spot_schedule_resolution_errors_total` (see
[monitoring](../operations/monitoring.md)).

## Versioning & compatibility

Per [ADR 0007](https://github.com/finos/5-spot/blob/main/docs/adr/0007-crd-multi-version-and-conversion.md),
5-Spot resolves providers by **group + kind**, not a pinned `apiVersion`, so you
may serve multiple versions of your CRD. Evolve the `status` contract
**additively** — add new optional fields; never rename, retype, or remove a
field 5-Spot reads (`active`, the `Ready` condition). 5-Spot ignores any extra
status fields, so you can carry as much provider-specific status as you like.

## Checklist

- [ ] CRD in `spotschedules.5spot.finos.org`, **namespaced**, `status`
      subresource enabled
- [ ] Controller patches `status.active` (+ recommended `Ready`,
      `observedGeneration`, `lastTransitionTime`)
- [ ] Event-driven (requeue at your next boundary; don't poll)
- [ ] Least-privilege RBAC: read your kind, write only its `/status`
- [ ] A `ScheduledMachine.spec.schedule` in the **same namespace** references it

## See also

- [Spot Schedule Provider Contract](../reference/spot-schedule-contract.md) — the authoritative spec
- [Spot Schedules concept](../concepts/spot-schedule.md) — why duck-typing
- [CapitalMarketsSchedule provider](capital-markets-schedule.md) — the in-repo reference
