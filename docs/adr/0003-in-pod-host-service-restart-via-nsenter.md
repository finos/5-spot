<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0003 — In-pod host k0s-service restart via `nsenter` (privileged Kata-config agent)

- **Status:** Accepted
- **Date:** 2026-06-08
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0002](./0002-kata-config-delivery-via-spec-kata.md) (the `spec.kata` CRD contract + workload-cluster resolution this builds on); the reclaim agent (`src/bin/reclaim_agent.rs`, `deploy/node-agent/daemonset.yaml`) as the DaemonSet shape to mirror; `.trivyignore` (reclaim-agent suppression block); `docs/src/security/threat-model.md`.

## Context

ADR 0002 resolves the kata containerd drop-in object on the workload cluster and
stamps the bound Node with an opt-in label plus a reference annotation, but stops
at the kube API. Two steps remain, and neither has any precedent in the codebase:

1. **Write the drop-in to the host filesystem** at the configured `destPath`.
2. **Make containerd reload it.** On k0s, containerd is managed by the host
   `k0sworker.service`; picking up a new `/etc/k0s/container.d/` drop-in requires
   restarting that service. Restarting it bounces containerd and therefore every
   pod on the node — including the agent pod itself.

The reclaim agent — the only existing node-side component — is **read-only on the
host** and **write-only to the kube API**. There is **no host-filesystem write,
no host command execution, no `nsenter`, and no executor abstraction** anywhere
in the tree. So this is net-new architecture, not a mirror of reclaim.

Options weighed for *triggering the host service restart*:

- **(A) Controller-side restart.** Impossible — the controller is on the
  management cluster with no host reach.
- **(B) A Kubernetes `Job` per node, `nodeName`-pinned, that nsenters.** Adds
  Job RBAC, per-restart object lifecycle, garbage collection, and a second
  moving part to reason about. The agent already runs on exactly the right node
  with exactly the right privileges; a Job buys nothing but indirection.
- **(C) A host-side systemd path-unit / drop-in watcher.** Pushes
  responsibility onto host provisioning *outside* 5-Spot, breaking the
  self-contained "SM declares it, 5-Spot delivers it" model and coupling us to
  bespoke node images.
- **(D) In-pod `nsenter -t 1` into host PID 1 (systemd), then `systemctl
  restart`.** Chosen. This is precisely the mechanism upstream `kata-deploy`
  uses, so it is a known-good pattern for this exact problem. It requires the
  agent pod to run `privileged: true` with `hostPID: true`.

The cost of (D) is real: `privileged: true` is a **strict escalation beyond the
reclaim agent**, which runs `runAsUser: 0` + `hostPID: true` + `NET_ADMIN` but is
*not* privileged. `privileged` is required because `setns()` into the host mount
and other namespaces needs it; `hostPID` is required so `nsenter -t 1` resolves
to the host's systemd rather than the container's PID 1.

The other hard problem is the **restart loop**: write → restart → containerd
SIGKILLs the agent → kubelet restarts the agent → agent must *not* write+restart
again. The on-disk content hash already converges this (after the restart the
file matches the source, so no second write), but we add belt-and-braces: before
calling `nsenter`, the agent records the content hash it is about to apply on its
own Node as `5spot.finos.org/kata-config-applied=<sha256>`. On every loop it
compares the source content hash to that annotation and only restarts when the
annotation is missing or stale. This also covers the case where an operator edits
the host file back to old content: the drift path rewrites the file but does
**not** re-restart if the applied hash already matches.

## Decision

We ship a dedicated **privileged node DaemonSet**, `5spot-kata-config-agent`,
that owns the host write and the host-service restart end-to-end. The controller
does **not** orchestrate the restart — its job stays narrow (resolve the object
on the workload cluster, stamp the opt-in label + reference annotation, per ADR
0002).

**Binary + library.** New `src/bin/kata_config_agent.rs` and
`src/kata_config_agent.rs` (I/O-light, unit-testable core), mirroring the
reclaim-agent file layout. A new `5spot-kata-config-agent` `[[bin]]` target in
`Cargo.toml`.

**Agent loop.** The agent reads its `spec.kata` reference off its own Node
annotation (namespace/name/key/destPath/restartService) and consumes the object
via the **workload kube API** (a `get` + `watch`, not a mounted file — a
cluster-wide DaemonSet cannot template a `configMap.name` volume per replica, per
ADR 0002 §3).

1. Resolve the source object (`<namespace>/<name>`, kind ConfigMap or Secret) from
   the workload API and extract `<key>`; compute its SHA-256.
2. Compute SHA-256 of `/host<destPath>` if present.
3. Hashes equal **and** applied-annotation matches → wait for the next watch
   event / poll tick (no-op).
4. Source present, hash differs → **atomic write**: temp file in the dest
   directory + `rename()`, mode `0644`, owner `root:root`.
