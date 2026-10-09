<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 05: Schedule-gated capacity

> The non-handover pattern: a host stays in the cluster and a bounded slice of
> it is conceded on a schedule. Decided in
> [ADR 0011](../../docs/adr/0011-schedule-gated-capacity-separate-controller.md),
> implemented 2026-10-03/04, and **reshaped 2026-10-08** by
> [ADR 0016](../../docs/adr/0016-own-the-capacity-object.md): 5-Spot creates and
> owns the capacity object instead of patching a consumer's.

## Why this exists

Every other activation path in 5-Spot ends in handover: the whole machine leaves
the cluster so an incumbent can have it back. This one does not. On a 32-core
host whose incumbent only ever uses 16, the operator concedes a bounded slice
for ephemeral workloads while the machine stays in the cluster throughout.

What 5-Spot owns in that picture is the only thing it has ever owned: **the
calendar.** The slice itself, the guests inside it, their isolation and their
drain all belong to the consumer.

ADR 0011 read that division as "5-Spot must not create the object either", and
built a field-path apparatus so it could safely reach into one the consumer made.
ADR 0016 reversed that half. The division is about **the slice**, not about the
object that declares it: 5-Spot creates the capacity object the way it creates
the CAPI `Machine` a `ScheduledMachine` is the reason for, and still knows
nothing about what runs inside. The premise ADR 0011 refused on, that deleting a
pool would pull the rug on claim holders, was checked against the consumer's code
and turned out to be false: it re-parents claimed members to their claim, so a
pool deletion takes the idle members and leaves the ones somebody is using.

## Status

| Phase | What | State |
|---|---|---|
| 1 | ADR 0011 accepted; CALM model, diagrams, trust boundary | ✅ |
| 2 | `ScheduledCapacity` CRD at `v1alpha1`, write-path control, generated CRD + example | ✅ |
| 3 | Dynamic reference watch generalized over the referencing kind; leader election extracted | ✅ |
| 4 | Reconciler, `5spot-capacity-controller` binary, metrics | ✅ |
| 5 | Own ServiceAccount, least-privilege ClusterRole, Deployment, kustomization | ✅ |
| 6 | API reference, this doc, concepts page, threat-model pass to v1.3 | ✅ |
| 6a | ADR 0014: conflict drives capacity to zero; threat-model pass to v1.4 | ✅ |
| 6b | ADR 0016: 5-Spot creates and owns the capacity object. `spec.targetRef` + `capacity.path` withdrawn for `spec.target` + `capacity.field` + `spec.template`; SSA replaces merge patch; RBAC gains `create` and still holds no `delete`; removal by owner-reference GC; reconciler ownership check | ✅ |
| 6c | `kind` e2e: stub consumer CRD on the real group, 16-test bats suite, `make kind-deploy-capacity` + `kind-verify-capacity` | ✅ written, ⛔ not yet run (needs a built image) |
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

  **Superseded by ADR 0016.** That control defended a path into a stranger's
  object. With the object owned, the path is rooted at a `spec` 5-Spot built, so
  `metadata.` and `status.` are unexpressible rather than rejected. The charset
  and depth rules survive as schema hygiene, and the reserved-root rejections
  are now about mistakes (`spec.warmReplicas` would build
  `spec.spec.warmReplicas`) rather than threats.
- **Restraint.** Writing the capacity field left every sibling field untouched
  and set no `ownerReferences`, finalizers or labels.
- **Never a create.** A `ScheduledCapacity` naming a non-existent target
  reported `TargetResolved=False` and did not bring the object into existence.
  **Reversed by ADR 0016**: creating the object is now the point, and the
  property that replaces it is narrower and stronger, that 5-Spot never writes an
  object it did not create. The reconciler refuses any object without a
  controller `ownerReference` whose `uid` is its own (`TargetNotOwned`), which is
  the constraint RBAC cannot express.
- **The full cycle.** `Active` at the active value, schedule closes,
  `HandingBack` at zero, consumer reports drained, `Inactive`. A
  `HandbackTimedOut` object returned to `Active` when the window reopened.
- **Conflict detection.** With `spec.nodeName` matching a `ScheduledMachine`'s
  `status.nodeRef`, the object went to `Error` with
  `HostGovernanceConflict=True`. This run is what exposed the gap ADR 0014
  then closed: it ended with the conflict reported **and** the target still
  holding the active value.

A hot reconcile loop was found this way and only this way: the controller
re-patched its own status every reconcile, re-triggering its own watch at ~50
`resourceVersion` bumps per second on an idle object. No unit test could see it.
Logged as `bug-005`.

## Open

- ~~A governance conflict does not retract capacity already written.~~
  **Closed 2026-10-05 by [ADR 0014](../../docs/adr/0014-zero-capacity-on-governance-conflict.md):**
  a conflict now drives capacity to zero, and the active value is never written
  to a conflicted object. The threat model's MEDIUM residual was removed rather
  than reworded, replaced by a LOW in the other direction (a mis-set
  `spec.nodeName` zeroes capacity until the reference is corrected).
- **Consumer-side integration is not 5-Spot's to build.** The roadmap entry for
  wiring a real consumer lives with that consumer. 5-Spot carries this row so
  the dependency is visible from both sides, as ADR 0011's follow-up asks.
- ~~No `kind` e2e.~~ **Written 2026-10-08, not yet run.**
  `.github/scripts/capacity-e2e.bats` (16 tests) plus
  `make kind-deploy-capacity` and `make kind-verify-capacity`. The stub consumer
  CRD (`.github/scripts/fixtures/capacity-target-crd.yaml`) is typed, and is on
  the **real** group and kind (`banlieue.io/v1alpha1 VirtualMachinePool`): under
  ADR 0016 the group allowlist gates the create, so a stub under an invented
  group would be refused before any interesting behaviour ran, and widening the
  allowlist for the test would mean the test no longer exercised the control. Its
  required set is generated from the real kind's CRD, and a unit test
  (`test_build_owned_object_satisfies_the_consumer_schemas_required_set`) reads
  that fixture and asserts the object 5-Spot builds satisfies it, so the two
  tiers cannot disagree about what the consumer demands. That test earned its
  keep immediately: it found the unit fixture's template missing a field the
  consumer requires, which would have been a 422 visible only at the e2e tier.

  **Still to do: run it.** It needs a locally built controller image, which is
  the maintainer's step, not the agent's.
