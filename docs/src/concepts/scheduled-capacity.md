<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Scheduled Capacity

`ScheduledCapacity` **creates and owns** an object that carries a bounded slice
of a machine's capacity, and scales one numeric field on it from the same
spot-schedule providers a `ScheduledMachine` uses
([ADR 0011](https://github.com/finos/5-spot/blob/main/docs/adr/0011-schedule-gated-capacity-separate-controller.md),
actuation per
[ADR 0016](https://github.com/finos/5-spot/blob/main/docs/adr/0016-own-the-capacity-object.md)).

## Handover versus sharing

Every other activation path in 5-Spot ends in the same act: a CAPI `Machine`
exists while the schedule says active, and is drained and deleted when it does
not. That is **handover**: the whole physical machine leaves the cluster so an
incumbent workload can have it back.

`ScheduledCapacity` is the other pattern. On a 32-core host whose incumbent only
ever uses 16, an operator can concede a bounded slice, say 10 guaranteed cores,
for ephemeral workloads **while the machine stays in the cluster the whole
time**. Nothing joins or leaves the cluster; a number changes.

| | `ScheduledMachine` | `ScheduledCapacity` |
|---|---|---|
| What the schedule controls | whether the node is in the cluster | how much of the node a consumer may use |
| Actuation | create and delete CAPI resources | create, then scale, a capacity object |
| What the schedule owns | the `Machine` | the capacity object |
| Owns the drain | 5-Spot (cordon, evict, `nodeDrainTimeout`) | the consumer |
| Window close | the node is drained and deleted | the value goes to zero, the object stays |
| Reversible mid-window | no, the node is gone | yes, the value is just rewritten |

A host is governed by one **or** the other, never both. See
[One host, one governor](#one-host-one-governor).

## What it owns, and what it will not do

5-Spot creates the capacity object, in the `ScheduledCapacity`'s own namespace
and under the `ScheduledCapacity`'s own name, and applies its whole `spec`. That
is symmetric with `ScheduledMachine`, which creates the CAPI `Machine` the
schedule is the reason for, rather than reaching into an object somebody else
made. When the window is open there is a pool of spot capacity because 5-Spot put
one there; when it closes, that pool holds nothing.

There is deliberately **no name** in `spec.target`. The owned object's name is
derived, so the watch, the write and the ownership check cannot aim at different
objects, and no edit can orphan a previously owned object. `spec.target` is also
immutable: changing the kind would leave the object of the old kind behind,
still owned but with nothing reconciling it.

What it will not do:

- **It never deletes on a schedule.** Window close writes the inactive value,
  `0`, and leaves the object standing, so its identity and the consumer's warm
  state survive to the next window and a reopen is a scale rather than a cold
  create.
- **It holds no `delete` verb at all.** Removal happens only when the
  `ScheduledCapacity` is deleted, through Kubernetes owner-reference garbage
  collection. That is not a convenience: RBAC cannot express "only objects you
  created", and garbage collection does not need it to, because it only ever
  removes objects that actually carry the reference. A compromised capacity
  controller therefore cannot destroy a consumer's own object of the same kind.
- **It never writes an object it did not create.** Before any write, the
  reconciler checks that an existing object carries a controller
  `ownerReference` whose `uid` is this `ScheduledCapacity`'s, and refuses
  otherwise with `TargetResolved=False` and reason `TargetNotOwned`. This is the
  check RBAC cannot express, and it is what makes the force-apply below safe.
- **It never drains.** The consumer owns claim binding, idle expiry and its own
  drain. An agent mid-task has no analogue of pod eviction that 5-Spot
  understands, so 5-Spot writes zero and waits. See
  [The handback sequence](#the-handback-sequence).

### A requirement this places on consumers

Deleting a `ScheduledCapacity` garbage-collects the owned object, which cascades
to whatever that object owns. A capacity consumer must therefore **re-parent
in-use resources away from the capacity object when they are claimed**, so that
only idle capacity goes with it. The reference consumer does exactly this, by
decision rather than by accident: a bound pool member's `ownerReferences` move
from the pool to the claim at bind time, so deleting the pool takes the warm
members and leaves the ones somebody is using.

A consumer that keeps owning its in-use resources from the capacity object would
lose work when a `ScheduledCapacity` is deleted. That is a property of the
consumer, not something 5-Spot can enforce.

## Its own identity

`ScheduledCapacity` is reconciled by a **separate binary**,
`5spot-capacity-controller`, with its own Deployment and ServiceAccount
(`deploy/capacity-controller/`). The main controller's `ClusterRole` is not
extended.

That separation is the point: compromising the capacity controller cannot delete
a CAPI `Machine`, and compromising the machine controller cannot write capacity.
The capacity identity holds `get`, `list`, `watch`, `create` and `patch` on
exactly the allowlisted target API group, with **no `delete` anywhere**, no
Secret access and no CAPI verbs.

`create` is a wider grant than the `patch` ADR 0011 originally chose, and that
is worth saying plainly: a compromised capacity controller can construct an
arbitrary object of an allowlisted kind rather than move one integer. What bounds
it is the group allowlist, this separate identity, and the ownership check above.

A cluster with no capacity consumer installs none of this: no Deployment, no
CRD, no RBAC.

## The capacity field, and why it stopped being a security control

`spec.capacity.field` names the numeric knob to scale, **relative to the owned
object's `spec`**: `warmReplicas`, not `spec.warmReplicas`. Dots are allowed for
a nested knob (`scale.warm`).

Under ADR 0011 the equivalent field was `spec.capacity.path`, a path into an
object 5-Spot did **not** own, and it was a genuine confused-deputy primitive:
the value came from whoever could create a `ScheduledCapacity`, and the
controller wrote it with its own broader credential. A `spec.`-prefix rule
existed to keep that path out of `metadata`.

Owning the object removes the problem rather than bounding it. Because the path
is rooted at a `spec` that 5-Spot built, `metadata.` and `status.` are not
rejected, they are **unexpressible**: there is no reachable `ownerReferences`,
`finalizers` or label, and no JSON Pointer escape to attempt. What remains is
schema hygiene, kept strict anyway:

- **Charset**: dot-separated camelCase segments, an allowlist rather than a
  denylist of characters someone thought of.
- **Reserved roots**: `spec`, `metadata` and `status` in first position are
  rejected as *mistakes*, not threats. `spec.warmReplicas` here would construct
  `spec.spec.warmReplicas`, and failing loudly beats building the wrong object.
- **Depth**: at most seven segments, one below the eight a constructed path
  allows, because `spec` occupies the first position.

These are enforced at admission (a schema pattern) **and** again in the
reconciler. The second is not redundant: it is the check that still holds when
the deployed CRD is older than the running controller, which is exactly the case
a schema cannot defend against.

The **inactive** value is fixed at `0` and is not configurable. A schedule that
hands nothing back is not a schedule.

## The template is opaque, and that is a residual

`spec.template` is forwarded verbatim as the owned object's `spec`, with
`spec.capacity.field` injected into it. 5-Spot does not and will not model a
consumer's schema: the reference consumer's pool needs `maxReplicas`, `readiness`
and a nested VM template, none of which is 5-Spot's business. It is the same
pass-through `ScheduledMachine.spec.bootstrapSpec` uses, and it carries the same
residual risk: whatever the consumer's CRD admits, a `ScheduledCapacity` author
can put there, and 5-Spot's ServiceAccount creates it. Bounded by the API-group
allowlist and by the consumer's own admission policies.

`spec.template` may not itself set `spec.capacity.field`. One source of truth, or
the template and the schedule fight on every reconcile and the value depends on
which wrote last.

## Writes are server-side applies

The controller applies the complete `spec` it owns under field manager
`5spot-capacity-controller`. ADR 0011 had to use a JSON merge patch instead,
because it was writing into a stranger's object alongside the stranger's own
field manager and server-side apply would have made 5-Spot a permanent
co-owner. That constraint is gone: 5-Spot created this object, owns the whole of
its `spec`, and the consumer's reconciler writes only `status`.

Applying the complete set also brings pruning, which a merge patch could never
do: a field removed from `spec.template` is removed from the owned object.

## The handback sequence

When the schedule closes, the controller writes **zero first**, then waits for
`spec.handback.drainedPath` to reach zero.

Writing zero first is not an escalation; it is what makes the drain converge.
The gated field is a warm-pool target, so zero stops the consumer replenishing
idle capacity while work already claimed finishes on its own. Holding the field
above zero until the consumer reported drained would be circular, because a pool
that keeps handing out warm members never reports zero in use.

```text
schedule closes
  -> write 0                     phase: HandingBack
  -> drainedPath reaches 0       phase: Inactive,  HandbackComplete=True
  -> or timeout expires first    phase: HandbackTimedOut, value HELD at 0
```

On timeout the controller **holds and reports loudly**. It does not delete,
force or escalate: a missed handover is visible and recoverable, while
destroying an agent mid-task is neither. An unread or non-numeric counter is
**not** treated as drained, so a consumer that reports nothing keeps the object
waiting rather than silently completing a handback that never happened.

If the schedule reopens mid-handback, the active value is simply written again
and the object returns to `Active`. That reversibility is precisely what was
bought by making actuation a scale rather than a delete.

Omit `spec.handback` entirely for a consumer that exposes no in-use counter;
handback then completes as soon as the zero write lands.

## One host, one governor

A host governed by both a `ScheduledMachine` and a `ScheduledCapacity` is a
contradiction, and it half-works: the node is drained for handover while a
capacity gate is still sizing guests on it.

Setting `spec.nodeName` lets the controller detect that. It compares the value
against every `ScheduledMachine.status.nodeRef.name` in the namespace and, on a
match, sets `HostGovernanceConflict=True` and **drives capacity to zero**
(ADR 0014). The active value is never written to a conflicted object, so a
conflicted host cannot be carrying conceded capacity whichever order the
conflict and the write arrived in. Zero is the safe direction: it stops
replenishment on a node the machine controller is already draining, while work
already claimed finishes on its own.

If the owned object does not yet exist, the conflict path does **not** create it
(ADR 0016 decision 8). A misconfigured `ScheduledCapacity` produces no foreign
object at all, which is the fail-closed reading taken one step further than was
possible against a stranger's object.

This check is deliberately in the controller rather than at admission. A
`ValidatingAdmissionPolicy` evaluates one request against its own object and its
bound `paramRef`, and cannot look up other objects, so it can never answer "is
any `ScheduledMachine` already governing this node?".

Without `spec.nodeName` the two objects cannot be correlated and the invariant
is documentation only.

!!! note "Recovery is automatic"
    When `spec.nodeName` no longer matches any `ScheduledMachine`, the next
    reconcile is an ordinary one and the active value is written again if the
    schedule says so. The cost of a mis-set `spec.nodeName` is therefore a
    warm pool that refills, which is why it is optional: leave it unset if you
    cannot name the node confidently, and no conflict is ever detected.

## Status at a glance

```console
$ kubectl get scap
NAME                     PHASE     WRITTEN   TARGET               SCHEDULE                ENABLED   NODE
agent-sandbox-capacity   Active    10        VirtualMachinePool   CapitalMarketsSchedule  true      worker-3
```

Phases are its own, not `ScheduledMachine`'s, because a budget ramp and a drain
wait are not node membership: `Pending`, `Active`, `HandingBack`,
`HandbackTimedOut`, `Inactive`, `Disabled`, `Terminated`, `Error`.

## See also

- [API reference](../reference/api.md#scheduledcapacity) for every field
- [Spot Schedules (Providers)](spot-schedule.md) for the provider contract both
  kinds consume
- [ScheduledMachine](scheduled-machine.md) for the handover pattern
- [Threat model](../security/threat-model.md) section 6.6 and TB7