5. Source absent (object or key gone) → `unlink` the dest file (404 benign).
   GitOps: absent in source ⇒ absent on host.
6. Record `5spot.finos.org/kata-config-applied=<sha256>` (or `=absent`) on the
   agent's own Node via the kubelet node-scoped token, **before** the restart.
7. `nsenter -t 1 -m -u -i -n -p -- systemctl restart <restartService>`.
   Expect to be SIGKILL'd mid-call — systemd still processes the D-Bus job after
   the client dies. On pod restart the hashes match → no-op. Single-cycle
   convergence.

The restart invocation is hidden behind a `RestartExecutor` trait so unit tests
assert the constructed `nsenter` command line without ever executing the real
binary; the concrete implementation is exercised only in integration tests.

**DaemonSet (`deploy/kata-config-agent/`).** `namespace: 5spot-system`;
`nodeSelector: { 5spot.finos.org/kata-config: enabled }` (label-gated opt-in
from ADR 0002); `priorityClassName: system-node-critical`; `tolerations: [{
operator: Exists }]`. Pod `securityContext`: `runAsUser/Group: 0`,
`runAsNonRoot: false`, `hostPID: true`, `seccompProfile: RuntimeDefault`.
Container `securityContext`: `privileged: true`, `readOnlyRootFilesystem: true`,
`seccompProfile: RuntimeDefault`. Volumes: hostPath `/` mounted at `/host` (the
dest path is user-configurable, so we cannot pin a `subPath`). There is **no**
ConfigMap volume — the source is read from the workload API, keyed by the Node
annotation. Env: `NODE_NAME` (downward API); `destPath` / `restartService` /
`key` are read from the annotation, with the binary defaults as fallback.

**Applied-hash annotation** `5spot.finos.org/kata-config-applied` is the
restart-loop guard; the existing node `patch` grant already covers it (no new
controller RBAC, no Job RBAC). The agent gets its own workload-cluster
ServiceAccount + narrow Role mirroring the reclaim agent's: `configmaps` (and
`secrets`, for `Secret` sources) `get/watch` in its target namespace (default
`5spot-system`, per `spec.kata.namespace`), plus node-scoped self-patch.

**Metrics.** `kata_config_writes_total`, `kata_config_deletes_total`,
`kata_config_drift_corrected_total`, `kata_config_last_sync_timestamp_seconds`.
(Note: unlike the reclaim agent, which emits no agent-side metrics, this agent
does — there is no existing agent-side metrics scaffold to copy.)

**Security justification.** `privileged: true` + `hostPID: true` are documented
in `.trivyignore` under a new `kata-config-agent` banner block following the
reclaim-agent format (rule-ID + written architectural rationale), and called out
in `docs/src/security/threat-model.md`. Mitigations of record: the agent lands
**only** on nodes the controller has label-gated (which only happens when an SM
with `spec.kata` binds that node), `readOnlyRootFilesystem: true`, and a
narrow node-scoped RBAC footprint.

## Consequences

- **Easier:** single-cycle, self-healing delivery of kata config to the exact
  node — write, restart, converge — owned entirely by one node-resident agent
  with no controller-side restart orchestration to coordinate.
- **Harder / riskier:** this is the **first host-filesystem write and first host
  command execution** in 5-Spot, and the **first `privileged: true` workload**.
  That privilege is a genuine attack-surface increase, justified only by being
  opt-in, label-gated, read-only-rootfs, and node-scoped. It must be reviewed as
  such in the regulated context and re-validated whenever the agent changes.
- **Open questions to resolve in implementation/e2e (Phase 5 of the roadmap):**
  (1) k0s service-name autodetection — `k0sworker.service` vs
  `k0scontroller.service`; ship default + `restartService` override, autodetect
  as follow-up. (2) First-provision always restarts (no prior config) — expected
  but documented so operators aren't surprised by the initial containerd bounce.
  (3) Double-restart interaction with `kata-deploy` if both write on the same
  node — verify experimentally. (4) `/etc/k0s/container.d/` drop-in path
  stability across k0s minor upgrades — `destPath` being configurable insulates
  us; pin a validated k0s version range in the guide.
- **Ruled out:** controller-side restart (no host reach), per-node Job
  indirection, and host-side systemd watchers that push setup outside 5-Spot.
- **CALM impact:** **updated.** Adds `service-kata-config-agent`'s security
  controls (least-privilege, container-hardening noting the privileged
  posture), the `rel-kata-agent-writes-host` relationship (hostPath write +
  `nsenter` restart against `network-physical-node`) with its host-isolation
  control, the `rel-kata-agent-workload-kube-api` relationship (applied-hash
  self-annotation), and the host-write/restart transitions of
  `flow-kata-config-delivery`.
