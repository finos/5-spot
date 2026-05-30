# Child-cluster kubeconfig support

5-Spot manages physical machines via `ScheduledMachine` custom resources.
The `ScheduledMachine`, the CAPI `Machine`, the bootstrap config, and the
infrastructure resource all live on the **management cluster**. In a
production CAPI + k0smotron / k0rdent topology, however, the actual
`Node` (and the `Pod`s scheduled onto it) live inside a **workload
(child) cluster** whose API server is reachable only via that cluster's
kubeconfig.

This page documents how 5-Spot bridges that split: when a
`ScheduledMachine` carries a kubeconfig reference, the controller uses
the management client for `Machine` / bootstrap / infrastructure
operations, and the **child-cluster client** for every `Node` and `Pod`
operation it performs on behalf of that resource.

## The `kubeconfigSecretRef` field

`spec.kubeconfigSecretRef` is an optional pointer to a Secret in the
`ScheduledMachine`'s own namespace whose data contains a kubeconfig YAML
document.

```yaml
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: gpu-worker
  namespace: hosted-cluster-alpha
spec:
  clusterName: alpha
  kubeconfigSecretRef:
    name: alpha-kubeconfig        # CAPI convention
    key: value                    # default — CAPI writes kubeconfig under data.value
  schedule:
    daysOfWeek: ["mon-fri"]
    hoursOfDay: ["9-17"]
    timezone: America/Toronto
    enabled: true
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: K0sWorkerConfig
    spec: {}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec:
      address: 10.0.0.1
      port: 22
      user: admin
```

### Resolution order

When 5-Spot needs a `Node` or `Pod` client for a `ScheduledMachine`, it
resolves the right client in this order:

1. **Explicit `spec.kubeconfigSecretRef`.** The Secret named in the field
   is read from the SM's namespace. A missing Secret or unparseable
   kubeconfig **fails closed**: the reconciler does not silently fall
   back to the management client. The SM goes into an error state and
   backs off.
2. **Auto-discovery — `<clusterName>-kubeconfig`.** With no explicit ref,
   the controller looks for a Secret named `<spec.clusterName>-kubeconfig`
   in the same namespace (CAPI's convention). If found, it's used. If
   absent (404), the controller falls through silently.
3. **Management client.** When neither an explicit ref nor an
   auto-discovered Secret is available, the management client is used
   for `Node` / `Pod` operations as well. This is the **degenerate
   single-cluster posture** — useful for dev/test where management ≡
   workload.

### Why no cross-namespace `namespace` field

`KubeconfigSecretRef` intentionally has only `name` and `key`. The
Secret MUST live in the `ScheduledMachine`'s own namespace. Allowing a
`namespace` field would let a tenant in one namespace point at a
privileged kubeconfig in another — a privilege-escalation surface. The
CRD enforces this via `deny_unknown_fields`: any `namespace` key in the
ref is a hard schema error, not a silent miss.

## Required child-cluster RBAC

The supplied kubeconfig must authenticate as a service account or user
whose ClusterRole grants:

```yaml
- apiGroups: [""]
  resources: ["nodes"]
  verbs: ["get", "list", "watch", "patch"]
- apiGroups: [""]
  resources: ["pods"]
  verbs: ["get", "list", "delete"]
```

If `spec.killIfCommands` is also set, the kubeconfig additionally needs:

```yaml
- apiGroups: [""]
  resources: ["configmaps"]
  resourceNames: ["reclaim-agent-<node>"]
  verbs: ["get", "create", "patch", "delete"]
  # in the reclaim-agent namespace (default: `5spot-system`)
```

A future preflight tool (`5spot validate-kubeconfig --secret <ns>/<name>`,
tracked in Phase 3) will run `SelfSubjectAccessReview` checks against
each of these verbs and report any gaps.

## Cache invalidation

5-Spot caches the built child-cluster `kube::Client` per `(namespace,
secret_name)` keyed by the Secret's `metadata.resourceVersion`. Every
reconcile GETs the Secret to compare `resourceVersion` against the
cached entry; on mismatch the client is rebuilt. This means **token /
certificate rotations driven by CAPI's control-plane provider take
effect on the next reconcile** with no additional refresh logic on
either side. The cache holds up to 256 entries and evicts the
least-recently-used entry when full
(`CHILD_CLIENT_CACHE_CAP` in `src/constants.rs`).

## What stays on the management client

For the avoidance of doubt, the following always use the management
cluster's client:

- `ScheduledMachine` status patches
- CAPI `Machine` create / get / delete
- Bootstrap and infrastructure resources (`K0sWorkerConfig`,
  `RemoteMachine`, etc.)
- Reads of the kubeconfig Secret itself
- Kubernetes Events
- The `kube-runtime` `Controller` driver (the watch on
  `ScheduledMachine` and on CAPI `Machine`)

What uses the child client:

- `Node` cordon, taint apply, status enrichment, reclaim annotation
  cleanup, reclaim-agent label patch
- `Pod` list and delete (drain)
- The reclaim-agent `ConfigMap` apply (the consumer `DaemonSet` runs on
  workload-cluster Nodes; the ConfigMap must land there too)

## Threat model

- **Compromised tenant.** A tenant who can create a `ScheduledMachine`
  in their own namespace can only point it at a kubeconfig Secret in
  that same namespace. They cannot reach into another namespace's
  Secrets via this CRD.
- **Compromised child cluster.** A compromised child kubeconfig grants
  attacker access to the workload cluster's `nodes` + `pods` + (if
  `killIfCommands` is set) `configmaps`. It does NOT grant management
  cluster access. The blast radius is the workload cluster only.
- **Stale credentials.** A kubeconfig Secret whose token has been
  rotated externally (without a CAPI Secret update) will fail at the
  *child* cluster's API server, not silently succeed. 5-Spot surfaces
  the failure as `ReconcilerError::ChildClusterUnreachable` and backs
  off. A future Phase 2 `ChildClusterReachable=False` condition will
  make this visible without log-grepping.

## Limitations of this release

This release ships the **client resolution + routing** layer of
multi-cluster support. The full event-driven story still has one gap:

- **No per-child-cluster Node watch yet.** The management-cluster Node
  watch (in `main.rs`) is unchanged, so co-located deployments
  (management ≡ workload) keep their existing Node-event-driven
  responsiveness. For child-cluster SMs, Node state changes are picked
  up via the periodic `TIMER_REQUEUE_SECS` requeue and via CAPI
  `Machine` status changes (which are watched on the management
  cluster). Drain progress and Node-Ready transitions may therefore lag
  by up to the requeue interval. A follow-up adds per-`(namespace,
  secret_name)` Node watchers, lazily started on the first
  reconcile that observes a kubeconfig reference, and multiplexed into
  the `Controller` via `reconcile_on`.
