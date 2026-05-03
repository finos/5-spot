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

All metrics use the `fivespot_` prefix. The full list lives in
`src/metrics.rs`; the table below is the operator-facing summary.

#### Reconciler

| Metric | Type | Labels | Description |
|---|---|---|---|
| `fivespot_reconciliations_total` | Counter | `phase`, `result` | Reconciliation attempts |
| `fivespot_reconciliation_duration_seconds` | Histogram | `phase` | Reconciliation latency (s) |
| `fivespot_machines_active` | Gauge | — | Machines currently in `Active` phase |
| `fivespot_machines_by_phase` | Gauge | `phase` | Machines per lifecycle phase |
| `fivespot_schedule_evaluations_total` | Counter | `result` | Schedule evaluations by outcome |
| `fivespot_kill_switch_activations_total` | Gauge | — | Kill-switch activations |
| `fivespot_controller_info` | Gauge | `version`, `instance_id` | Always 1; carries label metadata |
| `fivespot_is_leader` | Gauge | — | 1 if this instance holds the leader lease |
| `fivespot_errors_total` | Counter | `error_type` | Errors by type |
| `fivespot_finalizer_cleanup_timeouts_total` | Counter | — | Finalizer cleanup timeouts (force-removed; possible orphans) |

#### Node drain & eviction

| Metric | Type | Labels | Description |
|---|---|---|---|
| `fivespot_node_drains_total` | Counter | `result` | Node drain attempts |
| `fivespot_pod_evictions_total` | Counter | `result` | Pod eviction attempts during drain |

#### Emergency reclaim (process-match)

| Metric | Type | Labels | Description |
|---|---|---|---|
| `fivespot_emergency_drain_duration_seconds` | Histogram | `outcome` | Wall-clock duration of emergency-reclaim drains. `outcome={success\|timeout\|error}` |
| `fivespot_emergency_reclaims_total` | Counter | `namespace`, `name` | Emergency-reclaim events fired per ScheduledMachine |
| `fivespot_rapid_re_reclaims_total` | Counter | `namespace`, `name` | `RapidReReclaim` warnings emitted per ScheduledMachine (loop-protection — see [Emergency reclaim concept](../concepts/emergency-reclaim.md)) |

`fivespot_emergency_drain_duration_seconds` buckets are sized for the
60 s `EMERGENCY_DRAIN_TIMEOUT_SECS` ceiling: `[0.5, 1.0, 2.5, 5.0,
10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0]` seconds. The `outcome`
label lets dashboards compute success-only P95 and timeout-rate
side by side without mixing them in the same query.

### Labels

Common labels across metrics:

| Label | Description |
|---|---|
| `phase` | Machine lifecycle phase |
| `result` | Operation result (`success`, `failure`, `error`) |
| `outcome` | Outcome label on emergency-drain histogram (`success`, `timeout`, `error`) |
| `namespace` | Resource namespace (per-SM emergency-reclaim metrics only) |
| `name` | Resource name (per-SM emergency-reclaim metrics only) |
| `error_type` | Error category for `fivespot_errors_total` |
| `version` / `instance_id` | Controller info labels (carried on `fivespot_controller_info`) |

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

### Operator health (leader presence)

```promql
sum(fivespot_is_leader)
```

A value of 0 across all replicas is a paging condition — no instance
holds the lease, no reconciles are running.

### Machines by phase

```promql
sum by (phase) (fivespot_machines_by_phase)
```

### Reconciliation rate

```promql
rate(fivespot_reconciliations_total[5m])
```

### Reconciliation latency (P99)

```promql
histogram_quantile(0.99, rate(fivespot_reconciliation_duration_seconds_bucket[5m]))
```

### Reconciliation failure rate

```promql
rate(fivespot_reconciliations_total{result="failure"}[5m])
```

### Emergency-reclaim drain — success P95

```promql
histogram_quantile(
  0.95,
  rate(fivespot_emergency_drain_duration_seconds_bucket{outcome="success"}[10m])
)
```

Operator SLO: P95 should sit well below 30 s on a healthy fleet. A
growing P95 signals workloads with bad
`terminationGracePeriodSeconds` defaults or PodDisruptionBudgets
that are clipping the drain.

