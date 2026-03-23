# Configuration

5-Spot can be configured through environment variables and command-line arguments.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OPERATOR_INSTANCE_ID` | `0` | Instance ID for multi-instance deployments |
| `OPERATOR_INSTANCE_COUNT` | `1` | Total number of controller instances |
| `METRICS_PORT` | `8080` | Port for Prometheus metrics endpoint |
| `HEALTH_PORT` | `8081` | Port for health check endpoints |
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |

## Command-Line Arguments

```bash
5spot-controller [OPTIONS]

Options:
  --instance-id <ID>       Instance ID (default: 0)
  --instance-count <COUNT> Total instances (default: 1)
  --metrics-port <PORT>    Metrics port (default: 8080)
  --health-port <PORT>     Health port (default: 8081)
  -v, --verbose            Enable verbose logging
  -h, --help               Print help
  -V, --version            Print version
```

## ConfigMap Example

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: 5spot-config
  namespace: 5spot-system
data:
  OPERATOR_INSTANCE_COUNT: "3"
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
  template:
    spec:
      containers:
        - name: controller
          envFrom:
            - configMapRef:
                name: 5spot-config
          env:
            - name: OPERATOR_INSTANCE_ID
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
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
  - apiGroups: ["capi.5spot.io"]
    resources: ["scheduledmachines"]
    verbs: ["get", "list", "watch", "update", "patch"]
  - apiGroups: ["capi.5spot.io"]
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
```

## Related

- [Monitoring](./monitoring.md) - Metrics and health checks
- [Multi-Instance](./multi-instance.md) - High availability setup
- [Troubleshooting](./troubleshooting.md) - Common issues
