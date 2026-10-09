// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # CRD API documentation generator
//!
//! Offline tool that emits a Markdown API reference for the `ScheduledMachine`
//! and `ScheduledCapacity`
//! custom resource to `stdout`.  The output is committed to
//! `docs/src/reference/api.md` so that documentation consumers do not need a
//! running Rust toolchain.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --bin crddoc > docs/src/reference/api.md
//! ```
//!
//! Re-run this binary whenever the `ScheduledMachine` spec changes (fields
//! added/removed, descriptions updated) and commit the refreshed Markdown.
//! The `regen-api-docs` skill in `.claude/SKILL.md` automates this step.
//!
//! ## Implementation note
//! The documentation is generated as static `println!` calls rather than
//! derived from the JSON Schema.  Full schema-driven generation is deferred
//! pending the CAPI integration update (see TODO comment at the top of
//! `main()`).

// TODO: Re-enable when CRD documentation generation is updated for CAPI
// use five_spot::crd::ScheduledMachine;
// use kube::CustomResourceExt;

/// Emit the `ScheduledMachine` API reference as Markdown to `stdout`.
#[allow(clippy::too_many_lines)]
fn main() {
    println!("# 5Spot API Reference");
    println!();
    println!("## ScheduledMachine");
    println!();
    println!("The `ScheduledMachine` custom resource defines a machine that should be");
    println!(
        "automatically added to and removed from a k0smotron cluster based on a time schedule."
    );
    println!();
    println!("### API Group and Version");
    println!();
    println!("- **API Group**: `5spot.finos.org`");
    println!("- **API Version**: `v1beta1` (single served/storage version)");
    println!("- **Kind**: `ScheduledMachine`");
    println!();
    println!("> Since ADR 0009, `spec.schedule` is a **required reference** to a spot-schedule");
    println!("> provider object (in the `spotschedules.5spot.finos.org` group) that owns the");
    println!("> machine's active/inactive decision. The former inline time window is now the");
    println!("> first-party `TimeBasedSpotSchedule` provider; `CapitalMarketsSchedule` and");
    println!("> third-party providers are referenced the same way. The pre-release `v1alpha1`");
    println!("> `ScheduledMachine` version was dropped (ADR 0009 amends ADR 0007).");
    println!();
    println!("### Example");
    println!();
    println!("```yaml");
    println!("apiVersion: 5spot.finos.org/v1beta1");
    println!("kind: ScheduledMachine");
    println!("metadata:");
    println!("  name: example-spot-machine");
    println!("  namespace: default");
    println!("spec:");
    println!("  clusterName: my-cluster");
    println!("  enabled: true");
    println!("  # Required: reference the spot-schedule provider that owns the");
    println!("  # active/inactive decision (ADR 0009). The default is TimeBasedSpotSchedule.");
    println!("  schedule:");
    println!("    apiVersion: spotschedules.5spot.finos.org/v1alpha1");
    println!("    kind: TimeBasedSpotSchedule");
    println!("    name: weekdays-9-5");
    println!("  bootstrapSpec:");
    println!("    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1");
    println!("    kind: K0sWorkerConfig");
    println!("    spec:");
    println!("      version: v1.32.8+k0s.0");
    println!("      downloadURL: https://github.com/k0sproject/k0s/releases/download/v1.32.8+k0s.0/k0s-v1.32.8+k0s.0-amd64");
    println!("  infrastructureSpec:");
    println!("    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1");
    println!("    kind: RemoteMachine");
    println!("    spec:");
    println!("      address: 192.168.1.100");
    println!("      port: 22");
    println!("      user: root");
    println!("      sshKeyRef:");
    println!("        name: my-ssh-key");
    println!("  machineTemplate:");
    println!("    labels:");
    println!("      node-role.kubernetes.io/worker: spot");
    println!("    annotations:");
    println!("      example.com/scheduled-by: 5spot");
    println!("  priority: 50");
    println!("  gracefulShutdownTimeout: 5m");
    println!("  nodeDrainTimeout: 5m");
    println!("  killSwitch: false");
    println!("  killIfCommands:");
    println!("    - java");
    println!("    - idea");
    println!("  nodeTaints:");
    println!("    - key: workload");
    println!("      value: batch");
    println!("      effect: NoSchedule");
    println!("  kata:");
    println!("    kind: ConfigMap");
    println!("    name: kata-drop-in");
    println!("```");
    println!();
    println!("The referenced provider object lives in the same namespace. The default,");
    println!("first-party provider is `TimeBasedSpotSchedule`:");
    println!();
    println!("```yaml");
    println!("apiVersion: spotschedules.5spot.finos.org/v1alpha1");
    println!("kind: TimeBasedSpotSchedule");
    println!("metadata:");
    println!("  name: weekdays-9-5");
    println!("  namespace: default");
    println!("spec:");
    println!("  daysOfWeek:");
    println!("    - mon-fri");
    println!("  hoursOfDay:");
    println!("    - 9-17");
    println!("  timezone: America/New_York");
    println!("  enabled: true");
    println!("```");
    println!();
    println!("### Spec Fields");
    println!();
    println!("#### schedule");
    println!();
    println!("(required, object) Reference to the spot-schedule provider object that owns this");
    println!("machine's active/inactive decision (ADR 0009). The 5-Spot controller watches the");
    println!(
        "referenced object and reads only its duck-typed `status.active` (and `Ready` condition)"
    );
    println!("— never the provider `spec`, and it never writes the provider object. The provider");
    println!("verdict is the machine's should-be-active decision; `spec.enabled` and `killSwitch`");
    println!("override it.");
    println!();
    println!(
        "- **apiVersion** (required, string): `group/version` of the provider. The group MUST"
    );
    println!("  be `spotschedules.5spot.finos.org` (CEL-pinned); any served version is accepted.");
    println!("- **kind** (required, string): Provider kind, e.g. `TimeBasedSpotSchedule` (the");
    println!("  default) or `CapitalMarketsSchedule`.");
    println!(
        "- **name** (required, string): Provider object name in **this machine's namespace**."
    );
    println!("  Cross-namespace references are not supported.");
    println!();
    println!("See the [Spot Schedule Provider Contract](spot-schedule-contract.md) for the full");
    println!("contract a provider implements, plus the `TimeBasedSpotSchedule` and");
    println!("`CapitalMarketsSchedule` first-party providers.");
    println!();
    println!("#### enabled");
    println!();
    println!("(optional, boolean, default: `true`) Administrative master switch for this machine");
    println!("(ADR 0009). When `false` the machine is held **Disabled** regardless of what its");
    println!("`schedule` provider reports — the SM-scoped on/off operators reach for, and the");
    println!("loop-breaker the emergency-reclaim flow sets. Distinct from the provider's own");
    println!("`status.active` and from `killSwitch` (immediate, terminal teardown).");
    println!();
    println!("#### clusterName");
    println!();
    println!("(required, string) Name of the CAPI cluster this machine belongs to.");
    println!();
    println!("#### bootstrapSpec");
    println!();
    println!("(required, object) Inline bootstrap configuration that will be created when the schedule is active.");
    println!("This is a fully unstructured object that must contain:");
    println!();
    println!("- **apiVersion** (required, string): API version of the bootstrap resource (e.g., `bootstrap.cluster.x-k8s.io/v1beta1`)");
    println!("- **kind** (required, string): Kind of the bootstrap resource (e.g., `K0sWorkerConfig`, `KubeadmConfig`)");
    println!(
        "- **spec** (required, object): Provider-specific configuration for the bootstrap resource"
    );
    println!();
    println!(
        "The controller validates that the apiVersion belongs to an allowed bootstrap API group."
    );
    println!();
    println!("It may also include an optional `metadata` block:");
    println!();
    println!("- **metadata.labels** (optional, map of string to string): merged onto the created bootstrap resource");
    println!("- **metadata.annotations** (optional, map of string to string): merged onto the created bootstrap resource");
    println!();
    println!("`metadata.name` and `metadata.namespace` are **not** permitted — the controller");
    println!("names the resource after the ScheduledMachine and creates it in the SM's own");
    println!("namespace. Labels/annotations using reserved prefixes (`5spot.finos.org/`,");
    println!("`cluster.x-k8s.io/`, `kubernetes.io/`, `k8s.io/`) are rejected.");
    println!();
    println!("#### infrastructureSpec");
    println!();
    println!("(required, object) Inline infrastructure configuration that will be created when the schedule is active.");
    println!("This is a fully unstructured object that must contain:");
    println!();
    println!("- **apiVersion** (required, string): API version of the infrastructure resource (e.g., `infrastructure.cluster.x-k8s.io/v1beta1`)");
    println!("- **kind** (required, string): Kind of the infrastructure resource (e.g., `RemoteMachine`, `AWSMachine`)");
    println!("- **spec** (required, object): Provider-specific configuration for the infrastructure resource");
    println!();
    println!("The controller validates that the apiVersion belongs to an allowed infrastructure API group.");
    println!();
    println!("It may also include an optional `metadata` block:");
    println!();
    println!("- **metadata.labels** (optional, map of string to string): merged onto the created infrastructure resource");
    println!("- **metadata.annotations** (optional, map of string to string): merged onto the created infrastructure resource");
    println!();
    println!("`metadata.name` and `metadata.namespace` are **not** permitted — the controller");
    println!("names the resource after the ScheduledMachine and creates it in the SM's own");
    println!("namespace. Labels/annotations using reserved prefixes (`5spot.finos.org/`,");
    println!("`cluster.x-k8s.io/`, `kubernetes.io/`, `k8s.io/`) are rejected.");
    println!();
    println!("#### machineTemplate");
    println!();
    println!("(optional, object) Configuration for the created CAPI Machine resource.");
    println!();
    println!(
        "- **labels** (optional, map of string to string): Labels to apply to the created Machine"
    );
    println!("- **annotations** (optional, map of string to string): Annotations to apply to the created Machine");
    println!();
    println!("Note: Labels and annotations using reserved prefixes (`5spot.finos.org/`, `cluster.x-k8s.io/`) are rejected.");
    println!();
    println!("#### priority");
    println!();
    println!("(optional, integer 0-255, default: `50`) Priority for machine scheduling.");
    println!("Higher values indicate higher priority. Used for resource distribution across");
    println!("operator instances.");
    println!();
    println!("#### gracefulShutdownTimeout");
    println!();
    println!("(optional, string, default: `5m`) Timeout for graceful machine shutdown.");
    println!(
        "Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours)."
    );
    println!();
    println!("#### nodeDrainTimeout");
    println!();
    println!("(optional, string, default: `5m`) Timeout for draining the node before deletion.");
    println!(
        "Format: `<number><unit>` where unit is `s` (seconds), `m` (minutes), or `h` (hours)."
    );
    println!();
    println!("#### killSwitch");
    println!();
    println!("(optional, boolean, default: `false`) When true, immediately removes the machine");
    println!("from the cluster and takes it out of rotation, bypassing the grace period.");
    println!();
    println!("#### killIfCommands");
    println!();
    println!(
        "(optional, array of strings) Process patterns that trigger an emergency node reclaim."
    );
    println!("When non-empty, the 5-Spot controller installs the `5spot-reclaim-agent` DaemonSet");
    println!("on every Node backing this `ScheduledMachine`. The agent watches `/proc` for any");
    println!("process whose basename or argv matches one of these patterns and, on first match,");
    println!("annotates the Node to request immediate (non-graceful) removal from the cluster.");
    println!();
    println!(
        "When absent or empty, no agent is installed and behaviour is time-based scheduling only."
    );
    println!("Patterns are evaluated against both `/proc/<pid>/comm` (exact basename) and");
    println!("`/proc/<pid>/cmdline` (substring).");
    println!();
    println!("#### nodeTaints");
    println!();
    println!("(optional, array of NodeTaint, default: `[]`) User-defined taints applied to the");
    println!("Kubernetes Node once it is Ready. The controller owns and reconciles only the");
    println!("taints it applied (tracked in `status.appliedNodeTaints` plus the");
    println!("`5spot.finos.org/applied-taints` annotation on the Node). Admin-added taints on");
    println!("the same Node are left untouched. Taint identity is the tuple `(key, effect)`;");
    println!("`value` is mutable.");
    println!();
    println!("Each `NodeTaint` has the following fields:");
    println!();
    println!("- **key** (required, string): RFC-1123 qualified name. Max 253 chars total;");
    println!("  name-part ≤ 63. Reserved prefixes rejected at admission: `5spot.finos.org/`,");
    println!("  `kubernetes.io/`, `node.kubernetes.io/`, `node-role.kubernetes.io/`.");
    println!("- **value** (optional, string): Optional value, ≤ 63 chars. Mutable — changing");
    println!("  the value on an existing taint triggers an update, not an add/remove.");
    println!(
        "- **effect** (required, enum): One of `NoSchedule`, `PreferNoSchedule`, `NoExecute`."
    );
    println!();
    println!("Duplicate `(key, effect)` pairs are rejected at admission. Admin-added taints");
    println!("colliding on `(key, effect)` are surfaced as a `TaintOwnershipConflict` condition");
    println!("rather than overwritten.");
    println!();
    println!("#### kata");
    println!();
    println!("(optional, KataConfig) Reference to a `Secret` or `ConfigMap` **on the workload");
    println!("cluster** holding a Kata containerd drop-in to deliver to the node(s) this resource");
    println!("owns. When set, the controller resolves the object on the workload cluster (via the");
    println!("`kubeconfig-<clusterName>` Secret) in `kata.namespace` (default `5spot-system`). If");
    println!("present, it stamps the `5spot.finos.org/kata-config=enabled` opt-in label plus a");
    println!("reference annotation on the Node; the `5spot-kata-config-agent` DaemonSet reads the");
    println!("object from the workload API, writes the drop-in to the fixed host path");
    println!("`/etc/k0s/containerd.d/kata.toml` (not configurable — ADR 0005), and restarts");
    println!("`restartService` so containerd reloads it. If the object (or its namespace) is");
    println!("absent, the controller does NOT label the Node and reports a fail-fast status");
    println!("condition — 5-Spot never creates the object (it must pre-exist, Flux-delivered).");
    println!("This is config delivery, not a Kata install — `/opt/kata` binaries remain");
    println!("`kata-deploy`'s job. See ADR 0002 and ADR 0003.");
    println!();
    println!("`KataConfig` has the following fields:");
    println!();
    println!("- **kind** (required, enum): One of `ConfigMap`, `Secret` — the source kind.");
    println!("- **name** (required, string): Source object name on the workload cluster,");
    println!("  RFC-1123 DNS subdomain (≤ 253 chars).");
    println!("- **namespace** (optional, string, default: `5spot-system`): workload-cluster");
    println!("  namespace the agent reads the object from. Override for per-tenant placement.");
    println!("- **key** (optional, string, default: `kata-containers.toml`): `data` key whose");
    println!("  value is the drop-in content.");
    println!("- **restartService** (optional, string, default: `k0sworker.service`): systemd");
    println!("  unit restarted via `nsenter` so containerd reloads the drop-in. Override with");
    println!("  `k0scontroller.service` on single-node layouts.");
    println!();
    println!("### Status Fields");
    println!();
    println!("#### phase");
    println!();
    println!("Current phase of the machine lifecycle. Possible values:");
    println!();
    println!("- **Pending**: Initial state, awaiting schedule evaluation");
    println!("- **Active**: Machine is running and part of the cluster");
    println!("- **ShuttingDown**: Machine is being gracefully removed (draining, etc.)");
    println!("- **Inactive**: Machine is outside scheduled time window and has been removed");
    println!("- **Disabled**: Schedule is disabled, machine is not active");
    println!("- **Terminated**: Machine has been permanently removed");
    println!("- **Error**: An error occurred during processing");
    println!();
    println!("#### conditions");
    println!();
    println!("Array of condition objects with the following fields:");
    println!();
    println!("- **type**: Condition type (e.g., `Ready`, `Scheduled`, `MachineReady`)");
    println!("- **status**: `True`, `False`, or `Unknown`");
    println!("- **reason**: One-word reason in CamelCase");
    println!("- **message**: Human-readable message");
    println!("- **lastTransitionTime**: Last time the condition transitioned");
    println!();
    println!("#### inSchedule");
    println!();
    println!("(boolean) Whether the machine is currently within its scheduled time window.");
    println!();
    println!("#### ready");
    println!();
    println!(
        "(boolean) `True` only when `phase` is `Active`. Surfaced as the `Ready` printer column"
    );
    println!("for fast operator triage — any other phase (`Pending`, `ShuttingDown`, `Inactive`,");
    println!("`Disabled`, `Terminated`, `Error`) is reported as `False`.");
    println!();
    println!("#### message");
    println!();
    println!("(string) Human-readable message describing the current state.");
    println!();
    println!("#### observedGeneration");
    println!();
    println!("(integer) The generation observed by the controller. Used for change detection.");
    println!();
    println!("#### providerID");
    println!();
    println!(
        "(optional, string) Provider-assigned machine identifier, copied from the CAPI Machine's"
    );
    println!(
        "`spec.providerID`. Stable for the life of the machine and unique across the cluster."
    );
    println!("Examples: `libvirt:///uuid-abc-123`, `aws:///us-east-1a/i-0abcd1234`.");
    println!();
    println!("#### nodeRef");
    println!();
    println!(
        "(optional, object) Reference to the Kubernetes Node once the Machine is provisioned."
    );
    println!("Mirrors the shape of CAPI's `Machine.status.nodeRef`:");
    println!();
    println!(
        "- **apiVersion** (required, string): API version of the Node resource (typically `v1`)"
    );
    println!("- **kind** (required, string): Kind of the referenced object (typically `Node`)");
    println!("- **name** (required, string): Name of the Node");
    println!("- **uid** (optional, string): UID of the Node, protecting against name reuse");
    println!();
    println!("#### appliedNodeTaints");
    println!();
    println!("(optional, array of NodeTaint, default: `[]`) The controller's record of truth");
    println!("for which taints it applied to the Node. Only entries in this list are eligible");
    println!("for removal on a subsequent reconcile — admin-added taints colliding on");
    println!("`(key, effect)` are surfaced as a `TaintOwnershipConflict` condition rather than");
    println!("overwritten.");
    println!();
    println!("See `spec.nodeTaints` for the `NodeTaint` field schema.");

    scheduled_capacity();
}

