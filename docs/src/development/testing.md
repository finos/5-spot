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

```yaml
test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    
    - name: Run tests
      run: cargo test --all-features
      
    - name: Run clippy
      run: cargo clippy -- -D warnings
      
    - name: Check formatting
      run: cargo fmt -- --check
```

## End-to-End Tests

### With kind

```bash
# Create test cluster
kind create cluster --name e2e-test

# Install dependencies
kubectl apply -f deploy/crds/

# Run E2E tests
cargo test --features e2e -- --ignored
```

### Test Scenarios

1. **Happy Path**: Create → Schedule Match → Active → Schedule End → Inactive
2. **Kill Switch**: Active → Kill Switch → Immediate Removal
3. **Error Recovery**: Error → Retry → Success
4. **Multi-Instance**: Resource distribution across instances

## Related

- [Development Setup](./setup.md) - Environment setup
- [Building](./building.md) - Build instructions
- [Contributing](./contributing.md) - Contribution guidelines
