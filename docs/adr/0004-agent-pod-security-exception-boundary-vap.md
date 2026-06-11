<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0004 — Agent pod-security exception boundary via deny-by-default ValidatingAdmissionPolicy

- **Status:** Accepted
- **Date:** 2026-06-10
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0003](./0003-in-pod-host-service-restart-via-nsenter.md) (the privileged kata-config agent this boundary sanctions); ADR [0005](./0005-confine-kata-destpath-to-etc-k0s.md) (complementary containment *inside* the kata agent's hostPath `/` mount — 0005 confines what the agent writes, this ADR confines who may carry the mount at all); the reclaim agent (`deploy/node-agent/daemonset.yaml`); `deploy/admission/` (existing ValidatingAdmissionPolicy posture); `.trivyignore`; `docs/src/security/threat-model.md`.

## Context

5-Spot ships two node-side DaemonSets on the **workload cluster**, both in
`5spot-system`, whose security posture deliberately exceeds the Kubernetes
*restricted* (and parts of the *baseline*) Pod Security Standard:

| Attribute | `5spot-kata-config-agent` | `5spot-reclaim-agent` |
|---|---|---|
| `privileged: true` | **yes** (`setns` for the `nsenter` k0s restart, ADR 0003) | no |
| `hostPID: true` | **yes** (`nsenter -t 1` → host systemd) | **yes** (scan host `/proc`) |
| root (`runAsUser: 0`, `runAsNonRoot: false`) | **yes** (write `/etc/k0s` on host) | **yes** (read every `/proc/<pid>`) |
| `hostPath` volumes | `/` (RW — `destPath` is operator-configurable) | `/proc`, `/etc/machine-id` (RO) |
| added capabilities | none | `NET_ADMIN` (netlink proc connector) |

Each attribute is architecturally required and individually justified
(`.trivyignore`, threat model, ADRs 0002/0003). The problem is **deployment
into clusters with admission-enforced pod-security baselines** — Pod Security
Admission (PSA), OPA Gatekeeper, or Kyverno — which is the norm in the
regulated environments 5-Spot targets. Those engines will deny the agent pods
outright, and the kubelet/DaemonSet controller will silently fail to land them.

A mechanical constraint shapes the whole solution space: **Kubernetes admission
is conjunctive.** Every admission plugin must admit a request; any single deny
is final. A `ValidatingAdmissionPolicy` therefore **cannot "allow" or override**
a deny issued by PSA, Gatekeeper, Kyverno, or any other webhook. There is no
such thing as an "allow policy" in the admission chain. The exemption for the
agents *must* be granted inside whichever engine enforces the baseline:

- **PSA** — label the namespace `pod-security.kubernetes.io/enforce: privileged`
  (or list it in the API server's `AdmissionConfiguration` exemptions).
- **Gatekeeper** — add `5spot-system` to the constraints' `excludedNamespaces`
  (or a `config.gatekeeper.sh/v1alpha1 Config` excluded-namespaces entry).
- **Kyverno** — add an `exclude` block for the namespace (or the two
  ServiceAccounts) to the relevant `ClusterPolicy` rules.

All of these exemptions are **namespace-wide blunt instruments**: once
`5spot-system` is exempted, *anything* deployed there — a compromised CI
pipeline, a typo'd Deployment, an attacker with namespace-scoped `create pods`
— could run privileged, mount the host root, or grab capabilities, with no
admission check at all. That residual hole is what this ADR closes.

Options weighed:

- **(A) Document the exemption, ship nothing.** Leaves `5spot-system`
  admission-unguarded after exemption. Unacceptable in a regulated context: the
  exemption rationale ("only our two agents need this") would be enforced by
  nothing.
- **(B) Ship Gatekeeper ConstraintTemplates / Kyverno policies for the
  guardrail.** Couples 5-Spot to a specific third-party policy engine the
  cluster may not run; we would need one artifact per engine, kept in sync.
- **(C) Ship a deny-by-default `ValidatingAdmissionPolicy` scoped to
  `5spot-system` that re-imposes the baseline for everything except the two
  known agent identities, pinned to their exact, minimal posture.** Chosen.
  VAP is in-tree (GA since 1.30, same floor as our existing
  `scheduledmachine-validation` policy), engine-neutral, CEL-auditable, and
  follows the established `deploy/admission/` pattern.
- **(D) Per-workload PSA exemption.** PSA has no per-pod/per-SA exemption
  mechanism (only usernames/runtimeClasses/namespaces at API-server config
  level); rejected as not expressible.

## Decision

We ship a **compensating, deny-by-default `ValidatingAdmissionPolicy`**
(`5spot-agent-pod-security`, `deploy/admission/agent-pod-security-policy.yaml`
+ binding) applied to the **workload cluster**, matching `CREATE`/`UPDATE` of
pods **only in `5spot-system`** (binding-scoped via `namespaceSelector`). The
baseline engines' namespace exemption and this policy are a **paired
deployment**: the exemption opens exactly one door, this policy stands behind
it.

The policy is an **identity-pinned allowlist**. Pod identity is the
ServiceAccount (`5spot-kata-config-agent` / `5spot-reclaim-agent` — already
bound to narrow, dedicated RBAC). For each risky attribute the policy denies
use by any other identity, and clamps the agents to exactly what their
manifests declare:

1. `hostPID` — only the two agents.
2. `hostNetwork`, `hostIPC` — denied for **everyone** (no agent uses them).
3. `privileged` — only the kata-config agent.
4. `hostPath` volumes — kata agent: `/` only; reclaim agent: `/proc` and
   `/etc/machine-id` only; everyone else: none.
5. Added capabilities — reclaim agent: `NET_ADMIN` only; everyone else: none.
6. Explicit root (`runAsUser: 0` / `runAsNonRoot: false`) — only the two agents.
7. **Compensating controls become mandatory** for the agents: any privileged
   container must keep `readOnlyRootFilesystem: true`, and agent pods must keep
   `seccompProfile.type: RuntimeDefault`. The exception is conditional on the
   mitigations that justified it.

`failurePolicy: Fail` — this is a security boundary; it fails closed. The
binding ships `validationActions: [Deny]`, with `[Deny, Audit]` documented for
rollout observation. The exemption recipes for PSA / Gatekeeper / Kyverno are
documented in the policy header and in `docs/src/security/admission-validation.md`.

The management cluster needs no counterpart: the controller Deployment is
non-root/restricted-compliant and already guarded by
`5spot-controller-deployment-validation`.

## Consequences

- **Easier:** 5-Spot can be deployed into PSA/OPA/Kyverno-guarded clusters with
  a *narrow, enforced* exception instead of an unguarded namespace exemption —
  the "only our two agents, only their exact posture" claim becomes machine-
  enforced and auditable (NIST AC-6, CM-5 evidence).
- **Harder:** the agent manifests and this policy must evolve **together** —
  adding a capability, a hostPath, or a new agent requires updating the CEL
  allowlist in the same change (and re-justifying in `.trivyignore` / threat
  model). The policy pins ServiceAccount names; renaming an agent SA is now a
  two-file change.
- **Residual risk accepted:** identity is the ServiceAccount, so a principal
  holding `create pods` in `5spot-system` *and* the right to use an agent SA
  could wear the exception; RBAC on those SAs is the control (they are bound
  only to the DaemonSets). The policy clamps the blast radius to the agents'
  already-documented posture even then.
- **Ruled out:** engine-specific guardrails (Gatekeeper/Kyverno artifacts),
  and any notion of an "allow/override" policy — admission stays conjunctive;
  exemptions live in the engine that owns the deny.
- **Requires:** Kubernetes ≥ 1.30 on the workload cluster (VAP GA), consistent
  with the existing admission posture.
- **CALM impact:** **updated.** Adds the `agent-pod-security-boundary` control
  on the workload-cluster API server node, citing this policy as evidence and
  covering both agent relationships.