/// Emit the `ScheduledCapacity` API reference (ADR 0011).
///
/// Kept in its own function because it documents a different kind with a
/// different contract: `ScheduledMachine` hands a whole machine over, this
/// scales one field on an object 5-Spot does not own.
#[allow(clippy::too_many_lines)] // Linear prose emission, same shape as main().
fn scheduled_capacity() {
    println!();
    println!("---");
    println!();
    println!("## ScheduledCapacity");
    println!();
    println!("The `ScheduledCapacity` custom resource gates **one numeric capacity field on");
    println!("an object 5-Spot does not own**, from the same spot-schedule providers a");
    println!("`ScheduledMachine` uses (ADR 0011).");
    println!();
    println!("It is the opposite of `ScheduledMachine`. A `ScheduledMachine` is *handover*:");
    println!("the whole physical machine leaves the cluster when the schedule closes. A");
    println!("`ScheduledCapacity` keeps the machine in the cluster the whole time and");
    println!("concedes a bounded slice of it while the schedule is open. On a 32-core host");
    println!("whose incumbent only ever uses 16, that is how the spare capacity gets used");
    println!("without the incumbent ever losing the node.");
    println!();
    println!("**It never creates or deletes the object it governs.** Writing a capacity");
    println!("field is reversible and bounded; deleting a consumer's pool with live claims");
    println!("would destroy in-flight work using knowledge this controller does not have.");
    println!();
    println!("Reconciled by the separate `5spot-capacity-controller` binary under its own");
    println!("ServiceAccount, so compromising it cannot delete a CAPI `Machine` and");
    println!("compromising the machine controller cannot write capacity. A cluster with no");
    println!("capacity consumer installs none of it.");
    println!();
    println!("### API Group and Version");
    println!();
    println!("- **Group**: `5spot.finos.org`");
    println!("- **Version**: `v1alpha1`");
    println!("- **Kind**: `ScheduledCapacity`");
    println!("- **Short name**: `scap`");
    println!("- **Scope**: Namespaced");
    println!();
    println!("> `v1alpha1` while the consumer contract settles; `ScheduledMachine` is at");
    println!("> `v1beta1`. Field evolution follows ADR 0007's additive-only rule.");
    println!();
    println!("### Example");
    println!();
    println!("```yaml");
    println!("apiVersion: 5spot.finos.org/v1alpha1");
    println!("kind: ScheduledCapacity");
    println!("metadata:");
    println!("  name: agent-sandbox-capacity");
    println!("  namespace: sandboxes");
    println!("spec:");
    println!("  schedule:");
    println!("    apiVersion: spotschedules.5spot.finos.org/v1alpha1");
    println!("    kind: CapitalMarketsSchedule");
    println!("    name: nyse-trading-day");
    println!("  target:");
    println!("    apiVersion: banlieue.io/v1alpha1");
    println!("    kind: VirtualMachinePool");
    println!("  capacity:");
    println!("    field: warmReplicas");
    println!("    activeValue: 10");
    println!("  template:");
    println!("    maxReplicas: 20");
    println!("    readiness: GuestReady");
    println!("    template:");
    println!("      classRef:");
    println!("        name: small");
    println!("  handback:");
    println!("    drainedPath: status.claimed");
    println!("    timeout: 10m");
    println!("  nodeName: worker-3");
    println!("```");
    println!();
    println!("### Spec Fields");
    println!();
    println!("#### schedule");
    println!();
    println!("(required, object) Reference to the spot-schedule provider that owns the");
    println!("active/inactive decision. The same shape as");
    println!("`ScheduledMachine.spec.schedule`, resolved by the same code, so the");
    println!("`Unresolved`-is-not-`Inactive` rule cannot drift between the two kinds.");
    println!();
    println!("An unresolved provider **holds the last written value** and reports why; it is");
    println!("never read as inactive.");
    println!();
    println!("#### enabled");
    println!();
    println!("(optional, boolean, default: `true`) Administrative master switch. `false`");
    println!("holds the object `Disabled` and drives capacity to zero regardless of the");
    println!("provider.");
    println!();
    println!("#### killSwitch");
    println!();
    println!("(optional, boolean, default: `false`) Immediate, terminal handback. Takes");
    println!("precedence over `enabled` **and** the provider verdict, and is the one path");
    println!("that does not wait for a drain, because the operator has explicitly asked for");
    println!("the slice back now. It is still only a field write; nothing is deleted.");
    println!();
    println!("#### target");
    println!();
    println!("(required, object, **immutable**) The kind of object 5-Spot creates and owns");
    println!("to carry this capacity.");
    println!();
    println!("- **apiVersion** (required, string): `group/version` of the owned object. The");
    println!("  group must be in the controller's allowlist, checked at reconcile time so the");
    println!("  permitted set lives in one place and cannot be widened by a stale CRD");
    println!("- **kind** (required, string): Kind of the owned resource");
    println!();
    println!("There is deliberately **no `name`**. The owned object takes this object's own");
    println!("name, in this object's own namespace, so the watch, the write and the ownership");
    println!("check cannot aim at different objects and no edit can orphan a previously owned");
    println!("one. The object actually created is reported on `status.targetRef`.");
    println!();
    println!("The whole field is immutable for the same reason: changing the kind would leave");
    println!("the object of the old kind behind, still owned but with nothing reconciling it.");
    println!();
    println!("5-Spot sets a blocking controller `ownerReference` on the owned object, which is");
    println!("the entire removal mechanism: the controller holds **no `delete` verb**, and");
    println!("deleting this `ScheduledCapacity` garbage-collects the object instead.");
    println!();
    println!("#### capacity");
    println!();
    println!("(required, object) The single numeric field to scale and the value to write");
    println!("while the schedule is active.");
    println!();
    println!("- **field** (required, string): name of the numeric field **relative to the");
    println!("  owned object's `spec`**: `warmReplicas`, not `spec.warmReplicas`. Dots are");
    println!("  allowed for a nested knob (`scale.warm`)");
    println!("- **activeValue** (required, integer, 1..=1000000): value written while active");
    println!();
    println!("Rooting the path at `spec` rather than validating its prefix is why `field` is");
    println!("**not** a security control, where ADR 0011's `capacity.path` was one. Whatever");
    println!("it names is nested under the `spec` of an object 5-Spot built before anything is");
    println!("sent, so `metadata.` and `status.` are not rejected, they are unexpressible:");
    println!("there is no reachable `ownerReferences`, `finalizers` or label, and no JSON");
    println!("Pointer escape to attempt. What the schema still enforces is hygiene:");
    println!();
    println!("- camelCase segments only, at most 7 (one below the 8 a constructed path allows,");
    println!("  because `spec` occupies the first position)");
    println!("- `spec`, `metadata` and `status` in first position are refused as **mistakes**:");
    println!("  `spec.warmReplicas` here would construct `spec.spec.warmReplicas`");
    println!("- array indices, wildcards, `..`, quotes and `/` are inexpressible, an allowlist");
    println!("  rather than a denylist of characters someone thought of");
    println!();
    println!("`spec.template` must not set this field itself: one source of truth, or the");
    println!("template and the schedule fight on every reconcile.");
    println!();
    println!("The **inactive** value is fixed at `0` and is deliberately not configurable: a");
    println!("schedule that hands nothing back is not a schedule. Window close writes that");
    println!("zero and leaves the object standing; it is never a delete.");
    println!();
    println!("#### template");
    println!();
    println!("(required, object) Forwarded **verbatim** as the owned object's `spec`, with");
    println!("`capacity.field` injected into it.");
    println!();
    println!("Opaque by design, the same pass-through `ScheduledMachine.spec.bootstrapSpec`");
    println!("uses. 5-Spot does not and will not model a consumer's schema: the reference");
    println!("consumer's pool needs `maxReplicas`, `readiness` and a nested VM template, none");
    println!("of which is 5-Spot's business. A typed schema here would silently **prune**");
    println!("fields the consumer's CRD grew since 5-Spot last shipped, turning a correct");
    println!("template into an incorrect object with no error anywhere.");
    println!();
    println!("**Security.** Whatever the consumer's CRD admits can be written here, and");
    println!("5-Spot's ServiceAccount creates it. That carries the same residual risk as an");
    println!("embedded bootstrap spec, bounded the same two ways: the API-group allowlist, and");
    println!("the consumer's own admission policies on whatever the owned object goes on to");
    println!("create. Restrict `create` on `scheduledcapacities` accordingly.");
    println!();
    println!("Writes are **server-side applies** under field manager");
    println!("`5spot-capacity-controller`, applying the complete spec 5-Spot owns. That brings");
    println!("pruning: a field removed from `template` is removed from the owned object.");
    println!();
    println!("#### handback");
    println!();
    println!("(optional, object) The cooperative, bounded handback.");
    println!();
    println!("- **drainedPath** (optional, string): a numeric field on the owned object's");
    println!("  **status** reporting how much of the slice is still in use, e.g.");
    println!("  `status.claimed`. Must start with `status.`, because it reads the consumer's");
    println!("  own report: reading back the `spec` value 5-Spot just wrote would complete a");
    println!("  handback instantly and falsely");
    println!("- **timeout** (optional, string, default: `10m`): how long to wait for");
    println!("  `drainedPath` to reach zero");
    println!();
    println!("On deactivation the controller writes zero **first**, then waits. Zero is what");
    println!("makes the drain converge: the gated field is a warm-pool target, so zero stops");
    println!("the consumer replenishing idle capacity while work already claimed finishes on");
    println!("its own. Holding the field above zero until the consumer reported drained would");
    println!("be circular.");
    println!();
    println!("When `timeout` expires with the consumer still not drained, the controller");
    println!("**holds and reports loudly** (phase `HandbackTimedOut`, a `False`");
    println!("`HandbackComplete` condition, a metric). It never forces, deletes or");
    println!("escalates: a missed handover is visible and recoverable, while killing an");
    println!("agent mid-task is neither.");
    println!();
    println!("Omit the whole block for a consumer that exposes no in-use counter; handback");
    println!("then completes as soon as the zero write lands.");
    println!();
    println!("#### nodeName");
    println!();
    println!("(optional, string) The Kubernetes Node this object believes it governs.");
    println!();
    println!("A host is governed by `ScheduledMachine` **or** by `ScheduledCapacity`, never");
    println!("both: one removes the node, the other keeps it and shares it, and both at once");
    println!("half-works. Setting this lets the controller detect the contradiction, by");
    println!("comparing it against every `ScheduledMachine.status.nodeRef.name` in the");
    println!("namespace; on a match it sets `HostGovernanceConflict` and drives capacity to");
    println!("**zero** (ADR 0014). The active value is never written to a conflicted object,");
    println!("so a conflicted host can never be carrying conceded capacity, whichever order");
    println!("the conflict and the write arrived in. Without `nodeName` the two objects");
    println!("cannot be correlated and the invariant is");
    println!("documentation only.");
    println!();
    println!("### Status Fields");
    println!();
    println!("#### phase");
    println!();
    println!("(optional, string) One of `Pending`, `Active`, `HandingBack`,");
    println!("`HandbackTimedOut`, `Inactive`, `Disabled`, `Terminated`, `Error`.");
    println!();
    println!("Its own phase set, not `ScheduledMachine`'s: a budget ramp and a drain wait are");
    println!("not node membership. `HandingBack` and `HandbackTimedOut` exist nowhere else.");
    println!();
    println!("#### writtenValue");
    println!();
    println!("(optional, integer) The value this controller last successfully wrote. A");
    println!("written `0` is meaningful state (handback in progress or complete), not an");
    println!("absent one.");
    println!();
    println!("#### observedDrainedValue");
    println!();
    println!("(optional, integer) Last value read from `spec.handback.drainedPath`. Absent");
    println!("and `0` are different answers: an unread counter is not a drained one, and");
    println!("only `0` completes a handback.");
    println!();
    println!("#### handbackDeadline");
    println!();
    println!("(optional, string) RFC3339 instant at which the current handback wait expires.");
    println!("Cleared once handback completes.");
    println!();
    println!("#### governedNode");
    println!();
    println!("(optional, string) The Node this object believes it governs, echoed from");
    println!("`spec.nodeName`. Reported even when the overlap check cannot run, so an");
    println!("operator can correlate by hand.");
    println!();
    println!("#### conditions");
    println!();
    println!("(optional, array) `Ready`, `TargetResolved`, `CapacityWritten`,");
    println!("`HandbackComplete`, `HostGovernanceConflict`, plus `SpotScheduleResolved`");
    println!("reused unchanged from ADR 0006 so provider resolution reads the same on both");
    println!("kinds.");
}
