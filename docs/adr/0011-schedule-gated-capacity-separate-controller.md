<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0011 — Schedule-gated capacity: a `ScheduledCapacity` CRD and its own controller

- **Status:** Accepted
- **Date:** 2026-09-27 (proposed), 2026-10-02 (accepted, field names and the
  handback decision settled)
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
   `v1alpha1`, shortname `scap`. Its spec carries a `schedule` reference (the
   same `SpotScheduleRef` shape ADR-0009 pinned), a `targetRef` naming the
   object to govern, the JSON path of the field to write, and the `active` value
   to write. The inactive value is fixed at the type's zero: a schedule that
   hands nothing back is not a schedule.

   The field names, settled 2026-10-02 at the TDD step:

   ```yaml
   spec:
     schedule:                   # SpotScheduleRef, group-pinned as ADR-0009
       apiVersion: spotschedules.5spot.finos.org/v1alpha1
       kind: CapitalMarketsSchedule
       name: nyse-trading-day
     enabled: true               # default true
     killSwitch: false           # default false
     targetRef:                  # this object's own namespace, group allowlisted
       apiVersion: banlieue.io/v1alpha1
       kind: VirtualMachinePool
       name: agent-sandboxes
     capacity:
       path: spec.warmReplicas   # must start with `spec.`
       activeValue: 10           # inactive value fixed at 0
     handback:                   # optional, see decision 6
       drainedPath: status.claimed
       timeout: 10m
     nodeName: worker-3          # optional, see decision 7
   ```

   **`capacity.path` validation is a control, not a convenience.** The path
   names a field on an object 5-Spot does not own, so it is constrained to
   dot-separated camelCase segments
   (`^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*)*$`, at most 8 of them): no array
   indices, no wildcards, no `..`, no quotes and no `/`, so it can never be
   abused as a JSON Pointer escape. `capacity.path` **must** start with
   `spec.`; `metadata.` is rejected because a CR author who could write
   `ownerReferences`, `finalizers` or labels on a foreign object would have an
   elevation primitive, and `status.` is rejected because a status is a
   controller's own report, not a knob. `handback.drainedPath` must start with
   `status.`. Enforced both in the CRD schema (pattern plus a CEL rule) and
   again in the reconciler, because the second is the one that holds if the
   CRD is ever applied out of date.

   **Actuation is a JSON merge patch**, built by nesting the validated path.
   Server-side apply was considered and rejected: SSA would make 5-Spot a
   permanent co-owner of the target field and fight the consumer's own GitOps
   field manager on every reconcile. A merge patch writes the value and leaves
   ownership where it was.

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

6. **Handback is cooperative and bounded, and on expiry it holds.** On
   deactivation the controller writes zero and then *waits* for the consumer to
   report drained, up to `handback.timeout`. It does not delete, force, or
   escalate on its own.

   "Drained" is read from `handback.drainedPath`, a validated `status.` path on
   the target that must reach zero. That keeps the wait as generic as the write:
   any consumer exposing a numeric in-use counter can be gated without a line of
   code here, and no consumer has to adopt a 5-Spot-specific `Drained`
   condition. For banlieue's `VirtualMachinePool` the path is `status.claimed`.

   **`handback` is optional.** Absent, handback completes as soon as the zero
   write lands. This is a deliberate relaxation of the original wording, which
   required the wait unconditionally: a consumer with no observable in-use
   counter can still legitimately be gated, and ADR-0007's additive-only rule
   means a wrongly-required field could never be removed later.

   **When `handback.timeout` expires with the consumer still not drained, the
   controller holds the written value and reports loudly** (phase
   `HandbackTimedOut`, `HandbackComplete=False`, a metric). It never forces the
   value to zero. This resolves the open decision this ADR carried: a missed
   handover is visible and recoverable, while a killed agent task is neither,
   and forcing would destroy in-flight work using knowledge this controller does
   not have, which is the same reasoning that made actuation a scale rather
   than a delete. No per-object `onTimeout` knob: ADR-0007 would make that enum
   permanent, and it would push a safety decision onto every author of a CR.

