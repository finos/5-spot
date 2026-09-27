<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0011 — Schedule-gated capacity: a `ScheduledCapacity` CRD and its own controller

- **Status:** Proposed
- **Date:** 2026-09-27
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0006](./0006-pluggable-spot-schedule-provider-contract.md)
  (provider contract), ADR [0009](./0009-unify-schedule-as-provider-reference.md)
  (`spec.schedule` as a provider reference), ADR
  [0007](./0007-crd-multi-version-and-conversion.md) (additive-only CRD
  evolution), `docs/src/reference/spot-schedule-contract.md`. Consumer side:
  banlieue ADR-0046 (`VirtualMachinePool`), ADR-0047 (`VirtualMachineClaim`),
  ADR-0060 (host-resident Cloud Hypervisor provider), ADR-0063 (transient
  systemd units and cgroup limits), and its planned budget ADR.

## Context

Every activation path 5-Spot has today ends in the same act: a CAPI `Machine`
(with its bootstrap and infrastructure resources) exists while the schedule says
active, and is drained and deleted when it does not. That is **handover** — the
whole physical machine leaves the cluster so an incumbent workload can have it
back.

A second pattern is now wanted, and it is not handover. On a 32-core host whose
incumbent only ever uses 16, the operator will concede a bounded slice — say 10
cores guaranteed with a ceiling of 16, memory likewise — for ephemeral microVM
workloads, while **the machine stays in the cluster the whole time**. On the
consumer side that slice is a Cloud Hypervisor guest under a cgroup-limited
transient systemd unit (banlieue ADR-0060/ADR-0063), sized by a host budget, and
consumed by a `VirtualMachinePool` or an agent sandbox built on one. What 5-Spot
owns in that picture is the only thing it has ever owned: **the calendar**, and
in particular `CapitalMarketsSchedule`, which already knows what a trading day
looks like.

So the question is not *what a slice is*. It is **who drives it, and from where.**

| | Option | For | Against |
| --- | --- | --- | --- |
| A | **The consumer pulls.** banlieue's `Provider` takes an optional schedule reference and reads `status.active` itself. 5-Spot changes nothing but its docs. | Cheapest by a wide margin — one optional field on the consumer, no new CRD, controller, binary or RBAC here. The contract is already duck-typed, so it works for any consumer. | The intent is split across two objects in two repos. An operator reading a 5-Spot schedule cannot see that a host's capacity depends on it, and vice versa. Nothing in the repo that owns the calendar records what the calendar is *for*. |
| B | **A mode on `ScheduledMachine`.** | One CRD, one controller. | Most of `ScheduledMachineSpec` is meaningless for a slice — `clusterName`, `bootstrapSpec`, `infrastructureSpec`, `machineTemplate`, `nodeDrainTimeout` — and the phase set (`Pending → Active → ShuttingDown → Inactive`) describes node membership, not a budget ramp. Under ADR-0007's additive-only rule, a field added for this is served forever. It is a second kind wearing the first one's clothes. |
| C | **A new CRD in the existing controller and binary.** | No new Deployment to operate. | The controller's `ClusterRole` would gain write access to a foreign API group. The identity that can delete CAPI `Machine`s cluster-wide would also write sandbox capacity, on top of the `bootstrap`/`infrastructure` wildcards the threat model already carries as a HIGH residual. Clusters with no consumer installed still get the CRD and the RBAC. |
| D | **A new CRD, reconciler, binary and ServiceAccount.** *Chosen.* | A separate identity: compromising the capacity controller cannot delete a `Machine`, and compromising the machine controller cannot write capacity. The consumer dependency becomes optional — no consumer, no Deployment, no CRD, no RBAC. Symmetric with the two provider binaries that already *produce* this contract. Each CRD keeps its own phase set. | One more binary, image, Deployment and RBAC set to operate and review. |

Two facts made D affordable rather than aspirational:

1. **The schedule brain is already reusable.**
   `resolve_spot_schedule(client, namespace, &SpotScheduleRef) -> SpotScheduleVerdict`
   in `src/reconcilers/spot_schedule.rs` is written against `DynamicObject` and
   discovery, returns `Active` / `Inactive` / `Unresolved` with reasons, and
   names `ScheduledMachine` only in doc comments. A second consumer reuses it
   unchanged, so the `Unresolved`-is-not-`Inactive` rule, the `Ready` gate and
   the precedence order cannot drift between the two controllers.
2. **The binary precedent exists.** `5spot-reclaim-agent`,
   `5spot-kata-config-agent`, `spot-schedule-time-based` and
   `spot-schedule-capital-markets` already ship as separate binaries with their
   own identities. Two of them are *producers* of this contract; a consumer as
   its own binary is the same shape.

### Why the actuation is a scale, not a create and delete

5-Spot already creates and deletes foreign resources on a schedule —
`EmbeddedResource`, `validate_api_group()` and a pre-flight
`SelfSubjectAccessReview` are exactly that machinery — so reusing it for a
consumer's object is the obvious move. It is also wrong here.

