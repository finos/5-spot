# Testing

Testing strategies and commands for 5-Spot.

## Test Structure

```
src/
├── crd.rs              # CRD definitions
├── crd_tests.rs        # CRD unit tests
├── reconcilers/
│   ├── scheduled_machine.rs
│   └── scheduled_machine_tests.rs  # Reconciler tests
└── lib.rs
```

## Running Tests

### All Tests

```bash
cargo test
```

### Specific Test Module

```bash
cargo test crd_tests
cargo test scheduled_machine_tests
```

### Single Test

```bash
cargo test test_schedule_evaluation
```

### With Output

```bash
cargo test -- --nocapture
```

### Ignored Tests

```bash
# Run only ignored tests
cargo test -- --ignored

# Run all tests including ignored
cargo test -- --include-ignored
```

## Test Categories

### Unit Tests

Located in `*_tests.rs` files alongside source code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_parsing() {
        let schedule = Schedule {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "UTC".to_string(),
            enabled: true,
        };
        
        assert!(schedule.is_valid());
    }
}
```

### Integration Tests

Located in `tests/` directory:

```rust
// tests/integration_test.rs
use five_spot::*;

#[tokio::test]
async fn test_reconciliation_loop() {
    // Setup mock Kubernetes client
    // Create ScheduledMachine
    // Verify reconciliation behavior
}
```

### Documentation Tests

Embedded in doc comments:

```rust
/// Evaluates if the current time matches the schedule.
///
/// # Example
///
/// ```
/// use five_spot::Schedule;
///
/// let schedule = Schedule::default();
/// assert!(schedule.matches_now());
/// ```
pub fn matches_now(&self) -> bool {
    // ...
}
```

## Test Utilities

### Mock Kubernetes Client

```rust
use kube::Client;
use kube_runtime::controller::Action;

async fn create_test_client() -> Client {
    // Return mock or real client based on environment
}
```

### Test Fixtures

```rust
fn sample_scheduled_machine() -> ScheduledMachine {
    ScheduledMachine {
        metadata: ObjectMeta {
            name: Some("test-machine".to_string()),
            namespace: Some("default".to_string()),
            ..Default::default()
        },
        spec: ScheduledMachineSpec {
            schedule: Schedule {
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-17".to_string()],
                timezone: "UTC".to_string(),
                enabled: true,
            },
            // ... other fields
        },
        status: None,
    }
}
```

## Coverage

### Generate Coverage Report

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate HTML report
cargo tarpaulin --out Html
```

### Coverage Goals

| Category | Target |
|----------|--------|
| Unit Tests | 80% |
| Integration Tests | 60% |
| Overall | 70% |

## Continuous Integration

### GitHub Actions

Every workflow only installs tools and calls Makefile targets, so CI runs what a
contributor runs locally.

| Workflow | What it runs |
| --- | --- |
| `build.yaml` | formatting, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, release builds for x86_64 and ARM64, images, SBOMs and signing |
| `admission-test.yaml` | `make kind-verify-admission`: proves the admission policies actually deny, on a bare kind cluster |
| `e2e.yaml` | the k0smotron end-to-end tier below, plus `make validate-examples` |

## End-to-End Tests

Each tier proves something the one below cannot: unit tests prove decisions as
pure functions, integration tests prove reconciler flows against a mocked API,
and only a real Cluster API proves the contract between them. Two defects
shipped because a mock accepted what Cluster API rejects (ADR 0017, bug-007),
and the first run of the tier below found a third (bug-008).

### The k0smotron tier (ADR 0019)

A kind management cluster with cert-manager, Cluster API and k0smotron at pinned
versions; 5-Spot deployed from `deploy/`; a child cluster with a hosted
`K0smotronControlPlane`; and a worker container running systemd and sshd,
provisioned over SSH as a `RemoteMachine` exactly as a physical host would be.

```bash
make e2e-k0smotron-up      # builds and loads the image from your checkout
make validate-examples     # every example and docs manifest against real schemas
make e2e-k0smotron         # the lifecycle, agent and capacity suites
make e2e-k0smotron-logs    # diagnostics, if something failed
make e2e-k0smotron-down
```

