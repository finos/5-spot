# Admission Validation

5-Spot enforces `ScheduledMachine` spec correctness at API-server admission
time using a Kubernetes **`ValidatingAdmissionPolicy`** (VAP).  Invalid
resources are rejected the moment `kubectl apply` is run — before they are
persisted to etcd or ever seen by the reconciler.

!!! info "Regulatory context"
    Admission-time validation satisfies **NIST 800-53 CM-5** (Access
    Restrictions for Change) by ensuring only well-formed, provider-allowlisted
    specs can be created or updated in the cluster.

---

## Background

### ValidatingAdmissionPolicy vs. ValidatingWebhook

Prior to Kubernetes 1.26, admission-time validation required a
`ValidatingWebhook` — a separate HTTPS server that the API server calls for
every matching request.  Running a webhook adds operational complexity:
TLS certificates, a Deployment to manage, potential availability concerns,
and a `failurePolicy` that determines whether the cluster becomes unusable
if the webhook is down.

`ValidatingAdmissionPolicy` (VAP), introduced in Kubernetes 1.26 (alpha),
1.28 (beta), and GA in 1.30, moves the validation logic **inside the API
server** using CEL (Common Expression Language) expressions.  There is no
sidecar to deploy, no TLS to manage, and no additional availability surface.

| Aspect | `ValidatingWebhook` | `ValidatingAdmissionPolicy` |
|---|---|---|
| Runs inside API server | No — separate pod required | **Yes** |
| TLS certificate required | Yes | **No** |
| Availability risk | Yes — webhook outage can block admission | **No** |
| Logic language | Any (HTTP handler) | **CEL expressions** |
| Kubernetes version | All | ≥ 1.26 (alpha), ≥ 1.28 (beta), ≥ 1.30 (GA) |
| Cross-field validation | Yes | **Yes** (via CEL `has()` and combinators) |
| Dynamic parameters | Via `ParamKind` | Yes |

5-Spot uses VAP because it eliminates the operational overhead of a webhook
server while providing equivalent (and in some respects stronger) validation
guarantees.

---

## How Admission Validation Works

```mermaid
sequenceDiagram
    autonumber
    participant U  as kubectl / CI pipeline
    participant API as Kubernetes API Server
    participant VAP as ValidatingAdmissionPolicy<br/>(CEL engine)
    participant etcd

    U  ->> API : CREATE / UPDATE ScheduledMachine
    API ->> VAP : Evaluate structural CEL rules + RBAC authorizer checks
    alt All rules pass
        VAP -->> API : Allowed
        API ->>  etcd : Persist resource
        API -->> U    : 201 Created / 200 OK
    else Any rule fails
        VAP -->> API  : Denied — rule N: <message>
        API -->> U    : 422 Unprocessable Entity
        Note over U   : Resource is NOT persisted.<br/>Error message shown inline.
    end
```