A CAPI `Machine` is safe for 5-Spot to delete because 5-Spot owns the drain:
cordon, evict, `nodeDrainTimeout`, `gracefulShutdownTimeout`. A
`VirtualMachinePool` with live claims is not. An agent mid-task has no analogue
of pod eviction that 5-Spot understands, and the consumer already owns those
semantics — claim binding, idle expiry, its own drain. Deleting the pool at
market open would pull the rug on claim holders using knowledge this controller
does not have, and it is not reversible if the window reopens.

Writing a capacity field is reversible, bounded, and leaves drain where the
domain knowledge is.

## Decision

**A schedule gates *capacity on an existing object*, through a new
`ScheduledCapacity` CRD reconciled by its own controller, binary and
ServiceAccount. It never creates or deletes the object it governs.**

1. **`ScheduledCapacity`**, namespaced, group `5spot.finos.org`, served
   `v1alpha1`. Its spec carries a `schedule` reference (the same
   `SpotScheduleRef` shape ADR-0009 pinned), a `targetRef` naming the object to
   govern, the JSON path of the field to write, and the `active` value to write.
   The inactive value is fixed at the type's zero — a schedule that hands
   nothing back is not a schedule.

2. **Actuation is a field write, never a create or a delete.** The controller
   patches one field on an object it does not own and never sets an
   `ownerReference`. If `targetRef` does not resolve, that is `Unresolved` and a
   condition, not a create.

3. **Its own identity.** A `5spot-capacity-controller` binary, Deployment and
   ServiceAccount, with `patch` on exactly the allowlisted target groups. **The
   existing controller's `ClusterRole` is not extended.** A cluster with no
   capacity consumer installs none of this.

4. **The schedule verdict comes from `resolve_spot_schedule`, reused as is.**
   `Unresolved` never means `Inactive`: it holds the last written value and
   reports why, exactly as the machine controller holds a machine. `killSwitch`
   and `spec.enabled` keep their precedence over the provider's `active`.

5. **Target API groups are allowlisted**, in a third constant beside
   `ALLOWED_BOOTSTRAP_API_GROUPS` and `ALLOWED_INFRASTRUCTURE_API_GROUPS`, and a
   pre-flight `SelfSubjectAccessReview` for `patch` runs before the first write
   so an RBAC gap surfaces as a clear condition rather than a failed reconcile.

6. **Handback is cooperative and bounded.** On deactivation the controller writes
   zero and then *waits* for the consumer to report drained, up to
   `handbackTimeout`. It does not delete, force, or escalate on its own.

7. **A host is governed by `ScheduledMachine` or by `ScheduledCapacity`, never
   both.** They are opposite answers to "what happens to this metal on a
   schedule": one removes the node, the other keeps it and shares it. Both
   pointed at one host is a contradiction, and it half-works — the node is
   drained for handover while a capacity gate is still sizing guests on it. A
   `ValidatingAdmissionPolicy` rejects the overlap where the two objects can be
   correlated; where they cannot, the invariant is documented and the
   `ScheduledCapacity` status says which host it believes it governs.

8. **Its own phase and condition set**, not `ScheduledMachine`'s. A budget ramp
   and a drain wait are not node membership, and ADR-0007 makes a borrowed enum
   permanent.

## Consequences

**Easier.** The calendar stays in the repo that owns it, and one object states
the whole intent: this schedule, that target, that much capacity. Any consumer
that exposes a numeric capacity field can be gated without a line of code here —
the allowlist and RBAC are configuration. The two controllers cannot drift on
schedule semantics because they call the same function.

**Harder / newly ruled out.**

- One more binary, image, Deployment and RBAC set. Operationally this is the
  cost of option D and it is paid per install that wants the feature.
- 5-Spot now writes outside the CAPI API family for the first time. Contained to
  a separate ServiceAccount, but it is a new posture and the threat model has to
  say so.
- Option A stays strictly cheaper. If the intent-in-one-object argument ever
  stops mattering, A is the smaller system, and this ADR should be revisited
  rather than defended.
- Gating anything that needs *creation* rather than scaling is out of scope by
  construction. That is a different ADR, and it inherits the drain problem.

**CALM impact: yes — required before implementation.** A new controller node, a
new CRD, a flow from the schedule provider through the new controller to a
foreign object, and a new identity. The trust boundary around "5-Spot writes an
API group it does not own" is new and must be drawn.

**Threat model impact.** A new identity with `patch` on a foreign group, holding
no Secret access. It compounds nothing directly, but it sits beside two HIGH
residuals already recorded (`bootstrapSpec.spec` plaintext, and the
`bootstrap`/`infrastructure` wildcards), and the reason the grant is separate is
to keep them from compounding. A full pass is due when this is implemented.

**Open decision, needed before code.** What happens when `handbackTimeout`
expires and the consumer has not finished draining: hold capacity above zero and
miss the handover, or force it to zero and break in-flight work? That is a
business call, not an engineering one. Until it is answered, the controller
should fail safe by holding and reporting loudly, because a missed handover is
visible and recoverable while a killed agent task is neither.

**Follow-ups.** The roadmap entry for the integration lives on the consumer
side; 5-Spot carries a `ROADMAPS.md` row pointing at it so the dependency is
visible from both repositories. The CRD shape above is deliberately sketched,
not settled — field names land with the TDD step, under ADR-0007's
additive-only rule.
