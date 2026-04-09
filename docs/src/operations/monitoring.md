# Monitoring

5-Spot provides comprehensive monitoring through Prometheus metrics and health endpoints.

## Health Endpoints

### Liveness Probe

```
GET /health
Port: 8081 (default)
```

Returns `200 OK` if the controller is alive.

### Readiness Probe

```
GET /ready
Port: 8081 (default)
```

Returns `200 OK` if the controller is ready to accept work.

### Kubernetes Configuration

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 8081
  initialDelaySeconds: 5
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /ready
    port: 8081
  initialDelaySeconds: 5
  periodSeconds: 10
```

## Prometheus Metrics

### Endpoint

```
GET /metrics
Port: 8080 (default)
```

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `five_spot_up` | Gauge | Operator health (1 = healthy) |
| `five_spot_reconciliations_total` | Counter | Total reconciliations by phase and result |
| `five_spot_reconciliation_duration_seconds` | Histogram | Reconciliation duration |
| `five_spot_machines_total` | Gauge | Total machines by phase |
| `five_spot_schedule_evaluations_total` | Counter | Schedule evaluations performed |

### Labels

Common labels across metrics:

| Label | Description |
|-------|-------------|
| `phase` | Machine lifecycle phase |
| `result` | Operation result (success, error) |
| `namespace` | Resource namespace |
| `name` | Resource name |

## ServiceMonitor (Prometheus Operator)

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: 5spot-controller
  namespace: monitoring
spec:
  selector:
    matchLabels:
      app: 5spot-controller
  endpoints:
    - port: metrics
      interval: 30s
      path: /metrics
  namespaceSelector:
    matchNames:
      - 5spot-system
```

## Grafana Dashboard

Example queries for a Grafana dashboard:

### Operator Health

```promql
five_spot_up
```

### Machines by Phase

```promql
sum by (phase) (five_spot_machines_total)
```

### Reconciliation Rate

```promql
rate(five_spot_reconciliations_total[5m])
```

### Reconciliation Latency (p99)

```promql
histogram_quantile(0.99, rate(five_spot_reconciliation_duration_seconds_bucket[5m]))
```

### Error Rate

```promql
rate(five_spot_reconciliations_total{result="error"}[5m])
```

## Structured Logging

### Log Format

Logs are emitted as structured JSON by default (controlled by `RUST_LOG_FORMAT`). Every log line carries standard fields including a **`reconcile_id`** correlation field that is unique per reconciliation attempt:

```json
{
  "timestamp": "2026-04-09T00:00:00.123456Z",
  "level": "INFO",
  "fields": {
    "message": "Starting reconciliation",
    "reconcile_id": "deadbeef0001-17f3e2a1b",
    "resource": "my-machine",
    "namespace": "production"
  },
  "target": "five_spot::reconcilers::scheduled_machine",
  "span": { "name": "reconcile" }
}
```

### Correlation IDs

The `reconcile_id` field ties together every log line produced during a single reconciliation. Use it to trace a full reconciliation end-to-end in your log aggregation platform:

```bash
# Follow all log lines for a specific reconciliation (jq)
kubectl logs -n 5spot-system -l app=5spot-controller | \
  jq -c 'select(.fields.reconcile_id == "deadbeef0001-17f3e2a1b")'

# Find all reconciliations for a specific resource
kubectl logs -n 5spot-system -l app=5spot-controller | \
  jq -c 'select(.fields.resource == "my-machine")'

# Find all error-phase transitions
kubectl logs -n 5spot-system -l app=5spot-controller | \
  jq -c 'select(.fields.to_phase == "Error")'
```

### Phase Transition Logs

Every phase transition logs both the before (`from_phase`) and after (`to_phase`) values:

```json
{
  "level": "INFO",
  "fields": {
    "message": "Phase transition",
    "from_phase": "Pending",
    "to_phase": "Active",
    "reconcile_id": "deadbeef0001-17f3e2a1b",
    "resource": "my-machine",
    "namespace": "production"
  }
}
```

### Error Back-off Log Fields

When a reconciliation fails, the error policy emits an `error`-level log line with two additional fields:

| Field | Type | Description |
|-------|------|-------------|
| `retry_count` | u32 | How many consecutive failures have occurred for this resource |
| `backoff_secs` | u64 | Requeue delay chosen for this retry (30 s → 60 → 120 → 240 → 300 s cap) |

```json
{
  "level": "ERROR",
  "fields": {
    "message": "Reconciliation error — requeuing with exponential back-off",
    "error": "CAPI operation failed: ...",
    "retry_count": 3,
    "backoff_secs": 240,
    "resource": "my-machine",
    "namespace": "production"
  }
}
```

The retry count resets to 0 after a successful reconciliation, so a resource that recovers starts fresh on the next failure.

### Log Levels

| Level | Use |
|-------|-----|
| `error` | Unrecoverable failures — always investigate |
| `warn` | Recoverable issues (PDB-blocked eviction, event publish failure) |
| `info` | Phase transitions, reconciliation start/end |
| `debug` | Per-pod decisions, API call details |
| `trace` | Internal state, schedule evaluation |

Set via `RUST_LOG`:

```bash
RUST_LOG=info,kube=warn,hyper=warn  # Production default
RUST_LOG=debug                       # Verbose (--verbose flag)
```

## Kubernetes Events

5-Spot publishes a Kubernetes Event for every phase transition, visible via:

```bash
kubectl describe scheduledmachine <name>
# or
kubectl get events --field-selector involvedObject.kind=ScheduledMachine
```

Event types and reasons:

| Type | Reason | Trigger |
|------|--------|---------|
| Normal | `MachineCreated` | Transition to Active — CAPI resources provisioned |
| Normal | `ScheduleActive` | Machine entered schedule window |
| Normal | `ScheduleInactive` | Machine exited schedule window |
| Normal | `GracePeriodActive` | Graceful shutdown countdown started |
| Normal | `NodeDraining` / `NodeDrained` | Node drain start / completion |
| Normal | `MachineDeleted` | Transition to Inactive — CAPI resources removed |
| Normal | `ScheduleDisabled` | Schedule disabled, machine deactivated |
| Warning | `ReconcileFailed` | Unrecoverable error — machine in Error phase |
| Warning | `KillSwitchActivated` | Emergency kill switch triggered |

Events are written to the `events.k8s.io/v1` API and are immutable once created, providing an auditable state-change trail (SOX §404 / NIST AU-2).

## Alerting Examples

### Prometheus AlertManager Rules

```yaml
groups:
  - name: 5spot
    rules:
      - alert: FiveSpotOperatorDown
        expr: five_spot_up == 0
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "5-Spot controller is down"
          
      - alert: FiveSpotHighErrorRate
        expr: rate(five_spot_reconciliations_total{result="error"}[5m]) > 0.1
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "High reconciliation error rate"
          
      - alert: FiveSpotSlowReconciliation
        expr: histogram_quantile(0.99, rate(five_spot_reconciliation_duration_seconds_bucket[5m])) > 30
        for: 15m
        labels:
          severity: warning
        annotations:
          summary: "Slow reconciliation detected"
```

## Related

- [Configuration](./configuration.md) - Operator configuration
- [Troubleshooting](./troubleshooting.md) - Common issues
- [Multi-Instance](./multi-instance.md) - High availability
