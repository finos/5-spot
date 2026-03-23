# Troubleshooting

Common issues and solutions for 5-Spot.

## Diagnostic Commands

### Check Operator Status

```bash
# Operator pods
kubectl get pods -n 5spot-system

# Operator logs
kubectl logs -n 5spot-system -l app=5spot-controller --tail=100

# Detailed pod info
kubectl describe pod -n 5spot-system -l app=5spot-controller
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

2. **PodDisruptionBudget blocking**
   ```bash
   kubectl get pdb -A
   ```

3. **Long grace period**
   ```bash
   kubectl get scheduledmachine <name> -o jsonpath='{.spec.gracefulShutdownTimeout}'
   ```

**Solution:**
- Check for pods that can't be evicted
- Review PDB settings
- Consider using `killSwitch: true` for immediate removal

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