The policy is bound to **all namespaces** by default via a
`ValidatingAdmissionPolicyBinding` with `validationActions: [Deny]`.
The binding can be scoped to specific namespaces if needed — see
[Namespace Scoping](#namespace-scoping).

---

## Validation Rules

The policy contains 13 CEL validation rules.  Every rule must pass for the
request to be admitted.  Rules are evaluated against `object` (the incoming
resource) in the order listed.

| # | Field(s) | Rule | Error message |
|---|---|---|---|
| 1 | `spec.clusterName` | Must not be empty | `spec.clusterName must not be empty` |
| 2 | `spec.gracefulShutdownTimeout` | Must match `^\d+[smh]$` | `must be a duration string such as '5m', '30s', or '1h'` |
| 3 | `spec.nodeDrainTimeout` | Must match `^\d+[smh]$` | `must be a duration string such as '5m', '30s', or '1h'` |
| 4 | `spec.schedule` | Both `daysOfWeek` and `hoursOfDay` must be non-empty | `both daysOfWeek and hoursOfDay must be non-empty` |
| 5 | `spec.schedule.daysOfWeek[]` | Each item matches `mon\|tue\|…` with optional range/combo | `must be day names or ranges (e.g. 'mon', 'mon-fri', 'mon-wed,fri-sun')` |
| 6 | `spec.schedule.hoursOfDay[]` | Each item matches `\d{1,2}(-\d{1,2})?` with optional combo | `must be hours or ranges (e.g. '9', '9-17', '0-9,18-23')` |
| 7 | `spec.bootstrapSpec.apiVersion` | Must contain `/` — core API versions (`v1`) are rejected | `must use a namespaced API group` |
| 8 | `spec.bootstrapSpec.apiVersion` | Group must be `bootstrap.cluster.x-k8s.io` or `k0smotron.io` | `must be from an allowed group` |
| 9 | `spec.bootstrapSpec.kind` | Must not be empty | `spec.bootstrapSpec.kind must not be empty` |
| 10 | `spec.infrastructureSpec.apiVersion` | Must contain `/` | `must use a namespaced API group` |
| 11 | `spec.infrastructureSpec.apiVersion` | Group must be `infrastructure.cluster.x-k8s.io` or `k0smotron.io` | `must be from an allowed group` |
| 12 | `spec.infrastructureSpec.kind` | Must not be empty | `spec.infrastructureSpec.kind must not be empty` |
| 13a | `spec.bootstrapSpec` (RBAC) | Requesting **user** must hold `create` on the embedded bootstrap GVK in the target namespace | `user is not permitted to create the spec.bootstrapSpec resource type …` |
| 13b | `spec.infrastructureSpec` (RBAC) | Requesting **user** must hold `create` on the embedded infrastructure GVK in the target namespace | `user is not permitted to create the spec.infrastructureSpec resource type …` |
| 13c | `spec.bootstrapSpec.metadata.namespace` | Must not be set — controller-owned | `spec.bootstrapSpec.metadata.namespace is not permitted …` |
| 13d | `spec.bootstrapSpec.metadata.name` | Must not be set — controller-owned | `spec.bootstrapSpec.metadata.name is not permitted …` |
| 13e | `spec.infrastructureSpec.metadata.namespace` | Must not be set — controller-owned | `spec.infrastructureSpec.metadata.namespace is not permitted …` |
| 13f | `spec.infrastructureSpec.metadata.name` | Must not be set — controller-owned | `spec.infrastructureSpec.metadata.name is not permitted …` |

!!! note "nodeTaints rules"
    The policy also carries structural `spec.nodeTaints` rules (key format,
    length, reserved-prefix, and duplicate checks). They are omitted from the
    table above for brevity — see the policy YAML for the authoritative list.

### Rule details

#### Rules 2–3 — Duration format

The `gracefulShutdownTimeout` and `nodeDrainTimeout` fields accept strings
of the form `<positive-integer><unit>` where unit is `s`, `m`, or `h`.
These rules enforce the same constraint as `parse_duration()` in the
reconciler, catching malformed values (e.g., `"five minutes"`, `"5 m"`,
`""`) before any reconciliation runs.

```yaml
gracefulShutdownTimeout: "5m"   # ✅  valid
gracefulShutdownTimeout: "30s"  # ✅  valid
gracefulShutdownTimeout: "1h"   # ✅  valid
gracefulShutdownTimeout: "5"    # ❌  rejected — no unit
gracefulShutdownTimeout: "5 m"  # ❌  rejected — space not allowed
gracefulShutdownTimeout: "five" # ❌  rejected — not a number
```

#### Rule 4 — Schedule window must be complete

Both `daysOfWeek` and `hoursOfDay` must be non-empty. A schedule with only
days and no hours (or vice versa) is not meaningful.

```yaml
# ✅ Valid
schedule:
  daysOfWeek: ["mon-fri"]
  hoursOfDay: ["9-17"]
  timezone: "America/Toronto"
  enabled: true

# ❌ Rejected by rule 4 — hoursOfDay missing
schedule:
  daysOfWeek: ["mon-fri"]
  # hoursOfDay missing — rule 4 rejects this
```

#### Rules 7–8, 10–11 — Provider API group allowlist

The `bootstrapSpec.apiVersion` and `infrastructureSpec.apiVersion` fields
must reference an explicitly allowed CAPI provider group.  This mirrors the
`validate_api_group()` runtime check in the reconciler and provides
defence-in-depth: an attacker who can create `ScheduledMachine` resources
cannot use them to create arbitrary Kubernetes resources (e.g.,
`apiVersion: rbac.authorization.k8s.io/v1, kind: ClusterRole`).

| Provider | Allowed `bootstrapSpec.apiVersion` prefix | Allowed `infrastructureSpec.apiVersion` prefix |
|---|---|---|
| Cluster API (upstream) | `bootstrap.cluster.x-k8s.io/` | `infrastructure.cluster.x-k8s.io/` |
| k0smotron | `k0smotron.io/` | `k0smotron.io/` |

To add a new provider, update both the `ValidatingAdmissionPolicy`
(rules 8 and 11) and the constants in `src/constants.rs`
(`ALLOWED_BOOTSTRAP_API_GROUPS`, `ALLOWED_INFRASTRUCTURE_API_GROUPS`).

#### Rules 13a–13b — RBAC privilege-escalation guard

The 5Spot controller runs with broad RBAC so it can create the embedded
bootstrap, infrastructure, and CAPI `Machine` objects on the user's behalf.
Without a guard, a user who can create a `ScheduledMachine` but **not** the
embedded resource directly (e.g. a `K0sWorkerConfig`) could have the
controller create it for them — escalating privileges *through* the
controller. This is the same class of risk that Cluster API's own webhooks
address for templated resources.

Rules 13a and 13b close this by requiring the **requesting user** to
independently hold `create` permission on the embedded bootstrap and
infrastructure GVKs in the target namespace. The policy derives the RBAC
resource from the spec using CEL `variables`:

```yaml
variables:
  - name: bootstrapGroup
    expression: "object.spec.bootstrapSpec.apiVersion.split('/')[0]"
  - name: bootstrapResource
    expression: "object.spec.bootstrapSpec.kind.lowerAscii() + 's'"
  # … infraGroup / infraResource analogous …
```

and then evaluates the request user's permission via the CEL `authorizer`:

```yaml
- expression: >-
    authorizer.group(variables.bootstrapGroup)
      .resource(variables.bootstrapResource)
      .namespace(object.metadata.namespace)
      .check('create')
      .allowed()
  reason: Forbidden
```

The `lowerAscii() + 's'` pluralization mirrors `resource_plural()` in
`src/reconcilers/helpers.rs`, so the permission checked at admission is
exactly the one the controller exercises when it creates the resource.

!!! info "Two-layer defense"
    Rules 13a/13b check the **requesting user** at admission. The
    controller's **own** service account is independently checked at
    reconcile time by `ensure_can_create()` (a `SelfSubjectAccessReview`)
    in `src/reconcilers/helpers.rs`, which fails fast with a clear
    `PermissionDenied` error — naming the denied resource — instead of an
    opaque `403` surfacing partway through resource creation. The two layers
    together cover both *who asked* and *who acts*.

#### Rules 13c–13f — Embedded metadata is controller-owned

The controller owns the **identity** of every resource it creates: each
bootstrap/infrastructure resource is named after the `ScheduledMachine` and
created in the SM's **own namespace** (cross-namespace creation is forbidden —
threat T1; deletion in `remove_machine_from_cluster()` relies on the name
match). Accordingly, a user-supplied `metadata.name` or `metadata.namespace`
in `bootstrapSpec`/`infrastructureSpec` is **rejected**, not silently ignored.