7. **A host is governed by `ScheduledMachine` or by `ScheduledCapacity`, never
   both.** They are opposite answers to "what happens to this metal on a
   schedule": one removes the node, the other keeps it and shares it. Both
   pointed at one host is a contradiction, and it half-works — the node is
   drained for handover while a capacity gate is still sizing guests on it.

   **The overlap is detected in the controller, not at admission.** The original
   wording said a `ValidatingAdmissionPolicy` rejects it. That is not
   implementable: a VAP evaluates one request against its own object and its
   bound `paramRef`, and has no way to look up other objects, so it cannot ask
   "is any `ScheduledMachine` in this namespace already governing this node?".
   Instead:

   - `spec.nodeName` is an optional declaration of the host this object
     believes it governs;
   - the reconciler lists `ScheduledMachine`s in the namespace and compares
     `spec.nodeName` against each `status.nodeRef.name`;
   - on a hit it sets `HostGovernanceConflict=True` and **drives capacity to
     zero**, rather than half-applying the contradiction. *(Amended by
     [ADR-0014](./0014-zero-capacity-on-governance-conflict.md): this
     originally said "refuses to write capacity at all", which left a value
     written before the conflict standing, i.e. the exact contradiction this
     decision forbids. Zero is the safe direction, and the active value is
     still never written to a conflicted object.)*;
   - `status.governedNode` reports the host either way, which is what the
     original wording already prescribed for the uncorrelatable case.

   Where `spec.nodeName` is absent the two objects cannot be correlated at all
   and the invariant is documentation only, as before.

8. **Its own phase and condition set**, not `ScheduledMachine`'s. A budget ramp
   and a drain wait are not node membership, and ADR-0007 makes a borrowed enum
   permanent.

   Phases: `Pending`, `Active`, `HandingBack`, `HandbackTimedOut`, `Inactive`,
   `Disabled`, `Terminated`, `Error`.

   Conditions: `Ready`, `TargetResolved`, `CapacityWritten`,
   `HandbackComplete`, `HostGovernanceConflict`, plus `SpotScheduleResolved`
   reused unchanged from ADR-0006 so the two controllers report provider
   resolution identically.

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

**Resolved decision (2026-10-02), formerly open.** What happens when
`handback.timeout` expires and the consumer has not finished draining: hold
capacity above zero and miss the handover, or force it to zero and break
in-flight work? **Hold and report loudly**, per decision 6. The provisional
fail-safe this ADR recorded while the question was open is now the decision, for
the reason it gave: a missed handover is visible and recoverable while a killed
agent task is neither. Revisit only with a superseding ADR, not with a CRD
field.

**Amended after implementation (2026-10-05).** Decision 7's conflict behaviour
is superseded by [ADR-0014](./0014-zero-capacity-on-governance-conflict.md):
a conflict drives capacity to zero instead of refusing to write. Everything
else in this ADR stands.

**Amended at acceptance (2026-10-02).** Three things changed between Proposed
and Accepted, all recorded above rather than silently:

1. Decision 7's enforcement moved from a `ValidatingAdmissionPolicy` to the
   controller, because a VAP cannot correlate across objects.
2. Decision 6's wait became optional (`spec.handback` absent means no wait),
   so a consumer with no in-use counter is still gateable.
3. Decision 1 settled the field names, the path-validation rules, and merge
   patch over server-side apply.

**Follow-ups.** The roadmap entry for the integration lives on the consumer
side; 5-Spot carries a `ROADMAPS.md` row pointing at it so the dependency is
visible from both repositories. The CRD shape above was deliberately sketched
when this ADR was Proposed; it is settled as of acceptance (decision 1), and
every later field is additive-only under ADR-0007.
