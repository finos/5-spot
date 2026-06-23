# Deploying the Controller

This guide covers deploying the 5-Spot controller to your Kubernetes cluster.

## Deployment Methods

### Using kubectl

```bash
kubectl apply -R -f deploy/deployment/
```

The `-R` (recursive) flag is required so `deploy/deployment/rbac/`
(ServiceAccount, ClusterRole, ClusterRoleBinding) is included. Without
it the Deployment will be created but fail to schedule with
`serviceaccount "5spot-controller" not found`.

!!! important "You also need a spot-schedule provider"
    The controller alone does not decide *when* a machine is active — it reads
    `status.active` from a spot-schedule **provider** (ADR 0009). `deploy/deployment/`
    does **not** include one, so install at least the first-party
    [`TimeBasedSpotSchedule` provider](../guides/time-based-schedule.md#install)
    (`kubectl apply -k deploy/spot-schedule-providers/time-based/`). Without a provider
    running, `ScheduledMachine`s referencing a schedule never activate. The provider
    binaries ship inside the same `ghcr.io/finos/5-spot` image (selected by `command:`).

### Using Helm (Coming Soon)

```bash
helm repo add 5spot https://finos.github.io/5-spot
helm install 5spot 5spot/5spot-controller
```

## Manual Deployment

The shipped manifests under [`deploy/deployment/`](https://github.com/finos/5-spot/tree/main/deploy/deployment)
are the source of truth. Apply them step-by-step:

```bash
# 1. Namespace
kubectl apply -f deploy/deployment/namespace.yaml

# 2. RBAC — ServiceAccount, ClusterRole, ClusterRoleBinding
kubectl apply -f deploy/deployment/rbac/

# 3. Workload + supporting objects (ConfigMap, Deployment, Service,
#    NetworkPolicy, PodDisruptionBudget)
kubectl apply -f deploy/deployment/
```

Review the shipped files directly rather than copying YAML from these
docs — the real manifests pin the image to a released tag, configure
the `POD_NAME` / leader-election env vars the controller reads, set
the correct `/healthz` and `/readyz` probe paths, and ship
a least-privilege ClusterRole (which includes bootstrap /
infrastructure / k0smotron / events / secrets / nodes / pods rules
that a hand-written snippet is guaranteed to miss).

## Configuration

### Environment Variables

Defaults shown match `deploy/deployment/deployment.yaml` — the shipped
manifest turns leader election on by default.

| Variable | Default | Description |
|----------|---------|-------------|
| `POD_NAME` | — | Pod name (downward API); leader-election holder identity |
| `POD_NAMESPACE` | — | Pod namespace (downward API) |
| `ENABLE_LEADER_ELECTION` | `true` | Enable leader election for HA |
| `LEASE_NAME` | `5spot-leader` | Lease resource name |
| `LEASE_DURATION_SECONDS` | `15` | Lease validity duration |
| `LEASE_RENEW_DEADLINE_SECONDS` | `10` | Leader must renew within this window |
| `LEASE_RETRY_PERIOD_SECONDS` | `2` | Retry interval when acquiring/renewing |
| `RUST_LOG` | `debug` | Log level (`error`/`warn`/`info`/`debug`/`trace`) |
| `RUST_LOG_FORMAT` | `json` | `json` for SIEM ingestion, `text` for humans |

### High Availability Deployment

The shipped manifest already enables leader election. For HA, bump
replicas (the PodDisruptionBudget guarantees at least one remains
schedulable during voluntary disruptions):

```bash
kubectl scale deployment -n 5spot-system 5spot-controller --replicas=2
```

Or edit `deploy/deployment/deployment.yaml` in your fork / overlay
(`spec.replicas`). Do not copy a stripped-down Deployment here — it
will lose the security context, resource limits, probes, and
pod-anti-affinity that the shipped manifest ships with.

## Verify Deployment

```bash
# Check pods
kubectl get pods -n 5spot-system

# Check logs
kubectl logs -n 5spot-system -l app=5spot-controller

# Check health — the controller exposes /healthz (liveness) and
# /readyz (readiness) on the `health` port (8081).
kubectl port-forward -n 5spot-system svc/controller 8081:8081
curl http://localhost:8081/healthz
curl http://localhost:8081/readyz
```

## Next Steps

- [Quick Start](./quickstart.md) - Create your first ScheduledMachine
- [Configuration](../operations/configuration.md) - Advanced configuration options
- [Monitoring](../operations/monitoring.md) - Set up monitoring
