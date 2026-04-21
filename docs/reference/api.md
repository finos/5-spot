# 5Spot API Reference

## ScheduledMachine

The `ScheduledMachine` custom resource defines a machine that should be
automatically added to and removed from a k0smotron cluster based on a time schedule.

### API Group and Version

- **API Group**: `5spot.finos.org`
- **API Version**: `v1alpha1`
- **Kind**: `ScheduledMachine`

### Example

```yaml
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: example-spot-machine
  namespace: default
spec:
  clusterName: my-cluster
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
```

### Spec Fields

#### schedule

Machine scheduling configuration.

- **daysOfWeek** (required, array of strings): Days when machine should be active.
  Supports ranges (`mon-fri`) and combinations (`mon-wed,fri-sun`).

- **hoursOfDay** (required, array of strings): Hours when machine should be active (0-23).
  Supports ranges (`9-17`) and combinations (`0-9,18-23`).

- **timezone** (optional, string, default: `UTC`): Timezone for the schedule.
  Must be a valid IANA timezone (e.g., `America/New_York`, `Europe/London`).

- **enabled** (optional, boolean, default: `true`): Whether the schedule is enabled.

#### clusterName

(required, string) Name of the CAPI cluster this machine belongs to.

#### bootstrapSpec

(required, object) Inline bootstrap configuration that will be created when the schedule is active.
This is a fully unstructured object that must contain:

- **apiVersion** (required, string): API version of the bootstrap resource (e.g., `bootstrap.cluster.x-k8s.io/v1beta1`)
- **kind** (required, string): Kind of the bootstrap resource (e.g., `K0sWorkerConfig`, `KubeadmConfig`)
- **spec** (required, object): Provider-specific configuration for the bootstrap resource

The controller validates that the apiVersion belongs to an allowed bootstrap API group.

#### infrastructureSpec

(required, object) Inline infrastructure configuration that will be created when the schedule is active.
This is a fully unstructured object that must contain:

- **apiVersion** (required, string): API version of the infrastructure resource (e.g., `infrastructure.cluster.x-k8s.io/v1beta1`)
- **kind** (required, string): Kind of the infrastructure resource (e.g., `RemoteMachine`, `AWSMachine`)
- **spec** (required, object): Provider-specific configuration for the infrastructure resource

The controller validates that the apiVersion belongs to an allowed infrastructure API group.

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
