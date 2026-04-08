   Compiling five_spot v0.1.0 (/Users/erick/dev/5-spot)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.92s
     Running `target/debug/crddoc`
# 5Spot API Reference

## ScheduledMachine

The `ScheduledMachine` custom resource defines a machine that should be
automatically added to and removed from a k0smotron cluster based on a time schedule.

### API Group and Version

- **API Group**: `capi.5spot.io`
- **API Version**: `v1alpha1`
- **Kind**: `ScheduledMachine`

### Example

```yaml
apiVersion: capi.5spot.io/v1alpha1
kind: ScheduledMachine
metadata:
  name: example-machine
  namespace: default
spec:
  schedule:
    daysOfWeek:
      - mon-fri
    hoursOfDay:
      - 9-17
    timezone: America/New_York
    enabled: true
  machine:
    address: 192.168.1.100
    user: admin
    port: 22
    useSudo: false
    files: []
  bootstrapRef:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: KubeadmConfigTemplate
    name: worker-bootstrap-config
    namespace: default
  infrastructureRef:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: MachineTemplate
    name: worker-machine-template
    namespace: default
  clusterName: my-cluster
  priority: 50
  gracefulShutdownTimeout: 5m
  killSwitch: false
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

#### machine

Machine specification for k0smotron.

- **address** (required, string): IP address of the machine.

- **user** (required, string): Username for SSH connection.

- **port** (optional, integer, default: `22`): SSH port.

- **useSudo** (optional, boolean, default: `false`): Whether to use sudo for commands.

- **files** (optional, array): Files to be passed to user_data upon creation.

#### clusterName

(required, string) Name of the CAPI cluster this machine belongs to.
The bootstrap and infrastructure refs must be configured for this cluster.

#### priority

(optional, integer 0-100, default: `50`) Priority for machine scheduling.
Higher values indicate higher priority. Used for resource distribution across
operator instances.

#### gracefulShutdownTimeout

(optional, string, default: `5m`) Timeout for graceful machine shutdown.
Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours).

#### killSwitch

(optional, boolean, default: `false`) When true, immediately removes the machine
from the cluster and takes it out of rotation, bypassing the grace period.

### Status Fields

#### phase

Current phase of the machine lifecycle. Possible values:

- **Pending**: Initial state, schedule not yet evaluated
- **Scheduled**: Machine is within scheduled time window but not yet active
- **Active**: Machine is running and part of the cluster
- **UnScheduled**: Machine is outside scheduled time window
- **Removing**: Machine is being removed from cluster
- **Inactive**: Machine has been removed and is inactive
- **Error**: An error occurred during processing

#### conditions

Array of condition objects with the following fields:

- **type**: Condition type (e.g., `Ready`, `Scheduled`, `MachineReady`)
- **status**: `True`, `False`, or `Unknown`
- **reason**: One-word reason in CamelCase
- **message**: Human-readable message
- **lastTransitionTime**: Last time the condition transitioned

#### machineRef

Reference to the actual Machine resource:

- **name**: Machine name
- **namespace**: Machine namespace
- **uid**: Machine UID

#### lastScheduleTime

Last time the machine was scheduled and activated.

#### nextScheduleTime

Next time the machine will be scheduled (if calculable).

#### observedGeneration

The generation observed by the controller. Used for change detection.
