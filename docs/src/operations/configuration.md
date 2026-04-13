# Configuration

5-Spot can be configured through environment variables and command-line arguments.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OPERATOR_INSTANCE_ID` | `0` | Instance ID for multi-instance deployments |
| `OPERATOR_INSTANCE_COUNT` | `1` | Total number of controller instances |
| `METRICS_PORT` | `8080` | Port for Prometheus metrics endpoint |
| `HEALTH_PORT` | `8081` | Port for health check endpoints |
| `RUST_LOG` | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `RUST_LOG_FORMAT` | `json` | Log format: `json` (production/SIEM) or `text` (local dev) |
| `CONTROLLER_POD_NAME` | _(injected)_ | Pod name injected via `fieldRef`; used as the leader election holder identity and Kubernetes Event reporter |
| `ENABLE_LEADER_ELECTION` | `false` | Enable Kubernetes Lease-based leader election for multi-replica HA |
| `LEASE_NAME` | `5spot-leader` | Name of the Kubernetes `Lease` resource used for leader election |
| `POD_NAMESPACE` | `5spot-system` | Namespace in which to create the leader election `Lease` (injected via `fieldRef`) |
| `LEASE_DURATION_SECONDS` | `15` | How long the Lease is considered valid; a new leader is elected if not renewed in time |
| `LEASE_RENEW_DEADLINE_SECONDS` | `10` | The leader must renew the Lease within this many seconds; grace = duration − deadline |
| `LEASE_RETRY_PERIOD_SECONDS` | `2` | Documented for ops parity; not a direct LeaseManager parameter |

## Command-Line Arguments

```bash
5spot-controller [OPTIONS]

Options:
  --instance-id <ID>                  Instance ID (default: 0)
  --instance-count <COUNT>            Total instances (default: 1)
  --metrics-port <PORT>               Metrics port (default: 8080)
  --health-port <PORT>                Health port (default: 8081)
  --log-format <FORMAT>               Log format: json or text (default: json) [env: RUST_LOG_FORMAT]
  --enable-leader-election            Enable leader election [env: ENABLE_LEADER_ELECTION]
  --lease-name <NAME>                 Lease resource name (default: 5spot-leader) [env: LEASE_NAME]
  --lease-namespace <NS>              Lease namespace (default: 5spot-system) [env: POD_NAMESPACE]
  --lease-duration-secs <SECS>        Lease validity duration (default: 15) [env: LEASE_DURATION_SECONDS]
  --lease-renew-deadline-secs <SECS>  Renew deadline (default: 10) [env: LEASE_RENEW_DEADLINE_SECONDS]
  -v, --verbose                       Enable verbose logging
  -h, --help                          Print help
  -V, --version                       Print version
```

### Log Format

The default `json` format is designed for SIEM ingestion and log aggregation. Switch to `text` for human-readable output during local development:

```bash
# Local development
RUST_LOG=debug RUST_LOG_FORMAT=text cargo run

# Production (default — structured JSON)
RUST_LOG=info RUST_LOG_FORMAT=json ./5spot
```

### Leader Election

When deploying multiple replicas for high availability, enable leader election so only one instance reconciles resources at a time:

```bash
# Multi-replica HA deployment
ENABLE_LEADER_ELECTION=true \
LEASE_DURATION_SECONDS=15 \
LEASE_RENEW_DEADLINE_SECONDS=10 \
./5spot
```

Non-leader replicas watch for leadership changes and take over automatically within one `LEASE_DURATION_SECONDS` window if the leader stops renewing.

> **Note:** Leader election and multi-instance sharding (`OPERATOR_INSTANCE_COUNT > 1`) are alternative HA strategies. Use leader election for active/standby HA; use instance sharding to distribute load across all replicas.

## ConfigMap Example

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: 5spot-config
  namespace: 5spot-system
data:
  OPERATOR_INSTANCE_COUNT: "1"
  ENABLE_LEADER_ELECTION: "true"
  LEASE_NAME: "5spot-leader"
  LEASE_DURATION_SECONDS: "15"
  LEASE_RENEW_DEADLINE_SECONDS: "10"
  METRICS_PORT: "8080"
  HEALTH_PORT: "8081"
  RUST_LOG: "info"
```

## Deployment Configuration

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: 5spot-controller
spec:
  replicas: 2  # HA: 1 active leader + 1 standby
  template:
    spec:
      containers:
        - name: controller
          envFrom:
            - configMapRef:
                name: 5spot-config
          env:
            - name: CONTROLLER_POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
```

## RBAC Configuration

Minimum required permissions:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: 5spot-controller
rules:
  # ScheduledMachine resources
  - apiGroups: ["5spot.finos.org"]
    resources: ["scheduledmachines"]
    verbs: ["get", "list", "watch", "update", "patch"]
  - apiGroups: ["5spot.finos.org"]
    resources: ["scheduledmachines/status"]
    verbs: ["get", "update", "patch"]
  
  # CAPI Machine resources
  - apiGroups: ["cluster.x-k8s.io"]
    resources: ["machines"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  
  # Events for audit trail
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]
  
  # Secrets (if using SSH keys)
  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["get", "list", "watch"]

  # Leases for leader election
  - apiGroups: ["coordination.k8s.io"]
    resources: ["leases"]
    verbs: ["get", "create", "update", "patch"]
```

## Related

- [Monitoring](./monitoring.md) - Metrics and health checks
- [Multi-Instance](./multi-instance.md) - High availability setup
- [Troubleshooting](./troubleshooting.md) - Common issues