### Emergency-reclaim drain — timeout rate

```promql
sum(rate(fivespot_emergency_drain_duration_seconds_count{outcome="timeout"}[10m]))
```

Any non-zero value means the 60 s `EMERGENCY_DRAIN_TIMEOUT_SECS`
ceiling is biting. Cross-reference with
`fivespot_pod_evictions_total{result="failure"}` to identify the
workloads that wouldn't evict.

### Top emergency-reclaim offenders (per-SM rate)

```promql
topk(10, sum by (namespace, name) (rate(fivespot_emergency_reclaims_total[1h])))
```

The 10 ScheduledMachines emergency-reclaimed most often in the last
hour. A SM that consistently shows up here is a candidate for
`killIfCommands` review — the user's workload may not be a true
"got my box back" emergency.

### `RapidReReclaim` warnings

```promql
sum by (namespace, name) (rate(fivespot_rapid_re_reclaims_total[1h]))
```

Any non-zero rate is operator-actionable: a user is re-enabling a
SM whose conflicting process is still running. Trigger an alert and
follow the "Rapid re-reclaim loop" runbook in
[troubleshooting](./troubleshooting.md).

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
| Warning | `EmergencyReclaim` | Reclaim-agent process-match fired; emergency-remove flow started |
| Warning | `EmergencyReclaimDisabledSchedule` | Step 5 of the flow: `spec.schedule.enabled=false` patched (load-bearing — breaks the eject→re-add→re-eject loop) |
| Warning | `RapidReReclaim` | ≥3 reclaims for the same SM within 10 min — the user is re-enabling without first stopping the conflicting process. See [troubleshooting](./troubleshooting.md) |

Events are written to the `events.k8s.io/v1` API and are immutable once created, providing an auditable state-change trail (SOX §404 / NIST AU-2).

## Alerting Examples

### Prometheus AlertManager Rules

```yaml
groups:
  - name: 5spot
    rules:
      - alert: FiveSpotNoLeader
        expr: sum(fivespot_is_leader) == 0
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "No 5-Spot controller instance holds the leader lease"

      - alert: FiveSpotHighFailureRate
        expr: rate(fivespot_reconciliations_total{result="failure"}[5m]) > 0.1
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "High reconciliation failure rate"

      - alert: FiveSpotSlowReconciliation
        expr: histogram_quantile(0.99, rate(fivespot_reconciliation_duration_seconds_bucket[5m])) > 30
        for: 15m
        labels:
          severity: warning
        annotations:
          summary: "Slow reconciliation detected (P99 > 30 s)"

      - alert: FiveSpotEmergencyDrainTimeoutRising
        expr: |
          sum(rate(fivespot_emergency_drain_duration_seconds_count{outcome="timeout"}[10m])) > 0
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Emergency-reclaim drains hitting the 60 s timeout ceiling"
          description: |
            One or more emergency-reclaim drains failed to evict all pods within
            EMERGENCY_DRAIN_TIMEOUT_SECS (60 s). Cross-reference with
            fivespot_pod_evictions_total{result="failure"} to identify the
            offending workloads.

      - alert: FiveSpotRapidReReclaim
        expr: |
          sum by (namespace, name) (rate(fivespot_rapid_re_reclaims_total[15m])) > 0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "ScheduledMachine {{ $labels.namespace }}/{{ $labels.name }} is in a rapid re-reclaim loop"
          description: |
            ≥3 emergency-reclaim events fired within 10 minutes for the same SM —
            the user is re-enabling the schedule without first stopping the
            conflicting process. See troubleshooting.md "Rapid re-reclaim loop"
            runbook.

      - alert: FiveSpotFinalizerCleanupTimeouts
        expr: rate(fivespot_finalizer_cleanup_timeouts_total[15m]) > 0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Finalizers being force-removed; possible orphan CAPI resources"
```

## Related

- [Configuration](./configuration.md) - Operator configuration
- [Troubleshooting](./troubleshooting.md) - Common issues
- [Multi-Instance](./multi-instance.md) - High availability
