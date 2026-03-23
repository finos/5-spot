# 5-Spot API Reference

## ScheduledMachine

The `ScheduledMachine` custom resource defines a machine that should be
automatically added to and removed from a Cluster API cluster based on a time schedule.

### API Group and Version

- **API Group**: `5spot.io`
- **API Version**: `v1alpha1`
- **Kind**: `ScheduledMachine`
- **Short Name**: `sm`
- **Scope**: Namespaced

### Print Columns

When using `kubectl get scheduledmachines`:

| Column | JSONPath | Description |
|--------|----------|-------------|
| Phase | `.status.phase` | Current lifecycle phase |
| InSchedule | `.status.inSchedule` | Whether in scheduled window |
| Enabled | `.spec.schedule.enabled` | Whether schedule is enabled |
| KillSwitch | `.spec.killSwitch` | Kill switch status |
| Age | `.metadata.creationTimestamp` | Resource age |

### Example

```yaml
apiVersion: 5spot.io/v1alpha1
kind: ScheduledMachine
metadata:
  name: business-hours-worker
  namespace: default
spec:
  # Schedule configuration
  schedule:
    # Option 1: Cron expression (takes precedence if specified)
    # cron: "0 9-17 * * 1-5"
    
    # Option 2: Day/hour ranges
    daysOfWeek:
      - mon-fri
    hoursOfDay:
      - 9-17
    timezone: America/New_York
    enabled: true
  
  # Inline bootstrap configuration
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: K0sWorkerConfig
    spec:
      version: v1.30.0+k0s.0
  
  # Inline infrastructure configuration
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec:
      address: 192.168.1.100
      port: 22
      user: admin
      useSudo: true
  
  # Optional: Custom labels/annotations for created Machine
  machineTemplate:
    labels:
      environment: production
    annotations:
      description: "Business hours worker node"
  
  clusterName: my-cluster
  priority: 50
  gracefulShutdownTimeout: 5m
  nodeDrainTimeout: 5m
  killSwitch: false
```

---

## Spec Fields

### schedule

Machine scheduling configuration. Either `cron` OR `daysOfWeek`/`hoursOfDay` must be specified.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `cron` | string | No | - | Cron expression. Takes precedence over day/hour ranges. |
| `daysOfWeek` | []string | No* | `[]` | Days when active (e.g., `["mon-fri"]`). |
| `hoursOfDay` | []string | No* | `[]` | Hours when active (e.g., `["9-17"]`). |
| `timezone` | string | No | `UTC` | IANA timezone for schedule evaluation. |
| `enabled` | bool | No | `true` | Whether the schedule is enabled. |

*Either `cron` OR `daysOfWeek`/`hoursOfDay` should be specified.

#### Cron Format

Standard 5-field cron: `minute hour day-of-month month day-of-week`

```yaml
schedule:
  cron: "0 9-17 * * 1-5"  # Mon-Fri 9am-5pm
```

#### Day/Hour Ranges

Supports ranges, wrap-around, and comma-separated values:

```yaml
schedule:
  daysOfWeek:
    - mon-fri      # Range
    - fri-mon      # Wrap-around (Fri-Sun-Mon)
  hoursOfDay:
    - 9-17         # Range (inclusive)
    - 22-6         # Wrap-around (overnight)
```

---

### bootstrapSpec

**Required.** Inline bootstrap configuration. This resource is created when the schedule is active.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `apiVersion` | string | Yes | API version (e.g., `bootstrap.cluster.x-k8s.io/v1beta1`) |
| `kind` | string | Yes | Kind (e.g., `K0sWorkerConfig`, `KubeadmConfig`) |
| `namespace` | string | No | Namespace for created resource. Defaults to ScheduledMachine namespace. |
| `spec` | object | Yes | Provider-specific configuration. |

Example:

```yaml
bootstrapSpec:
  apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
  kind: K0sWorkerConfig
  spec:
    version: v1.30.0+k0s.0
```

---

### infrastructureSpec

**Required.** Inline infrastructure configuration. This resource is created when the schedule is active.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `apiVersion` | string | Yes | API version (e.g., `infrastructure.cluster.x-k8s.io/v1beta1`) |
| `kind` | string | Yes | Kind (e.g., `RemoteMachine`, `AWSMachine`) |
| `namespace` | string | No | Namespace for created resource. Defaults to ScheduledMachine namespace. |
| `spec` | object | Yes | Provider-specific configuration. |

Example:

```yaml
infrastructureSpec:
  apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
  kind: RemoteMachine
  spec:
    address: 192.168.1.100
    port: 22
    user: admin
    useSudo: true
```

