<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 05: Schedule-gated capacity

> The non-handover pattern: a host stays in the cluster and a bounded slice of
> it is conceded on a schedule. Decided in
> [ADR 0011](../../docs/adr/0011-schedule-gated-capacity-separate-controller.md),
> implemented 2026-10-03/04.

## Why this exists

Every other activation path in 5-Spot ends in handover: the whole machine leaves
the cluster so an incumbent can have it back. This one does not. On a 32-core
host whose incumbent only ever uses 16, the operator concedes a bounded slice
for ephemeral workloads while the machine stays in the cluster throughout.

What 5-Spot owns in that picture is the only thing it has ever owned: **the
calendar.** The slice itself, the guests inside it, their isolation and their
drain all belong to the consumer. That division is why actuation is a field
write and not a create.

## Status

| Phase | What | State |
|---|---|---|
| 1 | ADR 0011 accepted; CALM model, diagrams, trust boundary | ✅ |
| 2 | `ScheduledCapacity` CRD at `v1alpha1`, write-path control, generated CRD + example | ✅ |
| 3 | Dynamic reference watch generalized over the referencing kind; leader election extracted | ✅ |
| 4 | Reconciler, `5spot-capacity-controller` binary, metrics | ✅ |
| 5 | Own ServiceAccount, least-privilege ClusterRole, Deployment, kustomization | ✅ |
| 6 | API reference, this doc, concepts page, threat-model pass to v1.3 | ✅ |
| 7 | Consumer-side integration | ⛔ consumer-side, see below |

## Verified against a live cluster

Not only unit tests. Against a real API server and a real consumer CRD:

- **RBAC, 26/26 assertions** via `SubjectAccessReview`. The capacity identity
  can `patch` the allowlisted target group and its own `/status`; it **cannot**
  create or delete the governed object, patch a sibling kind in that group,
  touch `cluster.x-k8s.io/machines`, read a Secret, patch a `ScheduledMachine`,
  or evict a Pod.
- **The write-path control at admission.** `metadata.ownerReferences`,
  `metadata.finalizers`, `metadata.labels`, `status.claimed`, a 9-segment path,
  a PascalCase segment and a bare prefix are all rejected; `spec.warmReplicas`
  and an 8-segment path are accepted. `activeValue` is bounded 1..=1000000 and
  `drainedPath` is pinned to `status.`.
- **Restraint.** Writing the capacity field left every sibling field untouched
  and set no `ownerReferences`, finalizers or labels.
- **Never a create.** A `ScheduledCapacity` naming a non-existent target
  reported `TargetResolved=False` and did not bring the object into existence.
- **The full cycle.** `Active` at the active value, schedule closes,
  `HandingBack` at zero, consumer reports drained, `Inactive`. A
  `HandbackTimedOut` object returned to `Active` when the window reopened.
- **Conflict detection.** With `spec.nodeName` matching a `ScheduledMachine`'s
  `status.nodeRef`, the object went to `Error` with
  `HostGovernanceConflict=True` and refused to write.

A hot reconcile loop was found this way and only this way: the controller
re-patched its own status every reconcile, re-triggering its own watch at ~50
`resourceVersion` bumps per second on an idle object. No unit test could see it.
Logged as `bug-005`.

## Open

- **A governance conflict does not retract capacity already written.** "Refuse
  to write" also refuses to write zero, so a conflict arising after a value
  landed leaves it in place. Recorded as a residual risk in the threat model;
  closing it is a superseding-ADR decision, not a CRD field.
- **Consumer-side integration is not 5-Spot's to build.** The roadmap entry for
  wiring a real consumer lives with that consumer. 5-Spot carries this row so
  the dependency is visible from both sides, as ADR 0011's follow-up asks.
- **No `kind` e2e.** The live verification above was run by hand against a
  cluster. Folding it into `make kind-*` needs a stub target CRD that is typed
  rather than `x-kubernetes-preserve-unknown-fields`, since a fake more
  permissive than the real thing hides exactly the bugs this would be for.
