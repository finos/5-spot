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

Also install the default spot-schedule provider CRD (`TimeBasedSpotSchedule`),
which a `ScheduledMachine` references for its activation window. Applying the
whole `deploy/crds/` directory installs every CRD at once:

```bash
kubectl apply -f https://raw.githubusercontent.com/finos/5-spot/main/deploy/crds/timebasedspotschedule.yaml
# or: kubectl apply -f deploy/crds/   (applies all CRDs)
```

### 2. Deploy the Operator

The operator manifests live in multiple files across `deploy/deployment/`
(including a `rbac/` subdirectory), so clone the repo and apply
recursively:

```bash
git clone --depth=1 https://github.com/finos/5-spot.git
cd 5-spot
kubectl apply -R -f deploy/deployment/
```

The `-R` (recursive) flag is required so the ServiceAccount,
ClusterRole, and ClusterRoleBinding under `deploy/deployment/rbac/` are
applied. Without it the Deployment is created but cannot schedule pods
(`serviceaccount "5spot-controller" not found`).

### 3. Verify Installation

```bash
kubectl get pods -n 5spot-system
kubectl get crds | grep 5spot
```

## Create Your First ScheduledMachine

Create a file named `my-scheduled-machine.yaml`:

```yaml
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: my-first-scheduled-machine
  namespace: default
spec:
  # Administrative master switch (default true); false holds the machine Disabled.
  enabled: true

  # Required reference to a spot-schedule provider object in this namespace.
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: weekdays

  clusterName: my-cluster

  # Inline bootstrap configuration — 5-Spot creates this resource for you
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: K0sWorkerConfig
    spec:
      version: v1.30.0+k0s.0

  # Inline infrastructure configuration — 5-Spot creates this resource for you
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec:
      address: 192.168.1.100
      port: 22
      user: admin

  priority: 50
  gracefulShutdownTimeout: 5m
---
# The referenced provider object. It must exist in the SAME namespace as the
# ScheduledMachine, and the TimeBasedSpotSchedule CRD + its provider controller
# must be installed (deploy/spot-schedule-providers/time-based/).
apiVersion: spotschedules.5spot.finos.org/v1alpha1
kind: TimeBasedSpotSchedule
metadata:
  name: weekdays
  namespace: default
spec:
  daysOfWeek:
    - mon-fri
  hoursOfDay:
    - "9-17"
  timezone: UTC
  enabled: true
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
