# CRD Attack Surface

Per-field validation status and downstream sinks for every
attacker-controllable field in the `ScheduledMachine` CRD. The
intended threat model is a **namespace-scoped tenant** with
`create scheduledmachines.5spot.finos.org` in their own namespace —
the most realistic adversary in a multi-tenant cluster.

> Source of truth for this table is `src/crd.rs` and
> `deploy/admission/validatingadmissionpolicy.yaml`. If you add a new
> field to the CRD, update this page in the same PR.

## Spec fields

| Field | Validation present | Flows to | Status |
|---|---|---|---|
| `spec.clusterName` (string, ≤ 63 chars, ASCII alphanumerics + `-._`) | ✓ VAP rule 1b/1c, ✓ `validate_cluster_name()` | `cluster.x-k8s.io/cluster-name` label on every owned Machine; Prometheus metric labels; log lines | **Bounded**. Phase 1 of the 2026-04-25 audit roadmap is open: add a per-namespace allowlist binding so tenant A cannot join machines to tenant B's cluster. |
| `spec.schedule.enabled` (bool) | ✓ schema type | phase state machine (drives Active ↔ Inactive transitions) | Safe. Boolean enum, no injection surface. |
| `spec.schedule.daysOfWeek[]` (string array of `mon`–`sun` or ranges) | ✓ VAP rule 6 (regex), ✓ `parse_day_ranges()` | schedule evaluator (parsed into a `HashSet<u8>`) | Safe. Bounded grammar; parsed into a typed set. |
| `spec.schedule.hoursOfDay[]` (string array of `0`–`23` or ranges) | ✓ VAP rule 7 (regex), ✓ `parse_hour_ranges()` | schedule evaluator | Safe. Same shape as `daysOfWeek`. |
| `spec.schedule.timezone` (string, ≤ 64 chars, IANA-shape) | ✓ CRD schema regex, ✓ `chrono_tz::Tz::parse()` at runtime | schedule time conversion | Safe. Bounded charset; runtime parse rejects invalid IANA identifiers. |
| `spec.schedule.cron` (string, optional) | ✓ VAP rule 4 / 5 (XOR with day/hour windows), ✓ `parse_cron_expression()` | schedule evaluator | Safe. Mutually exclusive with day/hour windows. |
| `spec.bootstrapSpec.apiVersion` (string) | ✓ VAP rules 8 / 9, ✓ `validate_api_group()` | dynamic resource creation (GVK construction) | Safe. Provider allowlist enforced. |
| `spec.bootstrapSpec.kind` (string, non-empty) | ✓ VAP rule 10 | dynamic resource creation | Safe. Non-empty check; opaque to 5-Spot. |
| **`spec.bootstrapSpec.spec` (arbitrary JSON)** | ✗ **none — by design** | provider-specific bootstrap controller (e.g. k0smotron `K0sWorkerConfig.cloudInit`) | **Pass-through.** Trust boundary is the provider, not 5-Spot. See [Provider payload pass-through](../concepts/scheduled-machine.md#security-provider-payload-pass-through). |
| `spec.infrastructureSpec.apiVersion` (string) | ✓ VAP rules 11 / 12, ✓ `validate_api_group()` | dynamic resource creation | Safe. Provider allowlist. |
| `spec.infrastructureSpec.kind` (string, non-empty) | ✓ VAP rule 13 | dynamic resource creation | Safe. |
| **`spec.infrastructureSpec.spec` (arbitrary JSON)** | ✗ **none — by design** | provider-specific infrastructure controller (e.g. k0smotron `RemoteMachine.address` SSH endpoint) | **Pass-through.** Same trust-boundary argument as `bootstrapSpec.spec`. |
| `spec.machineTemplate.labels` / `.annotations` (string→string maps) | ✓ runtime `validate_labels()` | metadata on the generated CAPI Machine | Safe. Reserved-prefix rejection (`kubernetes.io/`, `k8s.io/`, `cluster.x-k8s.io/`, `5spot.finos.org/`). |
| `spec.killIfCommands` (string array, ≤ 100 items × ≤ 256 chars each) | ✓ VAP rules 1d / 1e, ✓ `validate_kill_if_commands()` | per-node `ConfigMap` (`reclaim.toml`) projected for the reclaim agent | Safe. Both list size and per-entry length bounded. |
| `spec.nodeTaints[]` ({ key, value, effect }) | ✓ VAP rules 14–19, ✓ `validate_node_taints()` | Node `spec.taints` (one per declared entry) | Safe. RFC-1123 qualified names, reserved-prefix rejection (`5spot.finos.org/`, `kubernetes.io/`, `node.kubernetes.io/`, `node-role.kubernetes.io/`), unique on `(key, effect)`. |
| `spec.killSwitch` (bool) | ✓ schema type | emergency-termination phase | Safe. Boolean. |
| `spec.gracefulShutdownTimeout` (string, e.g. `"5m"`) | ✓ VAP rule 2, ✓ `parse_duration()` (≤ 24h) | drain timeout | Safe. Format + 24h cap. |
| `spec.nodeDrainTimeout` (string) | ✓ VAP rule 3, ✓ `parse_duration()` | drain timeout | Safe. Same shape. |
| `spec.priority` (u8) | ✓ schema type (0–255) | consistent-hash assignment for multi-instance distribution | Safe. Range-bounded. |

## Status fields (writable by anyone with `patch scheduledmachines/status`)

| Field | How the controller treats it | Spoof risk |
|---|---|---|
| `status.phase` | Read at the top of every reconcile to dispatch to a phase handler. The reconciler also writes it, so any tampered value is overwritten on the next pass. | Low — phase handlers are idempotent and re-derive their actions from `spec` + canonical CAPI state. |
| `status.nodeRef.name` | **No longer used for routing** as of Phase 3 of the 2026-04-25 audit. The Node→SM watch mapper (`node_to_scheduled_machines_via_machine`) walks the canonical CAPI Machine ownership chain instead. The drain target is read from the canonical `Machine.status.nodeRef` via `get_node_from_machine`. | Low — both the routing and the drain target are now derived from controller-written state. The legacy `node_to_scheduled_machines` symbol is `#[deprecated]` and exported for one release only. |
| `status.machineRef.name` | Used when the controller needs the Machine name for fetch / delete. Always recomputed from the canonical Machine on the same reconcile, so a spoofed value cannot redirect a delete to an unowned Machine. | Low. |
| `status.bootstrapRef.name`, `status.infrastructureRef.name` | Same as `machineRef`. | Low. |
| `status.appliedNodeTaints[]` | Owner-tracking record of which taints the controller has applied. Read on subsequent reconciles to compute "should remove" deltas. | A tenant who patches this could induce taint churn (controller removes taints it didn't apply, or fails to remove ones it did). Confined to the SM's own bound Node — does not pivot to other Nodes. |
| `status.observedGeneration` | Read for debugging only. | None. |

## Summary

- 16 of 18 spec fields have defence-in-depth validation (VAP at admission + runtime check at reconcile).
- The two unvalidated fields (`spec.bootstrapSpec.spec`, `spec.infrastructureSpec.spec`) are **intentional pass-throughs**; the trust boundary is the provider. See the [Provider payload pass-through](../concepts/scheduled-machine.md#security-provider-payload-pass-through) section for the rationale and recommended layered policy.
- Status fields are no longer used for security-critical routing or drain decisions (Phase 3 of the 2026-04-25 audit). A tenant with `patch scheduledmachines/status` can induce small reconcile-loop noise but cannot pivot the controller to act on resources the tenant does not own.

## Related

- [Admission Validation](./admission-validation.md) — the CEL `ValidatingAdmissionPolicy` that enforces these rules at the API server.
- [Threat Model](./threat-model.md) — STRIDE analysis of the controller's trust boundaries.
- [ScheduledMachine — Security: Provider payload pass-through](../concepts/scheduled-machine.md#security-provider-payload-pass-through) — the inline-spec trust boundary in narrative form.
