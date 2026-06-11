# Delivering a Kata containerd Drop-In

This guide walks through delivering a Kata Containers containerd drop-in to a
scheduled node end-to-end: create the source object, declare `spec.kata`,
verify the host file and the k0s restart, exercise drift-healing, and tear it
down. For the architecture and design rationale, read the
[Kata config delivery concept](../concepts/kata-config-delivery.md) first.

## Prerequisites

- The 5-Spot controller is installed and managing your `ScheduledMachine`
  resources ([Deploying Operator](../installation/controller.md)).
- The `5spot-kata-config-agent` DaemonSet + RBAC are applied **on the workload
  cluster**:

  ```bash
  kubectl apply -k deploy/kata-config-agent/
  ```

  This is a no-op until a node opts in — the DaemonSet's `nodeSelector`
  matches a label only the controller stamps.

- Kata binaries (`/opt/kata`) are already on the node, or installed via
  [kata-deploy](https://github.com/kata-containers/kata-containers/tree/main/tools/packaging/kata-deploy).
  5-Spot delivers *config*, not the runtime.
- The node is k0s-provisioned (containerd consumes drop-ins from
  `/etc/k0s/containerd.d/`). The agent always writes
  `/etc/k0s/containerd.d/kata.toml` — the destination is **not configurable**
  (ADR 0005). `restartService` is overridable (e.g. `k0scontroller.service`).

## Step 1 — Create the source object on the workload cluster

The drop-in content lives in a `ConfigMap` (or `Secret`) **on the workload
cluster**, in `spec.kata.namespace` (default `5spot-system`). 5-Spot never
creates this object — deliver it via your GitOps pipeline (Flux) or apply it
directly:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: kata-drop-in
  namespace: 5spot-system
data:
  kata-containers.toml: |
    [plugins."io.containerd.grpc.v1.cri".containerd.runtimes.kata]
      runtime_type = "io.containerd.kata.v2"
```

If the content is sensitive, use a `Secret` with the same key and set
`kind: Secret` in step 2.

## Step 2 — Declare `spec.kata` on the ScheduledMachine

```bash
kubectl patch scheduledmachine/kata-business-hours-worker \
  --type merge \
  -p '{"spec":{"kata":{"kind":"ConfigMap","name":"kata-drop-in"}}}'
```

Only `kind` and `name` are required. The defaults — `namespace:
5spot-system`, `key: kata-containers.toml`, `restartService:
k0sworker.service` — fit a standard k0s worker. The host destination is
always `/etc/k0s/containerd.d/kata.toml` (fixed, ADR 0005). See
[`examples/scheduledmachine-kata.yaml`](https://github.com/finos/5-spot/blob/main/examples/scheduledmachine-kata.yaml)
for the full form.

!!! warning "First delivery restarts the node's k0s service"
    Delivery is the point of the feature: the first write **always** bounces
    `k0sworker.service`, which restarts containerd and every pod on that node.
    Set `spec.kata` during a window where a one-time pod bounce on that node
    is acceptable.

!!! note "Single-node / controller-runs-workloads layouts"
    On nodes running `k0scontroller` instead of `k0sworker`, set
    `restartService: k0scontroller.service`.

## Step 3 — Watch the delivery converge

When the machine's node reaches Ready, the controller verifies the source
object exists and opts the Node in:

```bash
# The opt-in label appears, then the agent pod lands:
kubectl get nodes -l 5spot.finos.org/kata-config=enabled
kubectl get pods -n 5spot-system -l app=5spot-kata-config-agent -o wide

# Follow the agent: write → applied record → restart
kubectl logs -n 5spot-system -l app=5spot-kata-config-agent -f | jq
```

Expect the agent pod to be **killed and restarted once** — the k0s restart it
issues bounces containerd, which takes the agent down with it. That is the
designed single-cycle convergence, not a crash loop. Steady state:

```bash
kubectl get node <node-name> -o jsonpath='{.metadata.annotations}' | jq '
  with_entries(select(.key | startswith("5spot.finos.org/kata-config")))'
```

The `5spot.finos.org/kata-config-applied` annotation carrying a content hash
(not `"absent"`) means: written, restarted, converged.

On the node itself:

```bash
cat /etc/k0s/containerd.d/kata.toml
systemctl status k0sworker.service   # shows the recent restart
```

## Step 4 — Verify drift self-healing (optional)

Edit the file out-of-band on the node:

```bash
echo "# tampered" >> /etc/k0s/containerd.d/kata.toml
```

Within one poll interval (30 s) the agent rewrites it to match the source —
**without** restarting the service again (the content hash still matches the
applied record). The rewrite shows up in
`fivespot_kata_config_drift_corrected_total` and the agent log.

## Updating the drop-in

Edit the source `ConfigMap` (or point `spec.kata` at a different object/key).
On the next tick the agent writes the new content and restarts the service
exactly once. Each distinct content hash earns exactly one restart per node.

## Tearing down

Clear the field:

```bash
kubectl patch scheduledmachine/kata-business-hours-worker \
  --type merge \
  -p '{"spec":{"kata":null}}'
```

GitOps semantics — absent in source ⇒ absent on host: the agent unlinks the
drop-in, restarts the service once more so containerd drops the config,
removes its own opt-in label, and the DaemonSet pod deschedules. Deleting the
source object (or its `data` key) while `spec.kata` is still set has the same
host-file effect.

## Troubleshooting

| Symptom | Likely cause | Check |
|---|---|---|
| Label never appears on the Node | Source object missing on the **workload** cluster (the controller fails fast and does not opt in) | Controller logs for `SourceNotFound` / `TargetNamespaceMissing`; `kubectl get cm/<name> -n <kata.namespace>` on the workload cluster |
| Agent pod scheduled but no file written | Agent can't read the source (RBAC) — e.g. `spec.kata.namespace` is not `5spot-system` and no Role exists there | Agent logs; add a Role/RoleBinding in that namespace (see the note in `deploy/kata-config-agent/rbac.yaml`) |
| Service restarts repeatedly | Two writers fighting over `/etc/k0s/containerd.d/kata.toml` (e.g. kata-deploy configured to manage the same file, or two `ScheduledMachine`s bound to the same node with different content) | `fivespot_kata_config_restarts_total` rate; ensure exactly one writer owns the file |
| File written but containerd ignores it | This k0s version does not import drop-ins from `/etc/k0s/containerd.d/` | `k0s version`; check the [k0s runtime docs](https://docs.k0sproject.io/stable/runtime/) for your version |
| Stale `…last_sync_timestamp_seconds` | Agent wedged or API unreachable from the node | Agent pod status + logs |

## Related

- [Kata config delivery concept](../concepts/kata-config-delivery.md) — architecture, annotation contract, security posture
- [ScheduledMachine API reference](../reference/api.md) — full `kata` field schema
- [Monitoring](../operations/monitoring.md#kata-config-delivery-node-agent) — agent metrics and alerts
