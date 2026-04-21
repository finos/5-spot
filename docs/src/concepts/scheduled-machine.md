# ScheduledMachine

The `ScheduledMachine` Custom Resource Definition (CRD) is the primary API for 5-Spot.

## Overview

A ScheduledMachine defines:

- When a machine should be active (schedule)
- Inline bootstrap and infrastructure specs (CAPI resources created on-demand)
- Lifecycle behavior (priority, grace period, kill switch)

## Example

```yaml
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: business-hours-worker
  namespace: default
spec:
  schedule:
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
  
  clusterName: production-cluster
  priority: 50
  gracefulShutdownTimeout: 5m
  nodeDrainTimeout: 5m
  killSwitch: false
```

## Spec Fields

### schedule

Defines when the machine should be active using day and hour ranges.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `daysOfWeek` | `[]string` | No* | `[]` | Days when active. Supports ranges and lists. |
| `hoursOfDay` | `[]string` | No* | `[]` | Hours when active (0-23). Supports ranges. |
| `timezone` | `string` | No | `UTC` | IANA timezone for schedule evaluation. |
| `enabled` | `bool` | No | `true` | Whether the schedule is enabled. |

*At least one of `daysOfWeek` or `hoursOfDay` must be non-empty.

#### Day Format

- Single day: `mon`, `tue`, `wed`, `thu`, `fri`, `sat`, `sun`
- Range: `mon-fri`, `sat-sun`
- Mixed: `mon-wed,fri`

#### Hour Format

- Single hour: `9`, `14`, `22`
- Range: `9-17` (inclusive of both start and end)
- Mixed: `0-9,17-23`

### bootstrapSpec

Inline bootstrap configuration spec. This resource is created when the schedule is active.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `apiVersion` | `string` | Yes | API version (e.g., `bootstrap.cluster.x-k8s.io/v1beta1`) |
| `kind` | `string` | Yes | Kind of bootstrap resource (e.g., `K0sWorkerConfig`) |
| `namespace` | `string` | No | Namespace for created resource (defaults to ScheduledMachine namespace) |
| `spec` | `object` | Yes | Provider-specific spec (e.g., K0sWorkerConfig spec) |

### infrastructureSpec

Inline infrastructure configuration spec. This resource is created when the schedule is active.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `apiVersion` | `string` | Yes | API version (e.g., `infrastructure.cluster.x-k8s.io/v1beta1`) |
| `kind` | `string` | Yes | Kind of infrastructure resource (e.g., `RemoteMachine`) |
| `namespace` | `string` | No | Namespace for created resource (defaults to ScheduledMachine namespace) |
| `spec` | `object` | Yes | Provider-specific spec (e.g., RemoteMachine spec) |

### machineTemplate (Optional)

Optional configuration applied to the created CAPI Machine.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `labels` | `map[string]string` | No | `{}` | Labels to apply to the created Machine |
| `annotations` | `map[string]string` | No | `{}` | Annotations to apply to the created Machine |

### Other Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `clusterName` | `string` | Yes | - | Name of the CAPI cluster. |
| `priority` | `int` | No | `50` | Priority (0-255). Higher = more important. |
| `gracefulShutdownTimeout` | `string` | No | `5m` | Time for graceful machine shutdown. |
| `nodeDrainTimeout` | `string` | No | `5m` | Timeout for draining the node before deletion. |
| `killSwitch` | `bool` | No | `false` | Operator-driven kill switch. Immediately remove machine if `true`; reset to `false` to return to scheduled service. |
| `killIfCommands` | `[]string` | No | `null` | Node-side process-match kill switch. When non-empty, the reclaim agent DaemonSet is installed on the backing node and watches `/proc` for any process whose `comm` or `cmdline` matches an entry. First match triggers `EmergencyRemove` + auto-disables the schedule. See [Emergency Reclaim](./emergency-reclaim.md). |

## Status Fields

The status subresource contains the current state:

| Field | Type | Description |
|-------|------|-------------|
| `phase` | `string` | Current lifecycle phase (Pending, Active, ShuttingDown, Inactive, Disabled, Terminated, EmergencyRemove, Error) |
| `message` | `string` | Human-readable status message |
| `inSchedule` | `bool` | Whether currently within scheduled window |
| `conditions` | `[]Condition` | Detailed status conditions |
| `machineRef` | `ObjectReference` | Reference to created CAPI Machine |
| `bootstrapRef` | `ObjectReference` | Reference to created bootstrap resource |
| `infrastructureRef` | `ObjectReference` | Reference to created infrastructure resource |
| `nodeRef` | `NodeRef` | Reference to the Kubernetes Node (apiVersion, kind, name, uid) once provisioned |
| `providerID` | `string` | Provider-assigned machine identifier (copied from CAPI `Machine.spec.providerID`) |
| `lastScheduledTime` | `Time` | Last time machine was created |
| `nextActivation` | `Time` | Next scheduled activation time |
| `nextCleanup` | `Time` | Time when machine will be cleaned up |
| `observedGeneration` | `int` | Last observed generation |

## How It Works

```mermaid
flowchart TD
    A[ScheduledMachine Created] --> B{Schedule Active?}
    B -->|Yes| C[Create Bootstrap Resource]
    C --> D[Create Infrastructure Resource]
    D --> E[Create CAPI Machine]
    E --> F[Machine Joins Cluster]
    F --> G{Schedule Still Active?}
    G -->|Yes| G
    G -->|No| H[Begin Shutdown]
    H --> I[Drain Node]
    I --> J[Delete CAPI Machine]
    J --> K[Delete Bootstrap/Infrastructure]
    K --> L[Wait for Next Schedule]
    L --> B
    B -->|No| L
```

The controller:

1. Watches for `ScheduledMachine` resources
2. Evaluates schedules against current time (in configured timezone)
3. When schedule is active: creates bootstrap, infrastructure, and Machine resources
4. When schedule ends: gracefully shuts down and cleans up all created resources
5. Maintains owner references for automatic garbage collection

## Related

- [API Reference](../reference/api.md) - Complete API documentation
- [Machine Lifecycle](./machine-lifecycle.md) - Phase transitions
- [Schedules](./schedules.md) - Schedule configuration details
- [Emergency Reclaim](./emergency-reclaim.md) - `killIfCommands` and the process-match kill switch
