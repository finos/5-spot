# Installing CRDs

5-Spot uses Custom Resource Definitions (CRDs) to extend the Kubernetes API.

## ScheduledMachine CRD

The `ScheduledMachine` CRD is the primary resource type for 5-Spot.

### Installation

```bash
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

Or apply the whole directory (also installs the spot-schedule provider CRD
below):

```bash
kubectl apply -f deploy/crds/
```

Or from the repository:

```bash
kubectl apply -f https://raw.githubusercontent.com/finos/5-spot/main/deploy/crds/scheduledmachine.yaml
```

The `ScheduledMachine` CRD serves a single version `v1beta1` (storage / served;
ADR 0009 removed the former `v1alpha1`). Its `spec.schedule` is a **required
reference** to a spot-schedule provider object in the same namespace.

## TimeBasedSpotSchedule CRD (default spot-schedule provider)

The **default, first-party** spot-schedule provider CRD
(`timebasedspotschedules.spotschedules.5spot.finos.org`, ADR 0009) — the reified
former inline schedule (day / hour / timezone windows). A `ScheduledMachine`
references it via `spec.schedule`. Install it (ships in `deploy/crds/`):

```bash
kubectl apply -f deploy/crds/timebasedspotschedule.yaml
```

## CapitalMarketsSchedule CRD (spot-schedule provider)

The reference spot-schedule provider CRD
(`capitalmarketsschedules.spotschedules.5spot.finos.org`, ADR 0006). Install it
if a `ScheduledMachine.spec.schedule` references a `CapitalMarketsSchedule`:

```bash
kubectl apply -f deploy/crds/capitalmarketsschedule.yaml
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

- **apiVersion**: `5spot.finos.org/v1beta1`
- **kind**: `ScheduledMachine`
- **spec**: Configuration for scheduling and machine management
- **status**: Current state and conditions

See the [API Reference](../reference/api.md) for complete field documentation.

## Generating CRDs

If building from source, generate CRDs using:

```bash
cargo run --bin crdgen
```

This writes `deploy/crds/scheduledmachine.yaml`,
`deploy/crds/capitalmarketsschedule.yaml`, and
`deploy/crds/timebasedspotschedule.yaml`.

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
