// CRD generator - outputs CRD YAML from Rust types
// Run with: cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml

use five_spot::crd::ScheduledMachine;
use kube::CustomResourceExt;

fn main() {
    // Generate CRD YAML
    let crd = ScheduledMachine::crd();

    // Output as YAML
    println!("{}", serde_yaml::to_string(&crd).unwrap());
}
