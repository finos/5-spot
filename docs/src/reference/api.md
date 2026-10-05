# 5Spot API Reference

## ScheduledMachine

The `ScheduledMachine` custom resource defines a machine that should be
automatically added to and removed from a k0smotron cluster based on a time schedule.

### API Group and Version

- **API Group**: `5spot.finos.org`
- **API Version**: `v1beta1` (single served/storage version)
- **Kind**: `ScheduledMachine`

> Since ADR 0009, `spec.schedule` is a **required reference** to a spot-schedule
> provider object (in the `spotschedules.5spot.finos.org` group) that owns the
> machine's active/inactive decision. The former inline time window is now the
> first-party `TimeBasedSpotSchedule` provider; `CapitalMarketsSchedule` and
> third-party providers are referenced the same way. The pre-release `v1alpha1`
> `ScheduledMachine` version was dropped (ADR 0009 amends ADR 0007).

### Example

```yaml
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: example-spot-machine
  namespace: default
spec:
  clusterName: my-cluster
  enabled: true
  # Required: reference the spot-schedule provider that owns the
  # active/inactive decision (ADR 0009). The default is TimeBasedSpotSchedule.
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: weekdays-9-5
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: K0sWorkerConfig
    spec:
      version: v1.32.8+k0s.0
      downloadURL: https://github.com/k0sproject/k0s/releases/download/v1.32.8+k0s.0/k0s-v1.32.8+k0s.0-amd64
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec:
      address: 192.168.1.100
      port: 22
      user: root
      sshKeyRef:
        name: my-ssh-key
  machineTemplate:
    labels:
      node-role.kubernetes.io/worker: spot
    annotations:
      example.com/scheduled-by: 5spot
  priority: 50
  gracefulShutdownTimeout: 5m
  nodeDrainTimeout: 5m
  killSwitch: false
  killIfCommands:
    - java
    - idea
  nodeTaints:
    - key: workload
      value: batch
      effect: NoSchedule
  kata:
    kind: ConfigMap
    name: kata-drop-in
```

The referenced provider object lives in the same namespace. The default,
first-party provider is `TimeBasedSpotSchedule`:

```yaml
apiVersion: spotschedules.5spot.finos.org/v1alpha1
kind: TimeBasedSpotSchedule
metadata:
  name: weekdays-9-5
  namespace: default
spec:
  daysOfWeek:
    - mon-fri
  hoursOfDay:
    - 9-17
  timezone: America/New_York
  enabled: true
```

### Spec Fields

#### schedule

(required, object) Reference to the spot-schedule provider object that owns this
machine's active/inactive decision (ADR 0009). The 5-Spot controller watches the
referenced object and reads only its duck-typed `status.active` (and `Ready` condition)
— never the provider `spec`, and it never writes the provider object. The provider
verdict is the machine's should-be-active decision; `spec.enabled` and `killSwitch`
override it.

- **apiVersion** (required, string): `group/version` of the provider. The group MUST
  be `spotschedules.5spot.finos.org` (CEL-pinned); any served version is accepted.
- **kind** (required, string): Provider kind, e.g. `TimeBasedSpotSchedule` (the
  default) or `CapitalMarketsSchedule`.
- **name** (required, string): Provider object name in **this machine's namespace**.
  Cross-namespace references are not supported.

See the [Spot Schedule Provider Contract](spot-schedule-contract.md) for the full
contract a provider implements, plus the `TimeBasedSpotSchedule` and
`CapitalMarketsSchedule` first-party providers.

#### enabled

(optional, boolean, default: `true`) Administrative master switch for this machine
(ADR 0009). When `false` the machine is held **Disabled** regardless of what its
`schedule` provider reports — the SM-scoped on/off operators reach for, and the
loop-breaker the emergency-reclaim flow sets. Distinct from the provider's own
`status.active` and from `killSwitch` (immediate, terminal teardown).

#### clusterName

(required, string) Name of the CAPI cluster this machine belongs to.

#### bootstrapSpec

