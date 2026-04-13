# Quick Start

Get started with 5-Spot in minutes.

## Prerequisites

- Kubernetes cluster (1.27+)
- kubectl configured
- Cluster API (CAPI) installed

## Installation

### 1. Apply the CRD

```bash
kubectl apply -f https://raw.githubusercontent.com/finos/5-spot/main/deploy/crds/scheduledmachine.yaml
```

### 2. Deploy the Operator

```bash
kubectl apply -f https://raw.githubusercontent.com/finos/5-spot/main/deploy/deployment/
```

### 3. Verify Installation

```bash
kubectl get pods -n 5spot-system
kubectl get crds | grep 5spot
```

## Create Your First ScheduledMachine

Create a file named `my-scheduled-machine.yaml`:

```yaml
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: my-first-scheduled-machine
  namespace: default
spec:
  schedule:
    daysOfWeek:
      - mon-fri
    hoursOfDay:
      - 9-17
    timezone: UTC
    enabled: true
  
  machine:
    address: 192.168.1.100
    user: admin
    port: 22
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
```

Apply it:

```bash
kubectl apply -f my-scheduled-machine.yaml
```

## Check Status

```bash
kubectl get scheduledmachines
kubectl describe scheduledmachine my-first-scheduled-machine
```

## Next Steps

- [Prerequisites](./prerequisites.md) - Detailed requirements
- [Installing CRDs](./crds.md) - Manual CRD installation
- [Deploying Operator](./controller.md) - Production deployment
- [Concepts](../concepts/index.md) - Understand how 5-Spot works
