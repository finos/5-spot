<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Scheduled Capacity

`ScheduledCapacity` gates **one numeric capacity field on an object 5-Spot does
not own**, driven by the same spot-schedule providers a `ScheduledMachine` uses
([ADR 0011](https://github.com/finos/5-spot/blob/main/docs/adr/0011-schedule-gated-capacity-separate-controller.md)).

## Handover versus sharing

Every other activation path in 5-Spot ends in the same act: a CAPI `Machine`
exists while the schedule says active, and is drained and deleted when it does
not. That is **handover**: the whole physical machine leaves the cluster so an
incumbent workload can have it back.

`ScheduledCapacity` is the other pattern. On a 32-core host whose incumbent only
ever uses 16, an operator can concede a bounded slice, say 10 guaranteed cores,
for ephemeral workloads **while the machine stays in the cluster the whole
time**. Nothing joins or leaves; a number changes.

| | `ScheduledMachine` | `ScheduledCapacity` |
|---|---|---|
| What the schedule controls | whether the node is in the cluster | how much of the node a consumer may use |
| Actuation | create and delete CAPI resources | `patch` one numeric field |
| Owns the drain | 5-Spot (cordon, evict, `nodeDrainTimeout`) | the consumer |
| Reversible mid-window | no, the node is gone | yes, the value is just rewritten |

A host is governed by one **or** the other, never both. See
[One host, one governor](#one-host-one-governor).

## What it will and will not do

It issues exactly one kind of mutation against the governed object: a JSON merge
patch setting the single field named by `spec.capacity.path`. It never creates
that object, never deletes it, never sets an `ownerReference` on it, and never
touches any other field. A `targetRef` that does not resolve is a condition, not
a create.

That restraint is the design, not an omission. A CAPI `Machine` is safe for
5-Spot to delete because 5-Spot owns the drain. A consumer's pool with live
claims is not: an agent mid-task has no analogue of pod eviction that 5-Spot
understands, and the consumer already owns claim binding, idle expiry and its
own drain. Writing a capacity field is reversible, bounded, and leaves drain
where the domain knowledge is.

## Its own identity

`ScheduledCapacity` is reconciled by a **separate binary**,
`5spot-capacity-controller`, with its own Deployment and ServiceAccount
(`deploy/capacity-controller/`). The main controller's `ClusterRole` is not
extended.

That separation is the point: compromising the capacity controller cannot delete
a CAPI `Machine`, and compromising the machine controller cannot write capacity.
The capacity identity holds `patch` on exactly the allowlisted target API group,
with no `create`, no `delete`, no Secret access and no CAPI verbs.

A cluster with no capacity consumer installs none of this: no Deployment, no
CRD, no RBAC.

## The field path is a security control

`spec.capacity.path` names a field on a foreign object, and its value comes from
whoever can create a `ScheduledCapacity`. The controller then writes it with its
own, broader credential, which makes an unconstrained path a confused-deputy
primitive rather than configuration.

It is therefore restricted to dot-separated camelCase segments, at most eight,
and must start with `spec.`:

- **`metadata.` is rejected.** A path reaching `ownerReferences`, `finalizers`
  or labels would let a CR author take ownership of, block deletion of, or
  re-label an object in an API group 5-Spot holds `patch` on.
- **`status.` is rejected.** A status is a controller's own report, not a knob.
- **Array indices, wildcards, `..`, quotes and `/` are inexpressible**, so the
  value can never be read as a JSON Pointer or a JSONPath expression. The
  charset is an allowlist, not a denylist of characters someone thought of.

Both halves are enforced at admission (a schema pattern plus a CEL rule) **and**
again in the reconciler. The second is not redundant: it is the check that still
holds when the deployed CRD is older than the running controller, which is
exactly the case a schema cannot defend against.

The **inactive** value is fixed at `0` and is not configurable. A schedule that
hands nothing back is not a schedule.

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
match, sets `HostGovernanceConflict=True` and **refuses to write at all**.

This check is deliberately in the controller rather than at admission. A
`ValidatingAdmissionPolicy` evaluates one request against its own object and its
bound `paramRef`, and cannot look up other objects, so it can never answer "is
any `ScheduledMachine` already governing this node?".

Without `spec.nodeName` the two objects cannot be correlated and the invariant
is documentation only.

!!! warning "Known gap"
    A conflict that arises *after* a value was written leaves that value in
    place, because refusing to write also refuses to write zero. Recorded in
    the [threat model](../security/threat-model.md) under residual risks.

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