(required, object) Inline bootstrap configuration that will be created when the schedule is active.
This is a fully unstructured object that must contain:

- **apiVersion** (required, string): API version of the bootstrap resource (e.g., `bootstrap.cluster.x-k8s.io/v1beta1`)
- **kind** (required, string): Kind of the bootstrap resource (e.g., `K0sWorkerConfig`, `KubeadmConfig`)
- **spec** (required, object): Provider-specific configuration for the bootstrap resource

The controller validates that the apiVersion belongs to an allowed bootstrap API group.

It may also include an optional `metadata` block:

- **metadata.labels** (optional, map of string to string): merged onto the created bootstrap resource
- **metadata.annotations** (optional, map of string to string): merged onto the created bootstrap resource

`metadata.name` and `metadata.namespace` are **not** permitted — the controller
names the resource after the ScheduledMachine and creates it in the SM's own
namespace. Labels/annotations using reserved prefixes (`5spot.finos.org/`,
`cluster.x-k8s.io/`, `kubernetes.io/`, `k8s.io/`) are rejected.

#### infrastructureSpec

(required, object) Inline infrastructure configuration that will be created when the schedule is active.
This is a fully unstructured object that must contain:

- **apiVersion** (required, string): API version of the infrastructure resource (e.g., `infrastructure.cluster.x-k8s.io/v1beta1`)
- **kind** (required, string): Kind of the infrastructure resource (e.g., `RemoteMachine`, `AWSMachine`)
- **spec** (required, object): Provider-specific configuration for the infrastructure resource

The controller validates that the apiVersion belongs to an allowed infrastructure API group.

It may also include an optional `metadata` block:

- **metadata.labels** (optional, map of string to string): merged onto the created infrastructure resource
- **metadata.annotations** (optional, map of string to string): merged onto the created infrastructure resource

`metadata.name` and `metadata.namespace` are **not** permitted — the controller
names the resource after the ScheduledMachine and creates it in the SM's own
namespace. Labels/annotations using reserved prefixes (`5spot.finos.org/`,
`cluster.x-k8s.io/`, `kubernetes.io/`, `k8s.io/`) are rejected.

#### machineTemplate

(optional, object) Configuration for the created CAPI Machine resource.

- **labels** (optional, map of string to string): Labels to apply to the created Machine
- **annotations** (optional, map of string to string): Annotations to apply to the created Machine

Note: Labels and annotations using reserved prefixes (`5spot.finos.org/`, `cluster.x-k8s.io/`) are rejected.

#### priority

(optional, integer 0-255, default: `50`) Priority for machine scheduling.
Higher values indicate higher priority. Used for resource distribution across
operator instances.

#### gracefulShutdownTimeout

(optional, string, default: `5m`) Timeout for graceful machine shutdown.
Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours).

#### nodeDrainTimeout

(optional, string, default: `5m`) Timeout for draining the node before deletion.
Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours).

#### killSwitch

(optional, boolean, default: `false`) When true, immediately removes the machine
from the cluster and takes it out of rotation, bypassing the grace period.

#### killIfCommands

(optional, array of strings) Process patterns that trigger an emergency node reclaim.
When non-empty, the 5-Spot controller installs the `5spot-reclaim-agent` DaemonSet
on every Node backing this `ScheduledMachine`. The agent watches `/proc` for any
process whose basename or argv matches one of these patterns and, on first match,
annotates the Node to request immediate (non-graceful) removal from the cluster.

When absent or empty, no agent is installed and behaviour is time-based scheduling only.
Patterns are evaluated against both `/proc/<pid>/comm` (exact basename) and
`/proc/<pid>/cmdline` (substring).

#### nodeTaints

(optional, array of NodeTaint, default: `[]`) User-defined taints applied to the
Kubernetes Node once it is Ready. The controller owns and reconciles only the
taints it applied (tracked in `status.appliedNodeTaints` plus the
`5spot.finos.org/applied-taints` annotation on the Node). Admin-added taints on
the same Node are left untouched. Taint identity is the tuple `(key, effect)`;
`value` is mutable.

