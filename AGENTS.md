# AGENTS.md - AI Coding Agent Instructions

> 5-Spot Machine Scheduler - Kubernetes Operator for Time-Based Machine Scheduling
> Language: Rust (Edition 2021) | Framework: kube-rs | Runtime: tokio

## Build / Lint / Test Commands

```bash
# Build
cargo build --release          # Release build
cargo build                    # Debug build

# Test
cargo test --all               # Run ALL tests
cargo test --lib               # Library tests only
cargo test <test_name>         # Run single test by name
cargo test <module>::          # Run tests in a module (e.g., cargo test crd::)
cargo test -- --nocapture      # Show println/tracing output

# Lint & Format
cargo fmt                      # Format code
cargo fmt -- --check           # Check formatting (CI)
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions

# CRD Generation (after modifying src/crd.rs)
make crds                      # Generate deploy/crds/scheduledmachine.yaml
make crddoc                    # Generate docs/src/reference/api.md

# Run locally
RUST_LOG=debug cargo run       # Run controller with debug logging

# Documentation (requires Poetry)
make docs-serve                # Serve docs locally with live reload
make docs                      # Build all documentation

# Corporate network: set PYPI_INDEX_URL for custom PyPI mirror
export PYPI_INDEX_URL=https://artifactory.example.com/api/pypi/pypi/simple
```

## Code Style Guidelines

### Imports

Order imports in this sequence, separated by blank lines:
1. Standard library (`std::`)
2. External crates (`kube::`, `tokio::`, `serde::`, etc.)
3. Internal modules (`crate::`, `super::`)

```rust
use std::sync::Arc;

use kube::{Api, Client};
use tokio::time::Duration;

use crate::constants::DEFAULT_REQUEUE_SECS;
```

### Formatting

- Use `cargo fmt` (default rustfmt settings)
- Max line width: 100 characters (rustfmt default)
- Use 4-space indentation (Rust standard)

### Types & Error Handling

- Use `thiserror` for library error types, NOT string errors
- Use `anyhow::Result` in binaries, typed errors in libraries
- NEVER use `unwrap()` in production code - use `?` or explicit error handling
- All k8s API calls MUST have timeout and retry logic

```rust
// GOOD: Typed errors with thiserror
#[derive(Debug, thiserror::Error)]
pub enum ReconcilerError {
    #[error("Failed to get machine: {0}")]
    GetMachine(#[source] kube::Error),
}

// BAD: String errors
Err("something went wrong".to_string())
```

### Naming Conventions

| Item | Convention | Example |
|------|------------|---------|
| Types/Structs | PascalCase | `ScheduledMachine` |
| Functions | snake_case | `reconcile_machine` |
| Constants | SCREAMING_SNAKE_CASE | `DEFAULT_REQUEUE_SECS` |
| Modules | snake_case | `scheduled_machine` |
| Test functions | `test_<description>` | `test_parse_day_range` |

### Constants - No Magic Numbers

Any numeric literal other than `0` or `1` MUST be a named constant in `src/constants.rs`:

```rust
// GOOD
const DEFAULT_REQUEUE_SECS: u64 = 300;
Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS))

// BAD
Action::requeue(Duration::from_secs(300))  // Why 300?
```

### Early Returns / Guard Clauses

Minimize `else` statements. Return early for edge cases:

```rust
// GOOD
pub fn process(input: Option<Value>) -> Result<()> {
    let value = match input {
        Some(v) => v,
        None => return Ok(()),  // Early return
    };
    // Main logic here
    Ok(())
}

// BAD - Nested if-else
pub fn process(input: Option<Value>) -> Result<()> {
    if let Some(value) = input {
        // Main logic
        Ok(())
    } else {
        Ok(())
    }
}
```

### Logging

Use `tracing` for logging, NOT `println!` or `log`:

```rust
use tracing::{debug, info, warn, error};

info!(resource = %name, namespace = %ns, "Reconciling resource");
error!(error = ?e, "Failed to reconcile");
```

### Documentation

All public functions and types MUST have rustdoc comments:

```rust
/// Reconciles the ScheduledMachine custom resource.
///
/// # Arguments
/// * `resource` - The ScheduledMachine CR to reconcile
///
/// # Errors
/// Returns `ReconcilerError` if reconciliation fails.
pub async fn reconcile(resource: Arc<ScheduledMachine>) -> Result<Action, ReconcilerError>
```

## Testing

### Test File Organization

Tests go in SEPARATE `_tests.rs` files, NOT embedded in source files:

```
src/foo.rs           # Source file - ends with: #[cfg(test)] mod foo_tests;
src/foo_tests.rs     # Test file
```

Test file structure:
```rust
#[cfg(test)]
mod tests {
    use super::super::*;  // Import parent module

    #[test]
    fn test_example() {
        // Arrange
        let input = "test";
        // Act
        let result = process(input);
        // Assert
        assert!(result.is_ok());
    }
}
```

### Test-Driven Development (TDD)

This project follows strict TDD. Write tests FIRST:
1. RED: Write a failing test
2. GREEN: Write minimal code to pass
3. REFACTOR: Improve while keeping tests green

## Project Structure

```
src/
├── main.rs                    # Entry point, CLI, health/metrics servers
├── main_tests.rs              # Tests for main.rs
├── lib.rs                     # Library exports
├── crd.rs                     # CRD definitions (SOURCE OF TRUTH)
├── crd_tests.rs               # CRD tests
├── constants.rs               # Global constants
├── labels.rs                  # Kubernetes labels
├── reconcilers/
│   ├── mod.rs
│   ├── scheduled_machine.rs
│   ├── scheduled_machine_tests.rs
│   ├── helpers.rs
│   └── helpers_tests.rs
└── bin/
    ├── crdgen.rs              # CRD YAML generator
    └── crddoc.rs              # API doc generator

deploy/crds/                   # AUTO-GENERATED - do not edit directly
docs/roadmaps/                 # All roadmaps and plans go here
```

## Kubernetes Operator Patterns

### CRD Development

Rust types in `src/crd.rs` are the SOURCE OF TRUTH. CRD YAML is auto-generated.
After modifying `src/crd.rs`, run: `make crds && make crddoc`

### Event-Driven (Watch, Not Poll)

Use Kubernetes watch API, NEVER polling loops:

```rust
// CORRECT
Controller::new(api, Config::default())
    .run(reconcile, error_policy, context)
    .await;

// WRONG - Polling
loop {
    let resources = api.list(&ListParams::default()).await?;
    tokio::time::sleep(Duration::from_secs(30)).await;
}
```

### Controller Best Practices

- Set `ownerReferences` for child resources
- Use finalizers for cleanup logic
- Implement exponential backoff for retries
- Log reconciliation start/end with resource name and namespace

## Things to Avoid

- `unwrap()` in production code
- Hardcoded namespaces (make configurable)
- `sleep()` for synchronization (use k8s watch/informers)
- Storing state outside Kubernetes (controllers must be stateless)
- `docker build/push` or `kubectl apply` (unless explicitly requested)
- Secrets, tokens, or internal hostnames in code

## Pre-Commit Checklist

Before completing any task:
1. `cargo fmt` - Format code
2. `cargo clippy --all-targets --all-features -- -D warnings` - Fix all warnings
3. `cargo test --all` - All tests pass
4. If CRD changed: `make crds && make crddoc`
5. Update `.claude/CHANGELOG.md` with author attribution
