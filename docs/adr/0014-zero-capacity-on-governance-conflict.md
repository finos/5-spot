<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0014: Drive capacity to zero on a host-governance conflict

- **Status:** Accepted
- **Date:** 2026-10-05
- **Deciders:** Erick Bourgeois
- **Supersedes:** Amends ADR [0011](./0011-schedule-gated-capacity-separate-controller.md)
  decision 7, which said a conflicted object "refuses to write capacity at all"
- **Related:** ADR [0011](./0011-schedule-gated-capacity-separate-controller.md)
  (the `ScheduledCapacity` contract and the one-host-one-governor invariant);
  `docs/src/security/threat-model.md` §6.6 C5 and its MEDIUM residual, which
  this closes; `src/reconcilers/capacity_decision.rs`

## Context

ADR 0011 decision 7 states that a host is governed by `ScheduledMachine` **or**
by `ScheduledCapacity`, never both, because the two are opposite answers to
"what happens to this metal on a schedule": one removes the node, the other
keeps it and shares it. It specified the enforcement as: on detecting the
overlap, set `HostGovernanceConflict=True` and **refuse to write capacity at
all**, failing closed.

Implementing it revealed that "fail closed" does not mean what it sounds like
here. A live test ended in exactly the state the invariant exists to prevent:

```
status.phase                        Error
status.conditions[HostGovernanceConflict]  True
target spec.warmReplicas            10        <- still conceded
```

The conflict was detected *after* a value had already been written, because the
window opened and the node was claimed in the same few seconds. Refusing to
write prevents the controller from making things **worse**, but it also
prevents it from making things **better**: the stale value stands, so the node
is drained for handover by the machine controller while the consumer still
believes it may size guests on it. That is the half-working state decision 7
was written to stop, reached through the door decision 7 left open.

The deeper mistake is that "refuse to write" treats writing as the dangerous
act. For this controller it is not symmetrical. Actuation is a single numeric
field with a fixed zero (ADR 0011 decision 1), so the two directions have
different risk:

| | Writing the active value | Writing zero |
| --- | --- | --- |
| Effect on a node leaving the cluster | concedes capacity on a host that is going away; guests land and then die with it | stops replenishment; claimed work finishes on its own |
| Reversible | yes | yes |
| Destroys in-flight work | no, but it creates work that is about to be destroyed | no: it is a warm-pool target, not a kill |

Zero is not a neutral refusal, it is the **safe direction**. Declining to move
in the safe direction because the object is in a bad state is the error.

### Options

| | Option | For | Against |
| --- | --- | --- | --- |
| A | **Keep "refuse to write".** | Literally fails closed on the write verb; a mis-set `spec.nodeName` can never cost a consumer its capacity. | Leaves the exact contradiction the invariant forbids, indefinitely. The longer the conflict lasts, the longer guests keep landing on a node being drained. Observed, not theoretical. |
| B | **Drive to zero.** *Chosen.* | Removes the contradiction instead of freezing it. Aligned with what the machine controller is already doing to the node. Reversible: fix `spec.nodeName` and the next reconcile restores the active value. | A mis-set `spec.nodeName` that collides with an unrelated machine zeroes capacity the operator wanted. Mitigated below, and recoverable. |
| C | **Delete the `ScheduledCapacity`, or the target.** | Unambiguous. | Ruled out by ADR 0011 decision 2 on principle: this controller never deletes. Also destroys the operator's declared intent over what is usually a typo. |
| D | **Refuse, but emit a Kubernetes Event and alert.** | No write at all; a human decides. | The contradiction still stands until a human acts, which on a draining node is minutes too late. An alert is a good addition, not a substitute. |

B's objection is real but bounded, and the asymmetry decides it. Under A the
failure is silent, indefinite, and exactly the thing the ADR forbids. Under B
the failure is loud, self-correcting on the next reconcile once the operator
fixes the reference, and costs at most a warm pool that refills. A wrong
`spec.nodeName` is also not a guess on our part: the evidence is another
object's `status.nodeRef`, written by the machine controller about a node it
actually owns.

## Decision

**A host-governance conflict drives capacity to zero. It does not refuse to
write.**

1. **Zero is written, and nothing else ever is.** While
   `HostGovernanceConflict` holds, the only value this controller will write is
   `CAPACITY_INACTIVE_VALUE`. The active value is never written to a conflicted
   object, whatever the schedule says, so the "fail closed" intent of decision 7
   is preserved exactly where it matters: a conflicted object cannot concede
   capacity.

2. **The conflict stays loud and stays visible.** Phase remains `Error`,
   `HostGovernanceConflict=True`, `Ready=False`, and the message names the
   conflicting `ScheduledMachine` and the node. The object does not quietly
   become `Inactive`, because it is not inactive by schedule; it is
   misconfigured, and `kubectl get scap` must keep saying so.

3. **No drain wait.** The conflict path does not run the cooperative handback.
   The node is already being drained for handover by the machine controller,
   which owns that drain and its timeout; adding a second clock against the
   same event would be the mistake ADR 0011 avoided by leaving drain where the
   domain knowledge is. `HandbackComplete` is reported `False`, because no
   handback happened: a contradiction was stopped.

4. **Conflict still outranks everything.** It is evaluated before `killSwitch`,
   `spec.enabled` and the provider verdict, as in ADR 0011. The change is only
   to what the branch *does*, not to where it sits.

5. **Recovery is automatic.** When `spec.nodeName` no longer matches any
   `ScheduledMachine.status.nodeRef.name`, the next reconcile is an ordinary
   one and the active value is written again if the schedule says so. No
   operator action beyond fixing the reference, and no state to clear.

## Consequences

**Easier.** The invariant is now enforced rather than merely asserted: a
conflicted host cannot be carrying conceded capacity, whichever order the
conflict and the write arrived in. The MEDIUM residual this opened in the
threat model closes.

**Harder / newly ruled out.**

- A `spec.nodeName` that collides with an unrelated `ScheduledMachine`'s node
  now costs that consumer its warm pool until the reference is corrected.
  `spec.nodeName` is optional precisely so an operator who cannot name the node
  confidently should leave it unset: with it unset the two objects cannot be
  correlated, no conflict is ever detected, and the invariant stays
  documentation only (ADR 0011 decision 7, unchanged).
- The controller now writes to a target it has declared contradictory. That is
  deliberate and bounded to the zero value by decision 1 above, but it does
  mean "the object is in `Error`" no longer implies "the controller is not
  touching the target".
- Option D's Kubernetes Event is **not** adopted here and remains worth doing:
  the condition and the `host_governance_conflicts_total` counter are the
  current signals, and neither pages anyone.

**CALM impact: yes, text only.** No node, relationship, interface or boundary
changes. `flow-schedule-gated-capacity` transition 3 described the refusal and
is corrected to describe the zero write.

**Threat model impact.** §6.6 C5's status changes from "mitigated with a known
gap" to mitigated, and the corresponding MEDIUM residual in §8 is removed
rather than reworded, because the gap is closed rather than accepted. A full
pass is due with this ADR's implementation.
