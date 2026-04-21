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
}
