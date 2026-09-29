<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0013 — The `kata-config-ref` annotation is writable by the controller only, enforced by admission

- **Status:** Accepted
- **Date:** 2026-09-27
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0002](./0002-kata-config-delivery-via-spec-kata.md) (the
  annotation contract), ADR
  [0012](./0012-kata-agent-validates-its-own-input.md) (which narrowed this
  threat but could not close it), ADR
  [0004](./0004-agent-pod-security-exception-boundary-vap.md) (the VAP pattern
  this follows), ADR
  [0003](./0003-in-pod-host-service-restart-via-nsenter.md);
  [Threat Model](../src/security/threat-model.md) §6.5 K5 and §8.

## Context

`5spot.finos.org/kata-config-ref` is not data. It is an **instruction**, written
by the controller onto a Node, and consumed by a privileged agent that acts on it
by writing the host filesystem and restarting a host systemd unit.

ADR-0012 made the agent distrust the annotation's *shape*: every field is now
re-validated against the constraint its CRD counterpart enforces, and a
malformed value is refused rather than executed. It could not address the
annotation's *authority*. Both node-side agents hold `nodes: patch`
**cluster-wide**, because RBAC cannot express "the Node this pod runs on", so a
compromised agent on node A can write a well-formed `kata-config-ref` onto node
B and have **B's** agent fetch an attacker-named object, write it to
`/etc/k0s/containerd.d/kata.toml`, and restart a unit there. The agent is
already node-root on A; this is the step from A to B, and it goes through the
control plane using a grant that looks innocuous in review.

The threat model records this as K5 (residual) and as the §8 MEDIUM
*"Node-side agents hold cluster-wide `nodes: patch`"*, whose recommendation is
exactly this ADR.

**Why not tighten RBAC instead.** There is no `resourceNames` that means "self"
for a Node, and `nodes` is cluster-scoped so a namespaced Role cannot help.
Per-node ServiceAccounts with `resourceNames: [<node>]` would work and were
rejected: they require an identity per node, created and garbage-collected by
something, which is a lifecycle system in exchange for a bound that admission
gives for nothing.

**Why deny the agents rather than allowlist the controller.** The obvious policy
is "only the controller may write this key", but the controller's identity *in
the workload cluster* is deployment-specific: it authenticates with the
`kubeconfig-<clusterName>` Secret (ADR-0002), so its username there is whatever
that kubeconfig carries — commonly a cluster-admin user, not
`system:serviceaccount:5spot-system:5spot-controller`. An allowlist would
therefore need a parameter ConfigMap naming the local identity, configured per
install, and fails closed on a legitimate write if it is wrong. The **agents'**
identities, by contrast, are ours: they come from ServiceAccounts in our own
manifests, with stable names. Denying exactly them is deployment-agnostic,
needs no configuration, and bounds the attacker the threat model actually
describes.

## Decision

**`deploy/admission/kata-config-annotation-policy.yaml` adds a
`ValidatingAdmissionPolicy` to the *workload* cluster that rejects any `UPDATE`
to a Node which changes `5spot.finos.org/kata-config-ref` when the requesting
identity is one of the 5-Spot node agents.**

1. **Identity comes from `request.userInfo.username`**, not from a pod spec.
   ADR-0004's policy validates Pods and reads `object.spec.serviceAccountName`;
   this one validates Nodes, where the only identity available is the requester.
   The matched usernames are the agents' ServiceAccounts in the form
   `system:serviceaccount:5spot-system:5spot-{kata-config,reclaim}-agent`.

2. **Only a *change* is rejected.** The expression compares
   `object` against `oldObject`, so an agent patching `kata-config-applied` — its
   own job, on the same object, usually in the same request — is untouched. A
   policy that rejected any Node update from an agent would break the applied-hash
   guard and with it the restart-loop protection.

3. **`failurePolicy: Fail`,** matching every other policy in `deploy/admission/`.
   An unavailable policy must not open the boundary it exists to close.

4. **`validationActions: [Deny]`,** with the file documenting `[Audit]` as the
   rollout option, mirroring ADR-0004's binding.

5. **Two test layers, because they prove different things.**
   - A **unit test** asserts the manifest's strings against the Rust constants.
     The policy hardcodes the annotation key and the ServiceAccount names;
     nothing else stops a rename in `constants.rs` from silently leaving the
     policy matching a key that no longer exists. This repository has been
     bitten by exactly that class of drift before (ADR-0041's comment that
     claimed a namespace scope the YAML did not grant).
   - A **behavioural suite** (`.github/scripts/admission-deny.bats`, run by
     `make kind-verify-admission` and the `Admission (deny tests)` workflow)
     proves the policy *fires*, against a real API server. A CEL typo, a wrong
     `matchConstraint` or a binding that never applied all fail **open** and
     silently — the manifest applies, the policy exists, and nothing is
     rejected. The suite grants the impersonated identities `patch nodes`
     first, precisely so a denial cannot be an authorization failure wearing
     admission's clothes, and asserts the policy's own message text to prove
     which layer refused. It also asserts the *positive* paths: the agents can
     still write `kata-config-applied`, and a non-agent identity can still set
     `kata-config-ref` — without those, a policy that rejected every Node
     update from an agent would pass the negative tests and break the
     restart-loop guard.

## Consequences

**Easier.** K5's authority half closes for the attacker it was written about: a
compromised agent can no longer retarget another node, so agent compromise stays
on one machine. The §8 MEDIUM residual narrows to "anything *else* holding
cluster-wide `nodes: patch`", which is a much smaller and more honest statement.

**Harder / accepted costs.**

- **It is not a complete answer to `nodes: patch`.** A human admin, a CI
  identity, or any other tooling with that verb can still write the key. The
  policy bounds the *agents*, which is what the lateral-movement path needs; the
  broader grant remains an operator concern, and §7's audit recommendation still
  stands.
- **A cluster that skips `deploy/admission/` gets none of this.** Already true
  of every control in §6/TB-1 and stated in §7, requirement 5.
- **The policy must be installed in the workload cluster**, like ADR-0004's, not
  the management cluster. A misplaced apply is a silent no-op — the manifest says
  so in its header, which is the same failure mode ADR-0004 already carries.
- Renaming an agent ServiceAccount now breaks admission unless the policy is
  updated with it. The unit test in decision 5 turns that from a production
  surprise into a failing build.

**CALM impact: yes.** A control on the controller-to-Node annotation
relationship (`rel-controller-kata-config-projection`), which now has an
admission gate around it. No new node, relationship or flow.

**Threat model impact.** K5 moves from residual to mitigated for the agent case,
with the remaining grant named. The §8 MEDIUM entry narrows rather than closes.
A full pass runs with the implementation, per `rules/threat-modeling.md`.