---

### machineTemplate

**Optional.** Configuration applied to the created CAPI Machine.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `labels` | map[string]string | No | `{}` | Labels to apply to the created Machine. |
| `annotations` | map[string]string | No | `{}` | Annotations to apply to the created Machine. |

Example:

```yaml
machineTemplate:
  labels:
    environment: production
    team: platform
  annotations:
    description: "Scheduled worker node"
```

---

### clusterName

**Required** (string). Name of the CAPI cluster this machine belongs to.

---

### priority

**Optional** (integer 0-100, default: `50`). Priority for machine scheduling.
Higher values indicate higher priority. Used for resource distribution across
controller instances.

---

### gracefulShutdownTimeout

**Optional** (string, default: `5m`). Timeout for graceful machine shutdown.
Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours).

---

### nodeDrainTimeout

**Optional** (string, default: `5m`). Timeout for draining the node before deletion.
Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours).

---

### killSwitch

**Optional** (boolean, default: `false`). When true, immediately removes the machine
from the cluster, bypassing the grace period. Sets phase to `Terminated`.

---

## Status Fields

### phase

Current phase of the machine lifecycle.

| Phase | Description |
|-------|-------------|
| `Pending` | Initial state, schedule being evaluated |
| `Active` | Machine is running and part of the cluster |
| `ShuttingDown` | Machine is being gracefully removed (drain in progress) |
| `Inactive` | Machine has been removed, waiting for next schedule |
| `Disabled` | Schedule is disabled (`enabled: false`) |
| `Terminated` | Machine was removed via kill switch |
| `Error` | An error occurred during processing |

---

### message

Human-readable status message describing current state.

---

### inSchedule

Boolean indicating whether the current time is within the scheduled window.

---

### machineRef

Reference to the created CAPI Machine resource:

```yaml
machineRef:
  apiVersion: cluster.x-k8s.io/v1beta1
  kind: Machine
  name: business-hours-worker-machine
  namespace: default
```

---

### bootstrapRef

Reference to the created bootstrap resource:

```yaml
bootstrapRef:
  apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
  kind: K0sWorkerConfig
  name: business-hours-worker-bootstrap
  namespace: default
```

---

### infrastructureRef

Reference to the created infrastructure resource:

```yaml
infrastructureRef:
  apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
  kind: RemoteMachine
  name: business-hours-worker-infra
  namespace: default
```

---

### nodeRef

Reference to the Kubernetes Node (once provisioned):

```yaml
nodeRef:
  name: worker-node-01
```

---

### conditions

Array of condition objects:

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Condition type (e.g., `Ready`, `Scheduled`, `MachineReady`, `ReferencesValid`) |
| `status` | string | `True`, `False`, or `Unknown` |
| `reason` | string | One-word reason in CamelCase |
| `message` | string | Human-readable message |
| `lastTransitionTime` | string | RFC3339 timestamp of last transition |

---

### lastScheduledTime

RFC3339 timestamp of the last time the machine was created.

---

### nextActivation

RFC3339 timestamp of the next scheduled activation time (if calculable).

---

### nextCleanup

RFC3339 timestamp of when the machine will be cleaned up (when in ShuttingDown phase).

---

### observedGeneration

The generation observed by the controller. Used for change detection.

---

## Condition Types

| Type | Description |
|------|-------------|
| `Ready` | Overall readiness status |
| `Scheduled` | Whether within schedule window |
| `MachineReady` | CAPI Machine health status |
| `ReferencesValid` | Bootstrap/Infrastructure spec validation |

## Condition Reasons

| Reason | Description |
|--------|-------------|
| `ReconcileSucceeded` | Reconciliation completed successfully |
| `ReconcileFailed` | Reconciliation failed |
| `ScheduleActive` | Current time is within schedule |
| `ScheduleInactive` | Current time is outside schedule |
| `ScheduleDisabled` | Schedule is disabled |
| `MachineCreated` | CAPI Machine was created |
| `MachineDeleted` | CAPI Machine was deleted |
| `MachineReady` | CAPI Machine is healthy |
| `KillSwitchActivated` | Kill switch was activated |
| `AwaitingSchedule` | Waiting for schedule window |
| `GracePeriodActive` | Grace period is in progress |
| `ReferencesInvalid` | Bootstrap/Infrastructure spec is invalid |

---

## Finalizers

The controller adds the finalizer `5spot.io/scheduledmachine` to ensure proper cleanup
of created resources before the ScheduledMachine is deleted.
