<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0002 — Kata config delivery via `spec.kata`: workload-cluster resolution, fail-fast on absence

- **Status:** Accepted
- **Date:** 2026-06-09
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0003](./0003-in-pod-host-service-restart-via-nsenter.md) (the
  node-side host-write + `nsenter` restart half of this feature);
  `child_clients.resolve` (`src/reconcilers/child_client.rs`);
  `KubeconfigSecretRef` (`src/crd.rs`); the reclaim-agent projection
  (`reconcile_reclaim_agent_provision`) as the opt-in-label shape to mirror;
  Trivy **KSV-0049**.

## Context

5-Spot's target nodes are k0s workers whose containerd runtime consumes drop-in
config from `/etc/k0s/container.d/`. To run Kata-isolated workloads, a specific
kata containerd drop-in (e.g. `/etc/k0s/container.d/kata-containers.toml`) must
land on the worker node(s) a given `ScheduledMachine` owns, reconciled the GitOps
way (present in source ⇒ present on host; removed ⇒ removed).

**An initial design was rejected.** The first cut had the controller *project a
per-node `ConfigMap`* (`kata-config-<node>`) onto the workload cluster, forking
the source content per node. Reviewed against the platform model, this was wrong:

- **Per-node fan-out is the wrong granularity** — two SMs pointing at the same
  source produce byte-identical copies; cardinality is O(opted-in nodes) with
  full duplication.
- **It creates a sync burden 5-Spot shouldn't own** — every source edit forces
  re-reconciling all N copies.
- **It mis-assigns ownership** — the config is platform-owned and GitOps-managed;
  the controller forking and deleting copies fights that ownership.

The platform model that drives the accepted design:

- **Tenants create nothing.** The platform owner manages kata config in Git;
  **Flux** reconciles it onto the workload clusters. 5-Spot treats it as a
  read-only input, never the author.
- **A tenant namespace backs one *or more* clusters / control planes.** In the
  k0smotron topology a namespace (e.g. `foo`) on the management cluster hosts one
  or more child control planes, each addressed by a `kubeconfig-<clusterName>`
  Secret (CAPI convention). The `ScheduledMachine`'s `clusterName` + that Secret
  tell the controller which workload cluster a bound node lives on.
- **The agent runs on the workload cluster**, and the controller **already has
  workload-cluster access** via `child_clients.resolve`. The cross-cluster
  boundary that an initial design used to justify controller-side projection is a
  *non-issue* — the controller can read the workload cluster with an existing
  credential.

**Design principle adopted: 5-Spot writes nothing for kata delivery.** The
delivered artifact is whatever Flux has already placed on the workload cluster.
5-Spot's only job is to *resolve* it, *opt the node in*, and — if it is not there
— **fail fast with a clear status reason**. 5-Spot does **not** copy objects,
does **not** create namespaces, and does **not** manufacture per-node ConfigMaps.
Things must exist for it to work; absence is a reported failure, not something
5-Spot papers over by creating resources.

## Decision

### 1. `spec.kata` on `ScheduledMachine`

`src/crd.rs` is the source of truth; the CRD YAML and API docs are regenerated
(`regen-crds` → `regen-api-docs`), never hand-edited.

```rust
pub kata: Option<KataConfig>,                 // on ScheduledMachineSpec

pub struct KataConfig {
    pub kind: KataConfigSourceKind,           // ConfigMap | Secret  (both supported)
    pub name: String,                         // object name on the WORKLOAD cluster
    pub key: Option<String>,                  // default: kata-containers.toml
    pub dest_path: Option<String>,            // default: /etc/k0s/container.d/kata-containers.toml
    pub restart_service: Option<String>,      // default: k0sworker.service
    pub namespace: Option<String>,            // target ns on the WORKLOAD cluster;
                                              // default: 5spot-system (the agent's namespace)
}

pub enum KataConfigSourceKind { ConfigMap, Secret }
```

`KataConfig` keeps the `KubeconfigSecretRef` derive/serde conventions
(`deny_unknown_fields`, camelCase, schemars bounds). There is **no**
`createNamespace` and **no** `namespaceMetadata` — 5-Spot never provisions a
namespace. Both `ConfigMap` and `Secret` sources are supported symmetrically.

### 2. Resolution: workload-cluster only, fail-fast on absence

For an SM with `clusterName: cluster-1` and target namespace `T`
(`spec.kata.namespace`, default `5spot-system`):

1. Resolve the workload client via `kubeconfig-cluster-1`
   (`child_clients.resolve`).
2. **Read `T/name`** of the requested `kind` from the **workload** cluster (a
   read-only `get`).
   - **Present** ⇒ stamp the bound Node (workload cluster) with the opt-in label
     **and** a reference annotation carrying `namespace`/`name`/`key`/`destPath`/
     `restartService`; set status `Ready`.
   - **Absent** (object missing, or its namespace missing) ⇒ **do not label the
     Node**; set a status condition with a precise reason (§4). 5-Spot creates
     nothing and waits for Flux to deliver it; the next reconcile re-checks.

