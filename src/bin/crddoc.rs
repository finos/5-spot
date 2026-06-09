// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # CRD API documentation generator
//!
//! Offline tool that emits a Markdown API reference for the `ScheduledMachine`
//! custom resource to `stdout`.  The output is committed to
//! `docs/reference/api.md` so that documentation consumers do not need a
//! running Rust toolchain.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --bin crddoc > docs/reference/api.md
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
    println!("- **API Version**: `v1alpha1`");
    println!("- **Kind**: `ScheduledMachine`");
    println!();
    println!("### Example");
    println!();
    println!("```yaml");
    println!("apiVersion: 5spot.finos.org/v1alpha1");
    println!("kind: ScheduledMachine");
    println!("metadata:");
    println!("  name: example-spot-machine");
    println!("  namespace: default");
    println!("spec:");
    println!("  clusterName: my-cluster");
    println!("  schedule:");
    println!("    daysOfWeek:");
    println!("      - mon-fri");
    println!("    hoursOfDay:");
    println!("      - 9-17");
    println!("    timezone: America/New_York");
    println!("    enabled: true");
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
    println!("  kataConfigRef:");
    println!("    kind: ConfigMap");
    println!("    name: kata-drop-in");
    println!("```");
    println!();
    println!("### Spec Fields");
    println!();
    println!("#### schedule");
    println!();
    println!("Machine scheduling configuration.");
    println!();
    println!("- **daysOfWeek** (required, array of strings): Days when machine should be active.");
    println!("  Supports ranges (`mon-fri`) and combinations (`mon-wed,fri-sun`).");
    println!();
    println!("- **hoursOfDay** (required, array of strings): Hours when machine should be active (0-23).");
    println!("  Supports ranges (`9-17`) and combinations (`0-9,18-23`).");
    println!();
    println!("- **timezone** (optional, string, default: `UTC`): Timezone for the schedule.");
    println!("  Must be a valid IANA timezone (e.g., `America/New_York`, `Europe/London`).");
    println!();
    println!(
        "- **enabled** (optional, boolean, default: `true`): Whether the schedule is enabled."
    );
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
    println!("(optional, integer 0-100, default: `50`) Priority for machine scheduling.");
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
    println!("object from the workload API, writes the drop-in to `destPath`, and restarts");
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
    println!("- **destPath** (optional, string, default:");
    println!("  `/etc/k0s/container.d/kata-containers.toml`): absolute host path written to.");
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
}
