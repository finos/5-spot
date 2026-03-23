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

## Kubernetes Events

5-Spot creates Kubernetes events for important state changes:

```bash
kubectl get events --field-selector involvedObject.kind=ScheduledMachine
```

Event types:

| Type | Reason | Description |
|------|--------|-------------|
| Normal | MachineScheduled | Machine entered schedule window |
| Normal | MachineActivated | Machine became active |
| Normal | MachineDeactivated | Machine was removed |
| Warning | MachineError | Error during operation |
| Warning | GracePeriodExpired | Grace period timeout |

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
