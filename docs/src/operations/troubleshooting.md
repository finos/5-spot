# Troubleshooting

Common issues and solutions for 5-Spot.

## Diagnostic Commands

### Check Operator Status

```bash
# Operator pods
kubectl get pods -n 5spot-system

# Operator logs (JSON — pipe through jq for readability)
kubectl logs -n 5spot-system -l app=5spot-controller --tail=100 | jq .

# Plain-text logs (for quick reads without jq)
RUST_LOG_FORMAT=text kubectl logs -n 5spot-system -l app=5spot-controller --tail=100

# Detailed pod info
kubectl describe pod -n 5spot-system -l app=5spot-controller
```

### Filter Logs by Correlation ID

Every reconciliation carries a unique `reconcile_id` field. Use it to isolate all log lines for a single reconciliation attempt:

```bash
# Stream logs and filter by resource name, showing reconcile_id
kubectl logs -n 5spot-system -l app=5spot-controller -f | \
  jq -c 'select(.fields.resource == "<machine-name>")'

# Trace a specific reconciliation end-to-end
kubectl logs -n 5spot-system -l app=5spot-controller | \
  jq -c 'select(.fields.reconcile_id == "<id-from-a-previous-log-line>")'

# Find all Error-phase transitions
kubectl logs -n 5spot-system -l app=5spot-controller | \
  jq -c 'select(.fields.to_phase == "Error")'
```

### Check ScheduledMachines

```bash
# List all ScheduledMachines
kubectl get scheduledmachines -A

# Detailed status
kubectl describe scheduledmachine <name>

# Get status as JSON
kubectl get scheduledmachine <name> -o jsonpath='{.status}'
```

### Check CAPI Machines

```bash
# List CAPI machines
kubectl get machines -A

# Describe machine
kubectl describe machine <name>
```

## Common Issues

### Machine Stuck in Pending

**Symptoms:**
- Machine stays in `Pending` phase
- No Machine resource created

**Possible Causes:**

1. **Schedule not matching current time**
   ```bash
   # Check current time vs schedule
   kubectl get scheduledmachine <name> -o jsonpath='{.spec.schedule}'
   date -u  # Compare with UTC
   ```

2. **Operator not running**
   ```bash
   kubectl get pods -n 5spot-system
   ```

3. **RBAC permissions**
   ```bash
   kubectl auth can-i create machines --as=system:serviceaccount:5spot-system:5spot-controller
   ```

**Solution:**
- Verify schedule matches current time and timezone
- Check controller logs for errors
- Ensure RBAC is correctly configured

### Machine Not Removing

**Symptoms:**
- Machine stays in `Active` after schedule window
- Grace period seems to never complete

**Possible Causes:**

1. **Pods not draining**
   ```bash
   kubectl get pods -o wide | grep <machine-name>
   ```

2. **PodDisruptionBudget blocking eviction**

   PDB-blocked evictions (HTTP 429) now surface as a `CapiError` in the reconciler and will cause the machine to enter the `Error` phase. Check for blocking PDBs:
   ```bash
   kubectl get pdb -A
   # Look for PDBs with maxUnavailable: 0 or minAvailable matching current replicas
   kubectl get pdb -A -o json | jq '.items[] | {name:.metadata.name, ns:.metadata.namespace, disruptions:.status.disruptionsAllowed}'
   ```

3. **Long grace period**
   ```bash
   kubectl get scheduledmachine <name> -o jsonpath='{.spec.gracefulShutdownTimeout}'
   ```

**Solution:**
- Check for pods that can't be evicted; look for `warn` log lines with `"Pod eviction blocked by PDB (HTTP 429)"`
- Review PDB settings — temporarily scale up or relax `minAvailable` to allow drain
- Consider using `killSwitch: true` for immediate removal (bypasses drain)

### Schedule Not Evaluating

**Symptoms:**
- Machine doesn't activate during schedule window
- No status changes

**Possible Causes:**

1. **Schedule disabled**
   ```bash
   kubectl get scheduledmachine <name> -o jsonpath='{.spec.schedule.enabled}'
   ```

