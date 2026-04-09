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
    println!("- **API Group**: `capi.5spot.io`");
    println!("- **API Version**: `v1alpha1`");
    println!("- **Kind**: `ScheduledMachine`");
    println!();
    println!("### Example");
    println!();
    println!("```yaml");
    println!("apiVersion: capi.5spot.io/v1alpha1");
    println!("kind: ScheduledMachine");
    println!("metadata:");
    println!("  name: example-machine");
    println!("  namespace: default");
    println!("spec:");
    println!("  schedule:");
    println!("    daysOfWeek:");
    println!("      - mon-fri");
    println!("    hoursOfDay:");
    println!("      - 9-17");
    println!("    timezone: America/New_York");
    println!("    enabled: true");
    println!("  machine:");
    println!("    address: 192.168.1.100");
    println!("    user: admin");
    println!("    port: 22");
    println!("    useSudo: false");
    println!("    files: []");
    println!("  bootstrapRef:");
    println!("    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1");
    println!("    kind: KubeadmConfigTemplate");
    println!("    name: worker-bootstrap-config");
    println!("    namespace: default");
    println!("  infrastructureRef:");
    println!("    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1");
    println!("    kind: MachineTemplate");
    println!("    name: worker-machine-template");
    println!("    namespace: default");
    println!("  clusterName: my-cluster");
    println!("  priority: 50");
    println!("  gracefulShutdownTimeout: 5m");
    println!("  killSwitch: false");
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
    println!("#### machine");
    println!();
    println!("Machine specification for k0smotron.");
    println!();
    println!("- **address** (required, string): IP address of the machine.");
    println!();
    println!("- **user** (required, string): Username for SSH connection.");
    println!();
    println!("- **port** (optional, integer, default: `22`): SSH port.");
    println!();
    println!(
        "- **useSudo** (optional, boolean, default: `false`): Whether to use sudo for commands."
    );
    println!();
    println!("- **files** (optional, array): Files to be passed to user_data upon creation.");
    println!();
    println!("#### clusterName");
    println!();
    println!("(required, string) Name of the CAPI cluster this machine belongs to.");
    println!("The bootstrap and infrastructure refs must be configured for this cluster.");
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
    println!("#### killSwitch");
    println!();
    println!("(optional, boolean, default: `false`) When true, immediately removes the machine");
    println!("from the cluster and takes it out of rotation, bypassing the grace period.");
    println!();
    println!("### Status Fields");
    println!();
    println!("#### phase");
    println!();
    println!("Current phase of the machine lifecycle. Possible values:");
    println!();
    println!("- **Pending**: Initial state, schedule not yet evaluated");
    println!("- **Scheduled**: Machine is within scheduled time window but not yet active");
    println!("- **Active**: Machine is running and part of the cluster");
    println!("- **UnScheduled**: Machine is outside scheduled time window");
    println!("- **Removing**: Machine is being removed from cluster");
    println!("- **Inactive**: Machine has been removed and is inactive");
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
    println!("#### machineRef");
    println!();
    println!("Reference to the actual Machine resource:");
    println!();
    println!("- **name**: Machine name");
    println!("- **namespace**: Machine namespace");
    println!("- **uid**: Machine UID");
    println!();
    println!("#### lastScheduleTime");
    println!();
    println!("Last time the machine was scheduled and activated.");
    println!();
    println!("#### nextScheduleTime");
    println!();
    println!("Next time the machine will be scheduled (if calculable).");
    println!();
    println!("#### observedGeneration");
    println!();
    println!("The generation observed by the controller. Used for change detection.");
}