Each `NodeTaint` has the following fields:

- **key** (required, string): RFC-1123 qualified name. Max 253 chars total;
  name-part ≤ 63. Reserved prefixes rejected at admission: `5spot.finos.org/`,
  `kubernetes.io/`, `node.kubernetes.io/`, `node-role.kubernetes.io/`.
- **value** (optional, string): Optional value, ≤ 63 chars. Mutable — changing
  the value on an existing taint triggers an update, not an add/remove.
- **effect** (required, enum): One of `NoSchedule`, `PreferNoSchedule`, `NoExecute`.

Duplicate `(key, effect)` pairs are rejected at admission. Admin-added taints
colliding on `(key, effect)` are surfaced as a `TaintOwnershipConflict` condition
rather than overwritten.

#### kata

(optional, KataConfig) Reference to a `Secret` or `ConfigMap` **on the workload
cluster** holding a Kata containerd drop-in to deliver to the node(s) this resource
owns. When set, the controller resolves the object on the workload cluster (via the
`kubeconfig-<clusterName>` Secret) in `kata.namespace` (default `5spot-system`). If
present, it stamps the `5spot.finos.org/kata-config=enabled` opt-in label plus a
reference annotation on the Node; the `5spot-kata-config-agent` DaemonSet reads the
object from the workload API, writes the drop-in to the fixed host path
`/etc/k0s/containerd.d/kata.toml` (not configurable — ADR 0005), and restarts
`restartService` so containerd reloads it. If the object (or its namespace) is
absent, the controller does NOT label the Node and reports a fail-fast status
condition — 5-Spot never creates the object (it must pre-exist, Flux-delivered).
This is config delivery, not a Kata install — `/opt/kata` binaries remain
`kata-deploy`'s job. See ADR 0002 and ADR 0003.

`KataConfig` has the following fields:

- **kind** (required, enum): One of `ConfigMap`, `Secret` — the source kind.
- **name** (required, string): Source object name on the workload cluster,
  RFC-1123 DNS subdomain (≤ 253 chars).
- **namespace** (optional, string, default: `5spot-system`): workload-cluster
  namespace the agent reads the object from. Override for per-tenant placement.
- **key** (optional, string, default: `kata-containers.toml`): `data` key whose
  value is the drop-in content.
- **restartService** (optional, string, default: `k0sworker.service`): systemd
  unit restarted via `nsenter` so containerd reloads the drop-in. Override with
  `k0scontroller.service` on single-node layouts.

### Status Fields

#### phase

Current phase of the machine lifecycle. Possible values:

- **Pending**: Initial state, awaiting schedule evaluation
- **Active**: Machine is running and part of the cluster
- **ShuttingDown**: Machine is being gracefully removed (draining, etc.)
- **Inactive**: Machine is outside scheduled time window and has been removed
- **Disabled**: Schedule is disabled, machine is not active
- **Terminated**: Machine has been permanently removed
- **Error**: An error occurred during processing

#### conditions

Array of condition objects with the following fields:

- **type**: Condition type (e.g., `Ready`, `Scheduled`, `MachineReady`)
- **status**: `True`, `False`, or `Unknown`
- **reason**: One-word reason in CamelCase
- **message**: Human-readable message
- **lastTransitionTime**: Last time the condition transitioned

#### inSchedule

(boolean) Whether the machine is currently within its scheduled time window.

#### ready

(boolean) `True` only when `phase` is `Active`. Surfaced as the `Ready` printer column
for fast operator triage — any other phase (`Pending`, `ShuttingDown`, `Inactive`,
`Disabled`, `Terminated`, `Error`) is reported as `False`.

#### message

(string) Human-readable message describing the current state.

#### observedGeneration

(integer) The generation observed by the controller. Used for change detection.

#### providerID

