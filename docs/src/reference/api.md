# 5Spot API Reference

## ScheduledMachine

The `ScheduledMachine` custom resource defines a machine that should be
automatically added to and removed from a k0smotron cluster based on a time schedule.

### API Group and Version

- **API Group**: `5spot.finos.org`
- **API Version**: `v1beta1` (storage/current), `v1alpha1` (served, deprecated)
- **Kind**: `ScheduledMachine`

> The CRD serves two versions (ADR 0007). `v1beta1` is the storage version and adds
> `spec.spotSchedule` plus an optional `spec.schedule`; `v1alpha1` is frozen and
> deprecated (existing `5spot.finos.org/v1alpha1` manifests continue to apply but have
> neither field). Conversion strategy is `None` — versions round-trip losslessly because
> `v1beta1` is a superset.

### Example

```yaml
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: example-spot-machine
  namespace: default
spec:
  clusterName: my-cluster
  # Optional: delegate the active/inactive decision to an external provider
  # (ADR 0006). AND-composed with `schedule` when both are present.
  spotSchedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: CapitalMarketsSchedule
    name: nyse-equities
  schedule:
    daysOfWeek:
      - mon-fri
    hoursOfDay:
      - 9-17
    timezone: America/New_York
    enabled: true
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

### Spec Fields

> **At least one of `schedule` / `spotSchedule` is required** (CEL-enforced). When
> both are set the machine is active only when the time window **and** the provider
> both agree (logical AND); `killSwitch` always overrides.

#### schedule

Inline time-based scheduling configuration. **Optional since `v1beta1`** — a machine
may instead delegate its decision to a `spotSchedule` provider.

- **daysOfWeek** (required, array of strings): Days when machine should be active.
  Supports ranges (`mon-fri`) and combinations (`mon-wed,fri-sun`).

- **hoursOfDay** (required, array of strings): Hours when machine should be active (0-23).
  Supports ranges (`9-17`) and combinations (`0-9,18-23`).

- **timezone** (optional, string, default: `UTC`): Timezone for the schedule.
  Must be a valid IANA timezone (e.g., `America/New_York`, `Europe/London`).

- **enabled** (optional, boolean, default: `true`): Whether the schedule is enabled.

#### spotSchedule

Reference to an external spot-schedule provider resource that owns this machine's
active/inactive decision (ADR 0006). The 5-Spot controller watches the referenced
object and reads only its duck-typed `status.active` (and `Ready` condition) — never
the provider `spec`, and it never writes the provider object. Composed with
`schedule` via logical AND when both are present.

- **apiVersion** (required, string): `group/version` of the provider. The group MUST
  be `spotschedules.5spot.finos.org` (CEL-pinned); any served version is accepted.
- **kind** (required, string): Provider kind, e.g. `CapitalMarketsSchedule`.
- **name** (required, string): Provider object name in **this machine's namespace**.
  Cross-namespace references are not supported.

See the [Spot Schedule Provider Contract](spot-schedule-contract.md) for the full
contract a provider implements, and the `CapitalMarketsSchedule` reference provider.

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

(optional, integer 0-100, default: `50`) Priority for machine scheduling.
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
