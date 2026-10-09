<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0016: 5-Spot creates and owns the capacity object it scales

- **Status:** Accepted
- **Date:** 2026-10-08
- **Deciders:** Erick Bourgeois
- **Supersedes:** ADR-0011 decisions 1 (the `targetRef` / `capacity.path`
  fields), 2 (actuation is a field write, never a create) and 5 (the
  allowlist's verb). The rest of ADR-0011 stands: the separate CRD, the
  separate binary and identity, the reused schedule verdict, the bounded
  cooperative handback, and its own phase and condition set.
- **Related:** ADR [0011](./0011-schedule-gated-capacity-separate-controller.md)
  (the decision this narrows), ADR
  [0014](./0014-zero-capacity-on-governance-conflict.md) (the conflict branch,
  which survives unchanged), ADR
  [0007](./0007-crd-multi-version-and-conversion.md) (the additive-only rule
  this change breaks on purpose, and why its webhook trigger does not fire),
  roadmap
  [05](../../.github/community/05-schedule-gated-capacity.md)

## Context

ADR 0011 decided that `ScheduledCapacity` would **patch one numeric field on an
object somebody else owns**, and built an apparatus around that premise:
`spec.targetRef` naming a consumer's object, `spec.capacity.path` as a
validated JSON path into it, a dotted-path validator treated as a security
control, a merge patch rather than server-side apply so 5-Spot would not become
a co-owner of a field under someone else's GitOps manager, and an allowlist of
API groups whose only verb is `patch`.

Implementing it to the point of a kind end-to-end test is what surfaced the
problem. The stub target CRD had to be written, the group allowlist had to be
widened to admit it, and the exercise made the shape visible: 5-Spot was
reaching into a stranger's object to turn a dial, and most of the design was
apparatus for making that reach safe.

That is not how the rest of 5-Spot works. `ScheduledMachine` does not patch a
`Machine` somebody else created; it **creates the `Machine`, owns it, and
deletes it**. The schedule is the reason the object exists. Capacity should
read the same way: when the window is open there is a pool of spot microVMs
because 5-Spot put one there, and when it closes that pool holds nothing.

### ADR 0011's argument against create-and-delete, re-examined

The ADR's reasoning was explicit and is worth quoting against itself:

> A `VirtualMachinePool` with live claims is not [safe to delete]. An agent
> mid-task has no analogue of pod eviction that 5-Spot understands, and the
> consumer already owns those semantics. Deleting the pool at market open would
> pull the rug on claim holders using knowledge this controller does not have.

The reasoning is sound and the premise is false. The consumer already solved
it, and says so in its own code. Verified in the sibling repository, not
assumed:

| Claim to check | Finding | Where |
| --- | --- | --- |
| Does deleting a pool destroy work in progress? | **No.** A bound member's `ownerReferences` are re-parented from the pool to the claim, by merge patch, at bind time, with the comment "the member now lives and dies with the claim, not with the pool. Deleting the pool must not destroy a sandbox somebody is using". | `banlieue-controller/src/reconciler/claim.rs:265` |
| Is that an accident of implementation? | **No**, it is a recorded decision (its ADR-0047 decision 3) and the pool reconciler's module doc states the resulting invariant: unclaimed members go with the pool through `ownerReferences` and background GC, claimed members survive it. | `banlieue-controller/src/reconciler/pool.rs:60` |
| Would 5-Spot writing the pool's `spec` fight the consumer's controller? | **No.** The pool reconciler only ever calls `patch_status`. It never writes `spec`. | `banlieue-controller/src/reconciler/pool.rs:507` |
| Does anything read the **pool's own** `ownerReferences`, where a 5-Spot-set owner could be mis-handled? | **No.** The only write of a pool-scoped owner reference is the pool setting *itself* as owner of a member. | `grep` across the consumer's crates |
| Is there a finalizer on the pool to deadlock against? | **No.** |  |
| Does an admission policy gate pool creation? | **No.** The consumer's `deploy/admission/` constrains `Provider`, `ProviderClass`, `VirtualMachine`, `VirtualMachineClaim` and `VMImage`. There is no pool policy. | `deploy/admission/` |

So the thing ADR 0011 was most afraid of is handled on the consumer side, by
design, and the asymmetry it reasoned from does not exist. Deleting a pool
takes the **idle** members, which is exactly the capacity being reclaimed, and
leaves the claimed ones to finish and expire under their own TTL.

### What patching a stranger's object actually cost

With the premise corrected, the apparatus turns out to be the expensive half:

- **`capacity.path` is a confused-deputy primitive.** A CR author who can
  create a `ScheduledCapacity` chooses a field path that 5-Spot's
  ServiceAccount then writes on an object the author may have no access to.
  Every rejection in the validator (`metadata.` first among them) exists to
  bound that. The threat model carries it as its own boundary.
- **The one-host-one-governor conflict** (ADR 0011 decision 7, resolved in ADR
  0014) is sharper than it needs to be *because* the object is shared: two
  governors writing one stranger's field.
- **Merge patch over server-side apply** was forced by the shared object. SSA
  is the better tool and was unavailable.

Owning the object removes all three, because there is no shared object. The
remaining question is not "how do we write safely into someone else's spec" but
"what does 5-Spot have to be told in order to construct the whole spec".

### Options

| | Option | For | Against |
| --- | --- | --- | --- |
| A | **Supersede: 5-Spot creates and owns the capacity object; scale to zero at window close; let garbage collection remove it when the `ScheduledCapacity` is deleted.** *Chosen.* | Symmetric with `ScheduledMachine`. No shared object, so no path validator as a control, no co-ownership, no confused deputy. SSA becomes correct. The object's lifetime is visibly the schedule's. | 5-Spot needs `create` on the target group, a strictly larger grant than `patch`, inverting ADR 0011's central security claim. 5-Spot must be told the whole spec, which is an opaque pass-through. |
| B | **Keep patching a consumer-owned object.** | Smallest possible grant. Already implemented. | Keeps the confused-deputy surface and the path validator that exists to bound it, keeps 5-Spot a writer inside a stranger's spec, and rests on a premise now shown to be false. |
| C | **Own the object, and delete it at every window close rather than scaling to zero.** | The strongest statement of "no window, no capacity": nothing is left behind at all. | Churns the pool identity and its template every day, discards the consumer's warm-image state, and makes every reopen a cold create. Scaling to zero reaches the same capacity with none of that. |
| D | **Own the object, and keep `delete` in the RBAC to remove it on CR deletion.** | Explicit, no reliance on another controller. | `delete` on a group cannot be scoped to "objects you created". A compromised capacity controller could delete a consumer-owned pool. Owner-reference GC achieves the same removal with no `delete` verb at all. |

## Decision

**`ScheduledCapacity` creates the capacity object, owns it, and scales it. It
never patches an object it did not create.**

1. **The owned object replaces `targetRef`.** `spec.target` declares only the
   `apiVersion` and `kind`; there is no `name`, because the owned object takes
   the `ScheduledCapacity`'s own name in the `ScheduledCapacity`'s own
   namespace. A name that cannot diverge cannot orphan a previously owned
   object. `spec.target` is **immutable** (CEL `self == oldSelf`): changing the
   kind would orphan the object of the old kind, and per ADR 0007 a field is
   made immutable rather than removed later.

2. **The spec is supplied as an opaque template.** `spec.template` is a
   pass-through object that becomes the owned object's `spec`, using the
   existing `EmbeddedResource` pattern that `ScheduledMachine.spec.bootstrapSpec`
   already uses. 5-Spot does not and will not model the consumer's schema. The
   worked example needs `maxReplicas`, `readiness` and `template`, all of which
   are the consumer's business, and one of which is a nested VM template.

3. **5-Spot injects exactly one field into that template: the capacity knob.**
   `spec.capacity.field` is a dotted path **rooted at the owned object's
   `spec`**, and `spec.capacity.activeValue` is what goes there while the
   window is open. `CAPACITY_INACTIVE_VALUE` (0) goes there when it is not.
   `spec.template` may not itself contain that path: one source of truth, or
   the two fight on every reconcile.

   The path validator survives, reduced, and is **structurally** stronger than
   ADR 0011's. `metadata.` and `status.` are no longer rejected by a string
   check; they are unreachable, because the path is rooted at `spec` by
   construction. What remains is charset, depth and shape, which is schema
   hygiene rather than a security control.

4. **Actuation is server-side apply, with field manager
   `5spot-capacity-controller`.** ADR 0011 chose merge patch to avoid becoming
   a permanent co-owner of a field under the consumer's GitOps manager. There
   is no other writer now: 5-Spot is the sole owner of this object's `spec`, and
   the consumer's reconciler writes only `status`. A writer that applies the
   **complete** set of fields its manager owns is idempotent under SSA, which is
   precisely the condition the sibling project's ADR-0087 identified; 5-Spot
   meets it, so SSA is correct here and brings pruning with it: removing a field
   from `spec.template` removes it from the owned object.

5. **Scale to zero at window close. Never delete on a schedule.** Window close
   writes the inactive value and runs the bounded cooperative handback of ADR
   0011 decision 6 unchanged, so claims drain under the consumer's own
   semantics. The object, its identity and its warm-image state persist to the
   next window, and a reopen is a scale rather than a cold create.

6. **Removal is garbage collection, not a `delete` verb.** The owned object
   carries a controller `ownerReference` to the `ScheduledCapacity`. Deleting
   the `ScheduledCapacity` therefore removes it through Kubernetes GC, under
   kube-controller-manager's identity, and **the capacity controller is granted
   no `delete` on the target group at all.** This is the point of option D
   above: RBAC cannot express "only objects you created", and owner-reference
   GC does not need it to, because GC only ever removes objects that actually
   carry our reference. No finalizer: the cascade is safe without one, since the
   consumer re-parents claimed members to their claim and only idle members go
   with the pool.

7. **The allowlist stays, and its verbs become `create` and `patch`.**
   `ALLOWED_CAPACITY_TARGET_API_GROUPS` is unchanged in purpose: the set of API
   groups 5-Spot will construct an object in, in one greppable constant,
   re-checked in the reconciler so a stale CRD cannot loosen it. The pre-flight
   `SelfSubjectAccessReview` checks **both** verbs, because server-side apply
   needs `create` when the object is absent and `patch` when it is present, and
   a controller holding only one of them would work until the first window and
   then stop. Checking the pair is what makes the pre-flight's promise true: an
   RBAC gap is a condition, not a 403 partway through actuation.

8. **ADR 0014 stands.** A host-governance conflict still writes nothing but
   zero, still stays loud, and still skips the drain wait. One clarification it
   now needs: on the conflict path the owned object is **not created** if it
   does not yet exist. A misconfigured `ScheduledCapacity` produces no foreign
   object at all, which is the fail-closed reading of decision 7 taken one step
   further than ADR 0014 could take it against a stranger's object.

## Consequences

**Easier.**

- The design reads like the rest of the project. Both controllers now create
  what the schedule is the reason for, and `kubectl get` shows an object whose
  existence is the schedule's doing.
- The confused-deputy class goes away with the user-supplied field path into a
  stranger's object. The threat model's capacity boundary loses its two
  headline threats; what replaces them is ordinary "we create a foreign
  resource from an opaque template", which the tree already carries for
  `bootstrapSpec`.
- SSA with pruning means `spec.template` is declarative. Under merge patch a
  field removed from the template would have lingered on the target forever.
- One fewer coupling to a consumer's installed state: there is no object that
  must already exist for a `ScheduledCapacity` to do anything, so the kind
  end-to-end test needs only the stub CRD, not a stub object with a hand-built
  schema that a real consumer would have rejected.

**Harder / newly ruled out.**

- **`create` is a larger grant than `patch`, and that inverts ADR 0011's
  central security argument.** It is still confined to a separate identity with
  no CAPI verbs and no Secret access, which was the more important half of
  decision 3, and it is still bounded to the allowlisted groups. But a
  compromised capacity controller can now construct arbitrary objects of an
  allowlisted kind rather than move one integer. This is a real widening and
  belongs in the threat model as such, not as a footnote.
- **`spec.template` is opaque, so it inherits the `bootstrapSpec` residual.**
  Whatever the consumer's schema admits, a `ScheduledCapacity` author can put
  there, and 5-Spot's ServiceAccount creates it. That is the same HIGH residual
  the tree already records for embedded bootstrap specs, with the same
  mitigation (the group allowlist plus the consumer's own admission policies,
  of which the worked consumer has several on the objects the pool creates).
  It is not a new class of risk, but it is a second instance of one, and the
  threat model must stop describing it as singular.
- **Deletion correctness now depends on the consumer's re-parenting.** The
  cascade is safe because claimed members escape it. That is a verified
  property of one consumer, recorded above with a file and line, and it is
  **not** a contract the spot-schedule reference states. A consumer that owns
  its in-use members from the pool would lose work on `ScheduledCapacity`
  deletion. Follow-up: state it as a requirement on capacity consumers in
  `docs/src/reference/`, so it is a published expectation rather than an
  observation about one repository.
- **This is a breaking change to a served version, and it is stated as one.**
  `spec.targetRef` is **required** in `5spot.finos.org/v1alpha1` as shipped in
  **v0.3.2**, so withdrawing it is a removal from a released API, not a
  pre-release correction. ADR 0007 constrains cross-version changes to additive
  only, and names a true breaking change as the trigger for a conversion-webhook
  ADR. That trigger does not fire here, for a specific reason: it exists for
  serving two versions that are not round-trippable under
  `conversion.strategy: None`, and `ScheduledCapacity` serves exactly **one**
  version. There is nothing to convert between. The alternative, serving
  `v1alpha2` alongside `v1alpha1`, is the case ADR 0007 forbids without a
  webhook, because `targetRef` has no counterpart in the new shape and the two
  cannot round-trip. In-place is therefore both the smaller change and the only
  one ADR 0007 permits webhook-free.

  What it costs: an existing `ScheduledCapacity` stored under the old schema
  becomes inert rather than converted. `targetRef` is pruned on the next write
  and the controller finds no `spec.target`, so the object must be **deleted and
  recreated**. The CHANGELOG entry carries `[x] Breaking change` with the
  recreate instruction, and the release notes repeat it, matching the precedent
  this repo already set when `ScheduledMachine` manifests last had to be
  rewritten. Three days of availability on an explicitly unstable alpha is the
  proportionality argument, not an exemption.

- **Rework, not addition.** `capacity_path.rs` shrinks, the reconciler's
  resolve step becomes a create-or-apply step, the `ClusterRole` changes verbs,
  the print column `.spec.targetRef.kind` becomes `.spec.target.kind`, and ADR
  0011's decisions 1, 2 and 5 are superseded in place.

**CALM impact: yes.** The capacity target stops being an external `data-asset`
that 5-Spot reaches into and becomes a resource the controller owns, so the
relationship direction and the "writes an API group it does not own" trust
boundary both change. `make calm-validate` and `make calm-diagrams` run with
the implementation.

**Threat model impact: yes, substantial, in both directions.** The capacity
boundary loses the crafted-write-path threats and gains a
construct-a-foreign-object threat; the embedded-spec residual gains a second
instance; the RBAC posture section changes from `patch` to `create` plus
`patch` with no `delete`, and the reason there is no `delete` is itself a
control worth recording. A full pass is due with the implementation, per
`.claude/rules/threat-modeling.md`.
