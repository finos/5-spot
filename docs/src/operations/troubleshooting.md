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
  kubectl annotate scheduledmachine <name> 5spot.finos.org/force-reconcile="$(date -u +%s)" --overwrite
  ```

### Orphan resources after finalizer timeout

**Symptom:**
- `fivespot_finalizer_cleanup_timeouts_total` increments above zero.
- A `kubectl describe scheduledmachine <name>` shows a Warning event with reason `FinalizerCleanupTimedOut`.
- The ScheduledMachine has been deleted (gone from `kubectl get`) but a CAPI Machine, bootstrap resource, or infrastructure resource may still exist in the namespace.

**Root cause.**
`handle_deletion` wraps CAPI cleanup in a hard timeout
(`FINALIZER_CLEANUP_TIMEOUT_SECS`, default 600s / 10 minutes) so a hung
eviction cannot stall namespace deletion. By default
(`--force-finalizer-on-timeout=true`, env `FORCE_FINALIZER_ON_TIMEOUT=true`)
the controller force-removes its finalizer when the timeout fires —
unblocking namespace deletion at the cost of potentially leaving CAPI
resources without a managing ScheduledMachine. The most common trigger
is a **misconfigured Pod Disruption Budget** (e.g. `minAvailable: 999`)
on a workload the controller is trying to evict during node drain.

**Runbook.**

1. Find the orphaned CAPI Machine:
   ```bash
   # Machines from this namespace whose owning ScheduledMachine no longer exists.
   kubectl get machines.cluster.x-k8s.io -n <ns> -o json \
     | jq -r '.items[] | select(.metadata.ownerReferences[]?.kind == "ScheduledMachine")
              | .metadata.name'
   for m in $(kubectl get machines.cluster.x-k8s.io -n <ns> -o name); do
     owner=$(kubectl get $m -n <ns> -o jsonpath='{.metadata.ownerReferences[?(@.kind=="ScheduledMachine")].name}')
     if [ -n "$owner" ] && ! kubectl get scheduledmachine "$owner" -n <ns> >/dev/null 2>&1; then
       echo "ORPHAN: $m (was owned by $owner)"
     fi
   done
   ```

2. Identify the bootstrap and infrastructure resources the orphan Machine references:
   ```bash
   kubectl get machine.cluster.x-k8s.io <orphan-name> -n <ns> -o jsonpath='{.spec.bootstrap.configRef}'
   kubectl get machine.cluster.x-k8s.io <orphan-name> -n <ns> -o jsonpath='{.spec.infrastructureRef}'
   ```

3. Delete the orphan Machine first; CAPI cascades into the bootstrap / infra refs via ownerReferences:
   ```bash
   kubectl delete machine.cluster.x-k8s.io <orphan-name> -n <ns>
   ```
   If the Machine itself is stuck terminating (drain still blocked),
   inspect Pods + PDBs on the underlying Node and remove the offending
   PDB before retrying.

4. Verify nothing is left behind:
   ```bash
   kubectl get machines.cluster.x-k8s.io,k0sworkerconfigs.k0smotron.io,remotemachines.k0smotron.io -n <ns>
   ```

**Prevention.**

- Alert on `rate(fivespot_finalizer_cleanup_timeouts_total[5m]) > 0`.
- Validate Pod Disruption Budgets at admission (CEL
  `ValidatingAdmissionPolicy`) — reject `minAvailable` values that exceed
  the workload's replica count.
- For environments where stalled SMs are preferable to potential
  orphans (e.g. a sweep job is in place), set
  `--force-finalizer-on-timeout=false` (env
  `FORCE_FINALIZER_ON_TIMEOUT=false`). The metric and Warning event
  fire in both modes; the only difference is whether the finalizer is
  removed on timeout. **Strict mode requires an external sweep** to
  garbage-collect SMs whose drain is permanently blocked, otherwise
  namespace deletion stalls indefinitely.

## Emergency Reclaim (Kill Switch)

See [Emergency Reclaim](../concepts/emergency-reclaim.md) for the full lifecycle.
This section covers the diagnostic angles most operators hit in the field.

### ScheduledMachine stuck in `EmergencyRemove`

**Symptoms:**

- `kubectl get scheduledmachine` shows `PHASE=EmergencyRemove` and does not move to `Disabled`.
- The node still appears in the cluster.

**Diagnosis:**

```bash
# Is the reclaim annotation still on the Node? (expected during eject, cleared at end)
kubectl get node <node-name> -o jsonpath='{.metadata.annotations}' | jq \
  'with_entries(select(.key | startswith("5spot.finos.org/reclaim")))'