Only `metadata.labels` and `metadata.annotations` are user-settable; the
controller merges them onto the created resource after running them through
the reserved-prefix allowlist (`validate_embedded_metadata()`), so a user
cannot forge `cluster.x-k8s.io/cluster-name` (threat T2) or `5spot.finos.org/*`
keys.

!!! warning "Why `metadata` preserves unknown fields"
    For CRDs, the API server **prunes** unknown fields *before* admission
    policies run — so a naive schema would silently drop
    `metadata.namespace` and rules 13c–13f could never see it. To make a
    *loud* rejection possible, `EmbeddedResource.metadata` is declared
    `x-kubernetes-preserve-unknown-fields: true` in `src/crd.rs`, which keeps
    the field around long enough for the policy (and the runtime
    `validate_embedded_metadata()` backstop) to reject it. A side effect is
    that other unknown `metadata.*` keys are preserved too — they are simply
    ignored, since the controller constructs the resource's `metadata` from
    scratch.

---

## Agent Pod-Security Exception Boundary (workload cluster)

The two 5-Spot node agents deliberately exceed the Pod Security Standards
([ADR 0004](https://github.com/finos/5-spot/blob/main/docs/adr/0004-agent-pod-security-exception-boundary-vap.md)):

| Attribute | kata-config agent | reclaim agent |
|---|---|---|
| `privileged` | ✅ (nsenter k0s restart, ADR 0003) | — |
| `hostPID` | ✅ | ✅ (scan host `/proc`) |
| root (`runAsUser: 0`) | ✅ | ✅ |
| `hostPath` | `/` (RW) | `/proc`, `/etc/machine-id` (RO) |
| added capabilities | — | `NET_ADMIN` |

Clusters enforcing a pod-security baseline (Pod Security Admission, OPA
Gatekeeper, Kyverno) will deny these pods. **Kubernetes admission is
conjunctive — no `ValidatingAdmissionPolicy` can override another engine's
deny** — so the exemption must be granted inside your baseline engine, for
the `5spot-system` namespace:

=== "Pod Security Admission"

    ```bash
    kubectl label namespace 5spot-system \
      pod-security.kubernetes.io/enforce=privileged \
      pod-security.kubernetes.io/audit=privileged \
      pod-security.kubernetes.io/warn=privileged
    ```

=== "OPA Gatekeeper"

    Add `5spot-system` to each relevant constraint's
    `spec.match.excludedNamespaces`, or exempt it cluster-wide via a
    `config.gatekeeper.sh/v1alpha1` `Config` entry.

=== "Kyverno"

    ```yaml
    exclude:
      any:
        - resources:
            namespaces: ["5spot-system"]
    ```

That exemption is namespace-wide — which is the hole the
`5spot-agent-pod-security` policy
(`deploy/admission/agent-pod-security-policy.yaml` + binding) closes. It is a
**deny-by-default compensating guardrail** scoped to pods in `5spot-system`:

- `hostPID`, `hostPath`, explicit root — restricted to the two agent
  ServiceAccounts; `privileged` to the kata-config agent only.
- `hostNetwork` / `hostIPC` — denied for everyone (no 5-Spot component uses them).
- hostPath **clamped per agent**: kata may mount only `/`; reclaim only
  `/proc` and `/etc/machine-id`. Capability adds clamped to `NET_ADMIN` on the
  reclaim agent.
- The compensating controls become **mandatory**: privileged containers must
  keep `readOnlyRootFilesystem: true`; agent pods must keep
  `seccompProfile.type: RuntimeDefault`.
- Ephemeral (debug) containers may never be privileged, add capabilities, or
  run as root — the exception covers the agents' declared workloads, not
  interactive escalation paths.

`failurePolicy: Fail` — the boundary fails closed. Treat the baseline-engine
exemption and this policy as a **paired deployment**: apply the policy + binding
*before* (or in the same change as) the namespace exemption.

---

## Deployment

### Prerequisites

- Kubernetes **≥ 1.26** (alpha — requires feature gate `ValidatingAdmissionPolicy=true`)
- Kubernetes **≥ 1.28** (beta — enabled by default)
- Kubernetes **≥ 1.30** (stable — GA, no feature gate required)

Check your cluster version:

```bash
kubectl version --short
```

For Kubernetes 1.26–1.27, enable the feature gate on the API server:

```yaml
# kube-apiserver flags
--feature-gates=ValidatingAdmissionPolicy=true
```

### Apply the manifests

`deploy/admission/` ships four policies, each with its own binding:

- `validatingadmissionpolicy*.yaml` — validates `ScheduledMachine` CRs.
- `controller-deployment-policy.yaml` + `controller-deployment-binding.yaml`
  — validates the controller's own `Deployment`, enforcing that
  `POD_NAME` is set via downward API and rejecting the deprecated
  `CONTROLLER_POD_NAME` env var with a migration message.
- `child-cluster-kata-runtime-mutatingpolicy.yaml` +
  `child-cluster-kata-runtime-mutatingpolicybinding.yaml` — **applied to
  the child (workload) cluster, not the management cluster.** A
  `MutatingAdmissionPolicy` (`admissionregistration.k8s.io/v1alpha1`,
  Kubernetes >= 1.32) that stamps `katacontainers.io/kata-runtime=true`
  on every Node at kubelet registration time (`CREATE`), which is the
  upstream `kata-deploy` DaemonSet's default `nodeSelector`. DaemonSet
  pod lifecycle naturally gates installation on Node `Ready`, so no
  controller is required. `failurePolicy: Ignore` ensures a policy
  error never blocks Node registration.
- `agent-pod-security-policy.yaml` + `agent-pod-security-binding.yaml` —
  **applied to the child (workload) cluster.** The deny-by-default
  pod-security exception boundary for `5spot-system` described
  [above](#agent-pod-security-exception-boundary-workload-cluster)
  (ADR 0004). Pair it with your baseline engine's namespace exemption.

Apply each policy before its binding (order matters — the binding
references the policy by name). The first two go on the **management**
cluster; the `child-cluster-*` and `agent-pod-security-*` pairs go on the
**child** cluster:

```bash
# Management cluster
kubectl apply -f deploy/admission/validatingadmissionpolicy.yaml
kubectl apply -f deploy/admission/validatingadmissionpolicybinding.yaml
kubectl apply -f deploy/admission/controller-deployment-policy.yaml
kubectl apply -f deploy/admission/controller-deployment-binding.yaml

# Child (workload) cluster
kubectl --kubeconfig <child-kubeconfig> apply \
  -f deploy/admission/child-cluster-kata-runtime-mutatingpolicy.yaml
kubectl --kubeconfig <child-kubeconfig> apply \
  -f deploy/admission/child-cluster-kata-runtime-mutatingpolicybinding.yaml
kubectl --kubeconfig <child-kubeconfig> apply \
  -f deploy/admission/agent-pod-security-policy.yaml
kubectl --kubeconfig <child-kubeconfig> apply \
  -f deploy/admission/agent-pod-security-binding.yaml
```

### Verify the policy is active

```bash
# List the policy and confirm it is accepted
kubectl get validatingadmissionpolicy scheduledmachine-validation

# List the binding
kubectl get validatingadmissionpolicybinding scheduledmachine-validation-binding

# Inspect the policy rules
kubectl describe validatingadmissionpolicy scheduledmachine-validation
```

Expected output includes `Type Ready` condition in the `Status` section.

---

## Rollout Strategy

!!! warning "Use Audit mode during initial rollout"
    Switching directly to `Deny` on an existing cluster may block legitimate
    resources that were created before the policy was deployed.  Always use
    `Audit` mode first to detect violations without blocking traffic.

### Phase 1 — Audit (observe without blocking)

Edit the binding to use `Audit` instead of `Deny`:

```yaml
spec:
  policyName: scheduledmachine-validation
  validationActions: [Audit]   # log violations, do NOT reject
  matchResources:
    namespaceSelector: {}
```

Apply and monitor the API server audit log for `FailedAdmissionValidation`
events:

```bash
kubectl get events -A --field-selector reason=FailedAdmissionValidation
```

Resolve any violations in existing resources before proceeding to phase 2.

### Phase 2 — Deny (enforce)

Once no audit violations are observed, switch to `Deny`:

```yaml
spec:
  validationActions: [Deny]
```

```bash
kubectl apply -f deploy/admission/validatingadmissionpolicybinding.yaml
```

### Phase 3 — AuditAndDeny (belt and braces)

For maximum observability during steady state, use both:

```yaml
validationActions: [Deny, Audit]
```

This blocks invalid requests **and** produces an audit log entry for every
attempted violation, which is useful for SIEM alerting.

---

## Testing

### Test with a valid spec

```bash
kubectl apply -f - <<'EOF'
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: test-valid
  namespace: default
spec:
  clusterName: my-cluster
  schedule:
    daysOfWeek: ["mon-fri"]
    hoursOfDay: ["9-17"]
    timezone: "America/Toronto"
    enabled: true
  bootstrapSpec:
    apiVersion: k0smotron.io/v1beta1
    kind: K0sWorkerConfig
    spec: {}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec: {}
  gracefulShutdownTimeout: "5m"
  nodeDrainTimeout: "10m"
EOF
```

Expected: `scheduledmachine.5spot.finos.org/test-valid created`

### Test invalid duration format (rules 2–3)

```bash
kubectl apply -f - <<'EOF'
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: test-bad-duration
  namespace: default
spec:
  clusterName: my-cluster
  schedule:
    daysOfWeek: ["mon-fri"]
    hoursOfDay: ["9-17"]
    timezone: "UTC"
    enabled: true
  bootstrapSpec:
    apiVersion: k0smotron.io/v1beta1
    kind: K0sWorkerConfig
    spec: {}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec: {}
  gracefulShutdownTimeout: "five minutes"   # ❌ invalid
  nodeDrainTimeout: "10m"
EOF
```

Expected error:

```
The ScheduledMachine "test-bad-duration" is invalid:
  spec.gracefulShutdownTimeout: Invalid value: "five minutes": must be a
  duration string such as '5m', '30s', or '1h' ...
```

### Test forbidden API group (rules 9, 12)

```bash
kubectl apply -f - <<'EOF'
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: test-bad-apigroup
  namespace: default
spec:
  clusterName: my-cluster
  schedule:
    daysOfWeek: ["mon-fri"]
    hoursOfDay: ["9-17"]
    timezone: "UTC"
    enabled: true
  bootstrapSpec:
    apiVersion: rbac.authorization.k8s.io/v1   # ❌ not an allowed group
    kind: ClusterRole
    spec: {}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec: {}
  gracefulShutdownTimeout: "5m"
  nodeDrainTimeout: "10m"
EOF
```

Expected error:

```
The ScheduledMachine "test-bad-apigroup" is invalid:
  spec.bootstrapSpec.apiVersion: Invalid value: ...: must be from an allowed
  group: bootstrap.cluster.x-k8s.io or k0smotron.io
```

### Test incomplete schedule window (rule 4)

```bash
kubectl apply -f - <<'EOF'
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledMachine
metadata:
  name: test-incomplete-schedule
  namespace: default
spec:
  clusterName: my-cluster
  schedule:
    daysOfWeek: ["mon-fri"]
    # hoursOfDay missing — ❌ rejected by rule 4
    timezone: "UTC"
    enabled: true
  bootstrapSpec:
    apiVersion: k0smotron.io/v1beta1
    kind: K0sWorkerConfig
    spec: {}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec: {}
  gracefulShutdownTimeout: "5m"
  nodeDrainTimeout: "10m"
EOF
```

Expected error:

```
The ScheduledMachine "test-incomplete-schedule" is invalid:
  spec.schedule: Invalid value: ...: both daysOfWeek and hoursOfDay must be non-empty
```

---

## Namespace Scoping

By default the binding applies to **all namespaces**.  To restrict enforcement
to specific namespaces, add a `namespaceSelector` to the binding:

```yaml
spec:
  policyName: scheduledmachine-validation
  validationActions: [Deny]
  matchResources:
    namespaceSelector:
      matchLabels:
        5spot.eribourg.dev/managed: "true"
```

Then label the namespaces where `ScheduledMachine` resources are permitted:

```bash
kubectl label namespace my-workload-ns 5spot.eribourg.dev/managed=true
```

!!! tip
    In production environments, combining a `namespaceSelector` on the
    binding with a `ResourceQuota` on the target namespaces (limiting the
    number of `ScheduledMachine` resources per namespace) provides layered
    admission controls with minimal blast radius.

---

## Kubernetes Version Compatibility

| Kubernetes version | VAP status | Action required |
|---|---|---|
| < 1.26 | Not available | Use `ValidatingWebhook` or upgrade cluster |
| 1.26 – 1.27 | Alpha | Enable `--feature-gates=ValidatingAdmissionPolicy=true` on API server |
| 1.28 – 1.29 | Beta — enabled by default | No action required |
| ≥ 1.30 | GA (stable) | No action required |

Check whether VAP is available in your cluster:

```bash
kubectl api-resources | grep validatingadmissionpolic
```

If the command returns results, VAP is available.

---

## See Also

- [Threat Model](threat-model.md) — full STRIDE analysis and residual risks
- [API Reference](../reference/api.md) — complete `ScheduledMachine` field reference
- [CAPI Integration](../advanced/capi-integration.md) — bootstrap and infrastructure provider details
- Kubernetes documentation: [ValidatingAdmissionPolicy](https://kubernetes.io/docs/reference/access-authn-authz/validating-admission-policy/)