2. **Timezone mismatch**
   ```bash
   kubectl get scheduledmachine <name> -o jsonpath='{.spec.schedule.timezone}'
   TZ=<timezone> date  # Check time in that timezone
   ```

3. **Multi-instance: wrong instance handling resource**
   ```bash
   # Check which instance should handle this resource
   kubectl logs -n 5spot-system -l app=5spot-controller | grep <resource-name>
   ```

**Solution:**
- Ensure `enabled: true`
- Verify timezone is correct
- Check controller instance distribution

### CAPI Integration Errors

**Symptoms:**
- Error events on ScheduledMachine
- CAPI Machine not being created

**Possible Causes:**

1. **Invalid bootstrapRef or infrastructureRef**
   ```bash
   kubectl get scheduledmachine <name> -o jsonpath='{.spec.bootstrapRef}'
   kubectl get <kind> <name> -n <namespace>  # Verify reference exists
   ```

2. **CAPI provider not ready**
   ```bash
   kubectl get pods -n capi-system
   kubectl get pods -n capi-kubeadm-bootstrap-system
   ```

**Solution:**
- Verify references point to existing resources
- Check CAPI provider health
- Review CAPI controller logs

### Reconciliation Retrying with Increasing Delay

**Symptoms:**
- Repeated error events on a `ScheduledMachine`
- Logs show `retry_count` climbing and `backoff_secs` growing (30 → 60 → 120 → 240 → 300)

**Cause:** The controller uses bounded exponential back-off. Each consecutive failure doubles the retry delay up to 300 s (5 min). The counter resets after a successful reconciliation.

```bash
# Watch the retry_count and backoff_secs fields
kubectl logs -n 5spot-system -l app=5spot-controller -f | \
  jq -c 'select(.fields.resource == "<machine-name>") | {retry: .fields.retry_count, backoff: .fields.backoff_secs, error: .fields.error}'
```

**Solution:**
- Check the underlying error causing repeated failures (CAPI, schedule, validation)
- Once the root cause is fixed, the next successful reconciliation resets the counter
- If the resource is stuck at max backoff (300 s), fix the underlying issue and patch the resource to trigger an immediate reconcile:
  ```bash
  kubectl annotate scheduledmachine <name> 5spot.io/force-reconcile="$(date -u +%s)" --overwrite
  ```

## Error Messages

### "Resource not owned by this instance"

**Cause:** Multi-instance deployment where this resource is assigned to a different instance.

**Solution:** This is expected behavior. Each instance handles a subset of resources.

### "Failed to evaluate schedule"

**Cause:** Invalid schedule configuration.

**Solution:** Check schedule syntax:
- Days: `mon-fri`, not `monday-friday`
- Hours: `9-17`, not `9:00-17:00`
- Timezone: Valid IANA name like `America/New_York`

### "Machine creation failed"

**Cause:** CAPI couldn't create the machine.

**Solution:**
1. Check CAPI logs: `kubectl logs -n capi-system -l control-plane=controller-manager`
2. Verify infrastructure provider is configured
3. Check bootstrap template validity

## Getting Help

### Collect Debug Information

```bash
# Operator version
kubectl get deployment -n 5spot-system 5spot-controller -o jsonpath='{.spec.template.spec.containers[0].image}'

# Full controller logs
kubectl logs -n 5spot-system -l app=5spot-controller --all-containers > controller-logs.txt

# ScheduledMachine YAML
kubectl get scheduledmachine <name> -o yaml > scheduledmachine.yaml

# Events
kubectl get events -A --sort-by='.lastTimestamp' > events.txt
```

### Filing Issues

When filing a GitHub issue, include:

1. 5-Spot version
2. Kubernetes version
3. CAPI version
4. Operator logs (sensitive data redacted)
5. ScheduledMachine YAML
6. Expected vs actual behavior

## Related

- [Configuration](./configuration.md) - Operator configuration
- [Monitoring](./monitoring.md) - Metrics and health checks
- [Machine Lifecycle](../concepts/machine-lifecycle.md) - Understanding phases
