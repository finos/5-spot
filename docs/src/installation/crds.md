# Installing CRDs

5-Spot uses Custom Resource Definitions (CRDs) to extend the Kubernetes API.

## ScheduledMachine CRD

The `ScheduledMachine` CRD is the primary resource type for 5-Spot.

### Installation

```bash
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

Or from the repository:

```bash
kubectl apply -f https://raw.githubusercontent.com/finos/5-spot/main/deploy/crds/scheduledmachine.yaml
```

### Verify Installation

```bash
kubectl get crds scheduledmachines.5spot.finos.org
```

Expected output:

```
NAME                                 CREATED AT
scheduledmachines.5spot.finos.org     2025-01-01T00:00:00Z
```

## CRD Schema

The CRD defines the following structure:

- **apiVersion**: `5spot.finos.org/v1alpha1`
- **kind**: `ScheduledMachine`
- **spec**: Configuration for scheduling and machine management
- **status**: Current state and conditions

See the [API Reference](../reference/api.md) for complete field documentation.

## Generating CRDs

If building from source, generate CRDs using:

```bash
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml
```

## Upgrading CRDs

When upgrading 5-Spot, update the CRD first:

```bash
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

!!! warning "Caution"
    CRD changes may affect existing resources. Always review the changelog before upgrading.

## Next Steps

- [Deploying Operator](./controller.md) - Deploy the 5-Spot controller
- [Quick Start](./quickstart.md) - Create your first ScheduledMachine