`make e2e-tools` installs the pinned kind and clusterctl into `.e2e-bin/`,
verifying each download against its recorded SHA-256. These pins are separate
from the other `kind-*` targets because Cluster API v1.14 needs a management
cluster of Kubernetes v1.33 or later.

On a host that does not build images, test a published image instead:

```bash
make e2e-k0smotron-up E2E_IMAGE=ghcr.io/finos/5-spot@sha256:<digest>
```

The worker needs a **rootful** container engine. k0s refuses to start a worker
when it cannot load its eBPF device filter or find the `cpuset` cgroup
controller, and a rootless engine provides neither. GitHub's runners use
rootful Docker; locally, run the targets as root with `CONTAINER_TOOL=podman`,
or use Docker.

### What the suite proves

`.github/scripts/k0smotron-e2e.bats`, in order:

1. **The schedule opens** and 5-Spot writes the `Machine` at `v1beta2`, read
   from the Machine's `managedFields` (reading the object back cannot prove it,
   because Cluster API's conversion serves any stored Machine at `v1beta2`).
2. The Machine's references use the `v1beta2` `apiGroup` shape (ADR 0017).
3. The Machine carries the controller-owned `5spot.finos.org/scheduled-machine`
   label that the secondary watches select on (bug-007).
4. **The worker joins** the child cluster and the `ScheduledMachine` goes
   `Active`, with `status.nodeRef` naming the Node.
5. **Closing the window** removes the Node, the Machine, and the bootstrap and
   infrastructure objects, and the phase becomes `Inactive`.
6. **Reopening it** brings the same host back as a Ready Node.
7. **Deleting the `ScheduledMachine`** removes everything it created.

Then the components that share the image (ADR 0020), each from its shipped
manifest:

8. **Reclaim agent:** a `ScheduledMachine` with `killIfCommands` gets its Node
   labelled, and the agent DaemonSet (installed on the child cluster, image
   imported into the worker's containerd) runs on it.
9. A process matching `killIfCommands`, started on the worker, triggers an
   **emergency removal**: the Machine and Node go, and `spec.enabled` is set to
   `false` so the machine does not come straight back (ADR 0021's poll
   detector, no added capability).
10. **Kata-config agent:** with `spec.kata` set, the drop-in appears at
    `/etc/k0s/containerd.d/kata.toml` on the worker, the agent records the
    applied hash on its Node, and the worker's `k0sworker` service gets a new
    invocation. The image has no `nsenter` or `systemctl`, so that restart can
    only have come from `5spot nsenter` entering the worker's PID 1 namespaces.
11. **Capacity controller:** `.github/scripts/capacity-e2e.bats` runs against
    the same cluster: the owned object is created, scaled, held at zero when the
    window closes, re-scaled on the same object, garbage-collected on delete,
    and the identity can do exactly what ADR 0011 and 0016 allow.

The suite fails rather than skips when a prerequisite is missing: a test that
skips reports success while proving nothing.

### Example and docs validation

`make validate-examples` runs `.github/scripts/validate-examples.py` against the
same cluster. Every document under `examples/` and every complete object in a
fenced YAML block under `docs/src/` goes through
`kubectl apply --dry-run=server --validate=strict`, so an unknown field is an
error and the Cluster API and k0smotron webhooks run. Each embedded
`bootstrapSpec` and `infrastructureSpec` is also validated as the standalone
provider object 5-Spot would create, which is the only check that catches a
field a provider renamed between API versions.

Two markers, written as YAML comments, control it:

- `validate-examples: expect-reject`: the object **must** be rejected (for
  example `examples/scheduledmachine-bad-taint.yaml`). Acceptance fails the run.
- `validate-examples: fragment (reason)`: a docs block that deliberately shows
  only part of an object. It is listed with its reason, never passed silently.

Anything else that cannot be validated on this cluster (a provider that is not
installed, a kind config file) is listed under `NOT VALIDATED` with a reason in
the script.

### Not covered

The cgroup confinement in
[Confining a Worker with cgroups](../guides/confining-a-worker-with-cgroups.md)
needs a real kernel and host cgroup tree. A container worker cannot prove a CPU
or memory ceiling, so that recipe is verified by hand on real hardware.

## Related

- [Development Setup](./setup.md) - Environment setup
- [Building](./building.md) - Build instructions
- [Contributing](./contributing.md) - Contribution guidelines
