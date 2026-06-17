# CAPI Integration

How 5-Spot integrates with Cluster API (CAPI) for machine lifecycle management.

## Overview

5-Spot leverages CAPI's infrastructure abstraction to manage physical machines:

```mermaid
flowchart TB
    SM[ScheduledMachine] --> 5-Spot[5-Spot Operator]
    5-Spot --> Machine[CAPI Machine]
    Machine --> Bootstrap[Bootstrap Provider]
    Machine --> Infra[Infrastructure Provider]
    Infra --> Physical[Physical Machine]
    Bootstrap --> K8s[Kubernetes Node]
```

## CAPI Resources

### Machine

5-Spot creates CAPI `Machine` resources:

```yaml
apiVersion: cluster.x-k8s.io/v1beta1
kind: Machine
metadata:
  name: scheduled-machine-worker-xyz
  namespace: default
  ownerReferences:
    - apiVersion: 5spot.finos.org/v1beta1
      kind: ScheduledMachine
      name: scheduled-machine
spec:
  clusterName: my-cluster
  bootstrap:
    configRef:
      apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
      kind: KubeadmConfig
      name: worker-bootstrap
  infrastructureRef:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: BareMetalMachine
    name: worker-infra
```

### Bootstrap Configuration

Inline bootstrap provider spec — 5-Spot creates this resource when the schedule window opens:

```yaml
bootstrapSpec:
  apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
  kind: K0sWorkerConfig
  spec:
    version: v1.30.0+k0s.0
```

### Infrastructure Specification

Inline infrastructure provider spec — 5-Spot creates this resource when the schedule window opens:

```yaml
infrastructureSpec:
  apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
  kind: RemoteMachine
  spec:
    address: 192.168.1.100
    port: 22
    user: admin
```

## Supported Providers

5-Spot works with any CAPI infrastructure provider:

| Provider | Use Case |
|----------|----------|
| Metal3 | Bare metal via Ironic |
| Packet/Equinix | Cloud bare metal |
| vSphere | VMware virtual machines |
| AWS | EC2 instances |
| Azure | Azure VMs |
| GCP | GCE instances |

## Machine Creation Flow

```mermaid
sequenceDiagram
    participant SM as ScheduledMachine
    participant 5S as 5-Spot
    participant CAPI as CAPI Controller
    participant Infra as Infrastructure Provider
    participant Boot as Bootstrap Provider
    participant Machine as Physical Machine
    
    5S->>SM: Watch for schedule match
    5S->>CAPI: Create Machine CR
    CAPI->>Infra: Provision infrastructure
    Infra->>Machine: Power on / provision
    Machine-->>Infra: Ready
    CAPI->>Boot: Generate bootstrap data
    Boot->>Machine: Apply bootstrap
    Machine-->>Boot: Node joined
    CAPI-->>5S: Machine Ready
    5S->>SM: Update status: Active
```

## Machine Deletion Flow

```mermaid
sequenceDiagram
    participant SM as ScheduledMachine
    participant 5S as 5-Spot
    participant K8s as Kubernetes
    participant CAPI as CAPI Controller
    participant Infra as Infrastructure Provider
    
    5S->>SM: Schedule window ends
    5S->>K8s: Cordon node
    5S->>K8s: Drain node
    Note over 5S: Grace period
    5S->>CAPI: Delete Machine CR
    CAPI->>Infra: Deprovision
    Infra-->>CAPI: Deprovisioned
    CAPI-->>5S: Machine deleted
    5S->>SM: Update status: Inactive
```

## Configuration Examples

### k0smotron / k0s

```yaml
spec:
  clusterName: k0s-cluster
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: K0sWorkerConfig
    spec:
      version: v1.30.0+k0s.0
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec:
      address: 192.168.1.100
      port: 22
      user: admin
```

### Metal3

```yaml
spec:
  clusterName: metal3-cluster
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: KubeadmConfig
    spec:
      joinConfiguration:
        nodeRegistration: {}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: Metal3Machine
    spec:
      image:
        url: http://example.com/ubuntu.img
        checksum: sha256:abc123
```

## Error Handling

### Infrastructure Provisioning Failure

- ScheduledMachine enters `Error` phase
- Condition updated with error details
- Automatic retry with backoff

### Bootstrap Failure

- CAPI handles bootstrap retries
- 5-Spot monitors Machine status
- Propagates errors to ScheduledMachine status

### Node Join Failure

- Machine marked as not ready
- ScheduledMachine reflects unhealthy state
- Manual intervention may be required

## Related

- [Architecture](../concepts/architecture.md) - System design
- [ScheduledMachine](../concepts/scheduled-machine.md) - CRD specification
- [Troubleshooting](../operations/troubleshooting.md) - Common issues