# Controller logs for the emergency-remove handler
kubectl logs -n 5spot-system -l app=5spot-controller --tail=200 | \
  jq -c 'select(.fields.phase == "EmergencyRemove")'

# Events on the ScheduledMachine
kubectl describe scheduledmachine/<name> | grep -A 5 Events
```

**Common causes:**

1. **Drain is blocked by non-evictable pods.** The handler uses `--force --disable-eviction`, so this should be rare — if it happens, a pod is probably stuck in `Terminating` waiting on a finalizer of its own.
2. **CAPI Machine deletion is blocked.** Check `kubectl describe machine/<machine-name>` for a finalizer that has not been cleared.
3. **Controller crashed mid-handler.** On restart the annotation is still there (cleared last), so the handler will retry from the top — the operation is idempotent.

### Node keeps getting ejected every schedule window

**Symptom:** The `ScheduledMachine` cycles `Disabled → Pending → Active → EmergencyRemove → Disabled → ...` at every schedule boundary.

**Cause:** The matched process is still running, the user re-enabled the schedule without quitting it first, and the agent correctly re-fired on the next poll.

**Confirm:**

```bash
# Check what the agent matched on
kubectl logs -n 5spot-system -l app=5spot-reclaim-agent --tail=50 | jq -c 'select(.fields.matched_pattern)'

# Check the condition reason on the ScheduledMachine
kubectl get scheduledmachine/<name> -o jsonpath='{.status.conditions}' | jq \
  '.[] | select(.reason == "EmergencyReclaimDisabledSchedule")'
```

**Solution:** Quit the matched process on the node, then re-enable:

```bash
kubectl patch scheduledmachine/<name> --type merge \
  -p '{"spec":{"schedule":{"enabled":true}}}'
```

If the user does not want this node in the reclaim path at all, clear `killIfCommands`:

```bash
kubectl patch scheduledmachine/<name> --type merge \
  -p '{"spec":{"killIfCommands":null}}'
```

### Reclaim agent never fires on a known-matching process

**Symptoms:** User has a matching process running, but the Node never gets annotated.

**Checklist:**

1. **Is the agent pod actually running on the node?**
   ```bash
   kubectl get pods -n 5spot-system -l app=5spot-reclaim-agent -o wide
   ```
   If no pod lands on the target node, the `5spot.finos.org/reclaim-agent=enabled` label is probably missing. Check the node labels:
   ```bash
   kubectl get node <node-name> --show-labels | grep reclaim-agent
   ```

2. **Is the per-node ConfigMap present and readable?**
   The agent no longer mounts its config from a file — it watches the per-node `ConfigMap` named `reclaim-agent-<NODE_NAME>` in `5spot-system` via the kube API and hot-reloads on every change. Check the ConfigMap directly:
   ```bash
   kubectl get cm -n 5spot-system reclaim-agent-<node-name> -o jsonpath='{.data.reclaim\.toml}'
   ```
   Missing ConfigMap → agent idles (no proc scanning) until one appears. Empty `match_commands` + empty `match_argv_substrings` = agent is armed but inert (never matches) by design. The agent logs `configmap applied — rearming scanner` at INFO on every observed change; tail the pod logs to confirm it sees yours:
   ```bash
   kubectl logs -n 5spot-system <agent-pod> | grep configmap
   ```

3. **Is the agent reading real `/proc`?**
   ```bash
   kubectl exec -n 5spot-system <agent-pod> -- ls /host/proc | head
   ```
   Expect many numeric directory names. If you only see `1` and `self`, the pod's `hostPID: true` mount is broken — re-check the DaemonSet template.

4. **Match is case-sensitive.** `match_commands = ["Java"]` does **not** match a `java` process. Lowercase the pattern to match the typical JVM binary name.

5. **The agent only reads `/proc/<pid>/comm` (exact basename) and `/proc/<pid>/cmdline` (substring).** A process whose `comm` is `java-wrapper` but argv starts with `/opt/jdk/bin/java ...` matches on `cmdline` (substring), not on `comm` (exact).

### `EmergencyReclaim` event fires but schedule is not disabled

**Symptom:** The `EmergencyReclaim` event is on the `ScheduledMachine`, but `spec.schedule.enabled` is still `true`.

**This indicates the controller crashed between the drain/delete steps and the `enabled=false` PATCH.** The Node annotation is cleared *after* the PATCH, so the controller will see the annotation on the next reconcile and retry. If it does not, check:

```bash
# Is the EmergencyReclaimDisabledSchedule event present?
kubectl get events --field-selector reason=EmergencyReclaimDisabledSchedule \
  --sort-by='.lastTimestamp'