(optional, string) Provider-assigned machine identifier, copied from the CAPI Machine's
`spec.providerID`. Stable for the life of the machine and unique across the cluster.
Examples: `libvirt:///uuid-abc-123`, `aws:///us-east-1a/i-0abcd1234`.

#### nodeRef

(optional, object) Reference to the Kubernetes Node once the Machine is provisioned.
Mirrors the shape of CAPI's `Machine.status.nodeRef`:

- **apiVersion** (required, string): API version of the Node resource (typically `v1`)
- **kind** (required, string): Kind of the referenced object (typically `Node`)
- **name** (required, string): Name of the Node
- **uid** (optional, string): UID of the Node, protecting against name reuse

#### appliedNodeTaints

(optional, array of NodeTaint, default: `[]`) The controller's record of truth
for which taints it applied to the Node. Only entries in this list are eligible
for removal on a subsequent reconcile — admin-added taints colliding on
`(key, effect)` are surfaced as a `TaintOwnershipConflict` condition rather than
overwritten.

See `spec.nodeTaints` for the `NodeTaint` field schema.

---

## ScheduledCapacity

The `ScheduledCapacity` custom resource gates **one numeric capacity field on
an object 5-Spot does not own**, from the same spot-schedule providers a
`ScheduledMachine` uses (ADR 0011).

It is the opposite of `ScheduledMachine`. A `ScheduledMachine` is *handover*:
the whole physical machine leaves the cluster when the schedule closes. A
`ScheduledCapacity` keeps the machine in the cluster the whole time and
concedes a bounded slice of it while the schedule is open. On a 32-core host
whose incumbent only ever uses 16, that is how the spare capacity gets used
without the incumbent ever losing the node.

**It never creates or deletes the object it governs.** Writing a capacity
field is reversible and bounded; deleting a consumer's pool with live claims
would destroy in-flight work using knowledge this controller does not have.

Reconciled by the separate `5spot-capacity-controller` binary under its own
ServiceAccount, so compromising it cannot delete a CAPI `Machine` and
compromising the machine controller cannot write capacity. A cluster with no
capacity consumer installs none of it.

### API Group and Version

- **Group**: `5spot.finos.org`
- **Version**: `v1alpha1`
- **Kind**: `ScheduledCapacity`
- **Short name**: `scap`
- **Scope**: Namespaced

> `v1alpha1` while the consumer contract settles; `ScheduledMachine` is at
> `v1beta1`. Field evolution follows ADR 0007's additive-only rule.

### Example

```yaml
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledCapacity
metadata:
  name: agent-sandbox-capacity
  namespace: sandboxes
spec:
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: CapitalMarketsSchedule
    name: nyse-trading-day
  targetRef:
    apiVersion: banlieue.io/v1alpha1
    kind: VirtualMachinePool
    name: agent-sandboxes
  capacity:
    path: spec.warmReplicas
    activeValue: 10
  handback:
    drainedPath: status.claimed
    timeout: 10m
  nodeName: worker-3
```

### Spec Fields

#### schedule

(required, object) Reference to the spot-schedule provider that owns the
active/inactive decision. The same shape as
`ScheduledMachine.spec.schedule`, resolved by the same code, so the
`Unresolved`-is-not-`Inactive` rule cannot drift between the two kinds.

An unresolved provider **holds the last written value** and reports why; it is
never read as inactive.

#### enabled

(optional, boolean, default: `true`) Administrative master switch. `false`
holds the object `Disabled` and drives capacity to zero regardless of the
provider.

#### killSwitch

(optional, boolean, default: `false`) Immediate, terminal handback. Takes
precedence over `enabled` **and** the provider verdict, and is the one path
that does not wait for a drain, because the operator has explicitly asked for
the slice back now. It is still only a field write; nothing is deleted.

#### targetRef

(required, object) The foreign object whose capacity field this schedule
gates. Must live in **this object's namespace**.

- **apiVersion** (required, string): `group/version` of the target. The group
  must be in the controller's allowlist, checked at reconcile time so the
  permitted set lives in one place and cannot be widened by a stale CRD
