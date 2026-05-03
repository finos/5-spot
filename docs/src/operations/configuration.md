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
| `POD_NAME` | _(injected)_ | Pod name injected via `fieldRef` (downward API); used as the leader-election holder identity and Kubernetes Event reporter |
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

## Reclaim agent (DaemonSet)

The node-side `5spot-reclaim-agent` is a separate binary deployed
via DaemonSet (`deploy/node-agent/daemonset.yaml`). It has its own
flags and environment variables — distinct from the controller — and
is opt-in: nothing happens on a Node until the controller stamps the
`5spot.finos.org/reclaim-agent: enabled` label, which it does only
when a `ScheduledMachine` on that Node has a non-empty
`spec.killIfCommands`. See the [emergency-reclaim concept doc](../concepts/emergency-reclaim.md)
for the full design.

### Environment variables

| Variable | Default | Description |
|---|---|---|
| `NODE_NAME` | _(required, injected via downward API)_ | Name of the Node the agent is running on. The agent only PATCHes this Node. |
| `RECLAIM_PROC_ROOT` | `/proc` | Path the agent treats as `/proc`. Override for sandboxed/test runs only. |
| `RECLAIM_DETECTOR` | `auto` | Process-event source. `auto` picks `netlink` on Linux and `poll` elsewhere. See the [Detector](#detector) subsection below. |
| `MACHINE_ID_PATH` | `/etc/machine-id` | Path the agent reads for the host machine-id (host-identity verification, security-audit Phase 4). The DaemonSet mounts the host file at `/host/etc/machine-id` and sets this to that path. |
| `SKIP_HOST_ID_CHECK` | `false` | If `true`, skip the `Node.status.nodeInfo.machineID` cross-check before PATCH. Use only when `/etc/machine-id` is genuinely unavailable; production must stay strict. |

### Command-line arguments

```bash
5spot-reclaim-agent [OPTIONS]

Options:
  --proc-root <PATH>           Filesystem root mapped to /proc
                                 [default: /proc] [env: RECLAIM_PROC_ROOT]
  --node-name <NAME>           Node to annotate
                                 [env: NODE_NAME]
  --detector <DETECTOR>        Process-event source: auto | netlink | poll
                                 [default: auto] [env: RECLAIM_DETECTOR]
  --machine-id-path <PATH>     Host machine-id file
                                 [default: /etc/machine-id] [env: MACHINE_ID_PATH]
  --skip-host-id-check         Skip the host-identity cross-check before PATCH
                                 (defence-in-depth; default off)
                                 [env: SKIP_HOST_ID_CHECK]
  --oneshot                    Run the detector once and exit
                                 (one-shot tests / smoke verification)
  -h, --help                   Print help
  -V, --version                Print version
```

### Detector

Two detection back-ends ship with the agent. Both produce identical
matches and go through the same Node-PATCH path; only the event
source differs.

| Mode | Mechanism | Latency | Idle CPU | Linux only? | Extra capability |
|---|---|---|---|---|---|
| `poll` | Walks `/proc` every `poll_interval_ms` | up to one poll interval (250 ms default) | ~0 | No | None |
| `netlink` | Subscribes to the kernel proc connector (`PROC_EVENT_EXEC`) | <10 ms (kernel-pushed) | sleeps until kernel wakes it | **Yes** | `CAP_NET_ADMIN` |

`auto` (the default):
- Linux → `netlink`
- macOS / any non-Linux → `poll` (the netlink subscriber's
  constructor returns `Unsupported` on those platforms)

When to **pin `--detector=poll`** explicitly:

- **Heavy-exec workloads** (`make -j32`, compile farms, CI workers)
  — `netlink` sees every short-lived process even if it exits in
  microseconds; `poll` only sees processes that survive to the
  next tick. Under exec storms `poll` can be cheaper.
- **`CAP_NET_ADMIN` is unacceptable** in your environment (PSA
  `restricted` profile, hardened cluster policy). The cap is
  granted only on opted-in nodes via the DaemonSet's pod-level
  `securityContext`, but you may have organisational reasons to
  keep it dropped.
- **Kernel without `CONFIG_PROC_EVENTS`** (very rare; some
  embedded / hardened distros). `netlink` socket opens cleanly
  but no events are ever delivered. See
  [troubleshooting](./troubleshooting.md#reclaim-agent-runs-but-never-observes-any-events).

Override at deploy time:

```bash
# Switch a running DaemonSet to poll mode (no pod restart needed —
# the agent watches its per-node ConfigMap, but env changes need a
# rollout; use kubectl set env to trigger one):
kubectl set env -n 5spot-system ds/5spot-reclaim-agent \
  RECLAIM_DETECTOR=poll
```

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
            - name: POD_NAME
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