# If yes, but spec.schedule.enabled is still true, the PATCH may have lost a race
# with a user edit. Check the generation on the ScheduledMachine:
kubectl get scheduledmachine/<name> -o jsonpath='{.metadata.generation} {.status.observedGeneration}'
```

## Node Taints

### Taints not appearing on Node

`spec.nodeTaints` is declared on a ScheduledMachine, the machine is Active,
but the Node does not have the expected taints. Walk the `NodeTainted`
condition on the CR first — it tells you exactly which layer is failing.

```bash
kubectl get scheduledmachine <name> \
  -o jsonpath='{.status.conditions[?(@.type=="NodeTainted")]}{"\n"}'
```

Three failure reasons, each with its own fix:

**`reason=NoNodeYet` (status=Unknown)**
CAPI populated `status.nodeRef` but the Node object is not yet in the API
server. This is usually a few seconds after Machine creation. The Node watch
will re-enqueue us automatically — no action needed. If stuck for > 1 min,
check that CAPI's Machine actually materialised the Node:

```bash
kubectl get machine <name>-machine -o jsonpath='{.status.nodeRef}{"\n"}'
kubectl get nodes <node-name>
```

**`reason=NodeNotReady` (status=False)**
Node exists but `Ready != True`. Kubelet hasn't registered, networking is
degraded, or CNI is failing. Look at the Node's own conditions first:

```bash
kubectl describe node <node-name> | sed -n '/Conditions:/,/Addresses:/p'
```

Fix the underlying Node problem; the controller will re-reconcile on the next
Node `Ready` transition.

**`reason=TaintOwnershipConflict` (status=False)**
An admin taint exists with the same `(key, effect)` tuple as a declared
`spec.nodeTaints` entry. The controller refuses to overwrite admin-owned
taints. Inspect the current state:

```bash
kubectl get node <node-name> -o jsonpath='{.spec.taints}{"\n"}'
kubectl get node <node-name> \
  -o jsonpath='{.metadata.annotations.5spot\.finos\.org/applied-taints}{"\n"}'
```

Resolve by either removing the admin taint (`kubectl taint nodes <node> key:effect-`)
or changing the `spec.nodeTaints` entry so the `(key, effect)` no longer
collides. Note: the annotation `5spot.finos.org/applied-taints` lists the
keys we own; any taint *not* in that list belongs to the admin.

**`reason=PatchFailed` (status=False)**
A non-404, non-conflict API error on the Node PATCH. Check controller logs for
the exact kube error (RBAC rejection, API server unreachable, etc.):

```bash
kubectl logs -n 5spot-system -l app=5spot-controller \
  | grep -E "node_taint|appliedNodeTaints"
```

The controller retries with exponential backoff on transient failures. For
RBAC issues, confirm the controller ClusterRole grants `patch` on `nodes`.

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