- **kind** (required, string): Kind of the target resource
- **name** (required, string): Name of the target object

5-Spot sets no `ownerReference` on the target and touches no other field.

#### capacity

(required, object) The single field to write and the value to write while the
schedule is active.

- **path** (required, string): dot-separated path of the numeric field, e.g.
  `spec.warmReplicas`
- **activeValue** (required, integer, 1..=1000000): value written while active

**`path` is a security control, not a convenience.** It names a field on an
object 5-Spot does not own, so it is restricted to dot-separated camelCase
segments (at most 8) and must start with `spec.`:

- `metadata.` is rejected: a path reaching `ownerReferences`, `finalizers` or
  labels on a foreign object would let a CR author use the controller's
  credential as an elevation primitive
- `status.` is rejected: a status is a controller's own report, not a knob
- array indices, wildcards, `..`, quotes and `/` are all inexpressible, so the
  value can never be read as a JSON Pointer or JSONPath expression

The **inactive** value is fixed at `0` and is deliberately not configurable: a
schedule that hands nothing back is not a schedule.

#### handback

(optional, object) The cooperative, bounded handback.

- **drainedPath** (optional, string): a numeric field on the target's
  **status** reporting how much of the slice is still in use, e.g.
  `status.claimed`. Must start with `status.`
- **timeout** (optional, string, default: `10m`): how long to wait for
  `drainedPath` to reach zero

On deactivation the controller writes zero **first**, then waits. Zero is what
makes the drain converge: the gated field is a warm-pool target, so zero stops
the consumer replenishing idle capacity while work already claimed finishes on
its own. Holding the field above zero until the consumer reported drained would
be circular.

When `timeout` expires with the consumer still not drained, the controller
**holds and reports loudly** (phase `HandbackTimedOut`, a `False`
`HandbackComplete` condition, a metric). It never forces, deletes or
escalates: a missed handover is visible and recoverable, while killing an
agent mid-task is neither.

Omit the whole block for a consumer that exposes no in-use counter; handback
then completes as soon as the zero write lands.

#### nodeName

(optional, string) The Kubernetes Node this object believes it governs.

A host is governed by `ScheduledMachine` **or** by `ScheduledCapacity`, never
both: one removes the node, the other keeps it and shares it, and both at once
half-works. Setting this lets the controller detect the contradiction, by
comparing it against every `ScheduledMachine.status.nodeRef.name` in the
namespace; on a match it sets `HostGovernanceConflict` and drives capacity to
**zero** (ADR 0014). The active value is never written to a conflicted object,
so a conflicted host can never be carrying conceded capacity, whichever order
the conflict and the write arrived in. Without `nodeName` the two objects
cannot be correlated and the invariant is
documentation only.

### Status Fields

#### phase

(optional, string) One of `Pending`, `Active`, `HandingBack`,
`HandbackTimedOut`, `Inactive`, `Disabled`, `Terminated`, `Error`.

Its own phase set, not `ScheduledMachine`'s: a budget ramp and a drain wait are
not node membership. `HandingBack` and `HandbackTimedOut` exist nowhere else.

#### writtenValue

(optional, integer) The value this controller last successfully wrote. A
written `0` is meaningful state (handback in progress or complete), not an
absent one.

#### observedDrainedValue

(optional, integer) Last value read from `spec.handback.drainedPath`. Absent
and `0` are different answers: an unread counter is not a drained one, and
only `0` completes a handback.

#### handbackDeadline

(optional, string) RFC3339 instant at which the current handback wait expires.
Cleared once handback completes.

#### governedNode

(optional, string) The Node this object believes it governs, echoed from
`spec.nodeName`. Reported even when the overlap check cannot run, so an
operator can correlate by hand.

#### conditions

(optional, array) `Ready`, `TargetResolved`, `CapacityWritten`,
`HandbackComplete`, `HostGovernanceConflict`, plus `SpotScheduleResolved`
reused unchanged from ADR 0006 so provider resolution reads the same on both
kinds.