There is exactly one namespace lookup, on the workload cluster. The default
`T = 5spot-system` means the agent reads from **its own namespace**, so no
cross-namespace agent RBAC is needed; an override exists for tenants who place
config elsewhere.

### 3. Node opt-in + agent consumption

The controller's only mutation is a Node patch: the opt-in label
(`5spot.finos.org/kata-config=enabled`, gating the agent's `nodeSelector`) plus a
reference annotation pointing at `T/name`. The node-side agent (**ADR 0003**)
reads that annotation, `get`s the object from the workload API, watches it for
live updates, and writes/heals the host file. This replaces any per-node
ConfigMap volume — a cluster-wide DaemonSet cannot template a `configMap.name`
volume per replica, which is exactly why the rejected design needed per-node
names.

**Tear-down handshake.** Removing the host drop-in cannot be a simple
"controller removes the label," because that would *deschedule the agent before
it could unlink the file*. Instead:

- The controller, on tear-down, clears **only the reference annotation** and
  leaves the opt-in label in place.
- On each successful write the agent records the absolute `destPath` it manages
  in a `5spot.finos.org/kata-config-applied` annotation on its own Node.
- When the agent sees the reference annotation gone, it unlinks the recorded host
  file (and, Phase 4, bounces k0s), then **removes the opt-in label from its own
  Node** — descheduling itself only *after* cleanup completes.

The agent therefore needs `nodes: get` **and** `patch` (its own Node only). This
is the one place the agent writes to the kube API; it still writes no
ConfigMaps/Secrets.

### 4. Status conditions (fail-fast, non-fatal to scheduling)

The `Active`-phase kata step sets a `KataConfigReady`-style condition. Reasons
cover at least:

- `Ready` — object resolved on the workload cluster; Node opted in.
- `SourceNotFound` — `T/name` absent on the workload cluster.
- `TargetNamespaceMissing` — namespace `T` absent on the workload cluster.

Failures are **best-effort / non-fatal** to scheduling (a missing kata config
degrades kata delivery but does not break day-to-day machine scheduling), exactly
as the reclaim-agent projection is — but they are **surfaced loudly** in status
so the operator sees *why* nothing landed.

### 5. RBAC — kata adds no write privilege

- 5-Spot performs **no writes** for kata delivery beyond the Node label/annotation
  patch it already has (`nodes: patch`). No ConfigMap/Secret writes, no namespace
  creation.
- Kata's reads — the source-object existence check and the namespace fail-fast
  probe — ride the `kubeconfig-<clusterName>` identity in true multi-cluster, and
  the controller's own ServiceAccount only in the degenerate co-located case.
  Those reads are covered by **read-only** ClusterRole rules: `configmaps: get`,
  `secrets: get/list/watch` (already present), and a new `namespaces: get`.
- The **agent's workload-cluster ServiceAccount** gets `get/watch` on
  `configmaps` (and `secrets`, for `Secret` sources) in its target namespace
  (default `5spot-system`) — a separate manifest, not this ClusterRole.
- **KSV-0049:** the controller `ClusterRole` holds **`configmaps: ["get"]` only**
  — no `create/patch/delete`. Kata is read-only, and the **reclaim-agent**
  per-node ConfigMap projection writes on the *workload* cluster via the resolved
  `kubeconfig-<clusterName>` identity, not the controller token (the posture
  reclaim shipped with — this ClusterRole had no `configmaps` rule before the kata
  branch). KSV-0049 therefore **does not fire** and no suppression is needed.
  *Caveat:* in the degenerate co-located dev/test posture (no kubeconfig Secret,
  `child_clients.resolve` falls back to the controller SA), the reclaim ConfigMap
  projection 403s — but it is best-effort / non-fatal, so this is an accepted
  degradation, not a failure.

## Consequences

- **Easier:** no per-node fan-out, no controller-owned duplication, no sync
  burden; Flux stays the single source of truth; the controller gains **no** new
  write ability anywhere; the controller `ClusterRole` ConfigMap rule is
  `get`-only, so KSV-0049 does not fire (see §5).
- **Operator contract:** kata config (and its namespace) **must pre-exist** on the
  workload cluster, delivered by Flux, before an SM references it. A missing
  object is a fail-fast, clearly-reasoned status condition — never silently
  created. This is the intended behaviour, not a limitation.
- **Edge case:** if a single workload cluster is shared by multiple tenants
  placing same-named objects in the default `5spot-system`, names can collide;
  the `spec.kata.namespace` override is the escape hatch (documented in the
  concept page).
- **Ruled out:** per-node ConfigMaps; controller-side copying/projection of
  config; controller-created namespaces; controller-SA ConfigMap writes; any
  5-Spot ownership of the delivered object's lifecycle.
- **CALM impact:** **updated.** The kata flow is "controller reads the
  workload-cluster object and opts the node in (or fails with status); agent reads
  the workload object." Reflect this in `data-asset-kata-config-configmap`,
  `rel-controller-kata-config-projection` (now a read + Node label/annotation, not
  a write), and `flow-kata-config-delivery`; the agent's read edge points at the
  workload-target namespace. `make calm-validate` + `make calm-diagrams` before
  implementation. ADR 0003 models the host-write/restart edges.
