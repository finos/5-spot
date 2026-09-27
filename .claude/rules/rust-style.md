# Rust Style Guide

## Core Principles

- Use `thiserror` for error types, not string errors
- Prefer `anyhow::Result` in binaries, typed errors in libraries
- Use `tracing` for logging, not `println!` or `log`
- Async functions use `tokio`
- All k8s API calls must have timeout and retry logic
- **No magic numbers**: any numeric literal other than `0` or `1` MUST be a named constant
- **Early returns / guard clauses**: minimize nesting; handle edge cases first

---

## Early Return / Guard Clause Pattern

**CRITICAL: Prefer early returns over nested if-else statements.**

1. **Handle preconditions first** — validate at the top and return
   immediately (`return Err(...)`, `return Ok(())`, `return None`):

   ```rust
   // ✅ GOOD - Early return for validation
   pub async fn reconcile(resource: Arc<ScheduledMachine>, ctx: Arc<Context>) -> Result<Action> {
       // Guard clause: nothing to do
       if !needs_reconciliation {
           debug!("Spec unchanged, skipping reconciliation");
           return Ok(Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS)));
       }

       // Main logic continues here (happy path)
       perform_reconciliation(&resource, &ctx).await?;
       Ok(Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS)))
   }

   // ❌ BAD - Nested if-else
   pub async fn reconcile(resource: Arc<ScheduledMachine>, ctx: Arc<Context>) -> Result<Action> {
       if needs_reconciliation {
           perform_reconciliation(&resource, &ctx).await?;
           Ok(Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS)))
       } else {
           debug!("Spec unchanged, skipping reconciliation");
           Ok(Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS)))
       }
   }
   ```

2. **Minimize `else` blocks** — mutually exclusive conditions become early
   returns inside `if` blocks, with the general case unindented below.

3. **Use `?` liberally** — it is early return for errors and keeps the happy
   path unindented.

### Benefits

Reduced nesting, clearer flow, fail-fast on invalid state, and edge-case
changes that don't disturb the main logic.

### When to use

Input validation, precondition checks before expensive operations, special
cases before the general case, and state validation at the top of
reconciliation loops (deletion timestamp, kill switch, `spec.enabled`).

---

## Magic Numbers Rule

**CRITICAL: All numeric literals (except 0 and 1) MUST be named constants.**

- **`0` and `1` are allowed** — ubiquitous and self-explanatory.
- **Everything else is a named constant** — no exceptions. The name explains
  *why* the value matters, not just what it is.

```rust
// ✅ GOOD - Named constants
const DEFAULT_REQUEUE_SECS: u64 = 300;
const ERROR_REQUEUE_SECS: u64 = 30;

fn error_policy() -> Action {
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS))
}

// ❌ BAD - Magic numbers
fn error_policy() -> Action {
    Action::requeue(Duration::from_secs(30))  // Why 30?
}
```

Special cases that still need constants: unit conversions
(`MILLISECONDS_PER_SECOND`), buffer sizes, meaningful array bounds, and —
already the convention here — **HTTP/Kubernetes status codes**: branch on the
named constants in `src/constants.rs` (built on the `http` crate), never on a
bare `404`/`409`/`429`.

**Test files exception:** `*_tests.rs` may use literal values for one-off
test data. A value that repeats or represents real configuration still uses
the shared constant.

Verification sweep before committing:

```bash
rg -n '\b[2-9][0-9]*\b' -trs src/ --glob '!*_tests.rs' | rg -v '://|const |^\s*//'
```

---

## Global Constants for Repeated Strings

When a string literal appears in multiple places, it MUST be a constant.

Create one when a string appears 2+ times in a file, appears in multiple
files, is a configuration value (paths, filenames, keys, labels), or is part
of an API contract — condition types/reasons, annotation and label keys, the
fixed kata drop-in path.

```rust
// ✅ GOOD
const KATA_CONFIG_DEST_PATH: &str = "/etc/k0s/containerd.d/kata.toml";
// ❌ BAD - the same path string pasted at three call sites
```

Where: module-level for file-local use; `src/constants.rs` (or `src/labels.rs`
for label keys) for cross-module use. Group related constants with docs.

---

## Dependency Management

Before adding a new dependency:
1. Check if existing deps solve the problem — and check roadmap 04
   (`.github/community/04-dependency-internalization-matrix.md`) for the
   internalization stance on that tier
2. Verify the crate is actively maintained
3. Prefer well-known ecosystem crates; trim default features you don't use
   (the `prometheus`-without-`protobuf` and `warp` `server`-only pins are the
   pattern)
4. Document why in `.claude/CHANGELOG.md`

---

## Code Comments

All public functions and types **must** have rustdoc:

```rust
/// Reconciles the ScheduledMachine custom resource.
///
/// # Arguments
/// * `resource` - The ScheduledMachine CR to reconcile
/// * `ctx` - Controller context with client and instance info
///
/// # Errors
/// Returns `ReconcilerError` if schedule evaluation fails, k0smotron API is
/// unreachable, or machine lifecycle operations fail.
pub async fn reconcile_scheduled_machine(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
```

Doc comments in `src/crd.rs` land in the generated CRD YAML and the API docs —
after changing them, run the `regen-crds` skill, then `regen-api-docs` (LAST).

---

## Things to Never Do

- **Never** use `unwrap()` in production code — `?` or explicit handling
- **Never** hardcode namespaces — make them configurable
- **Never** use `sleep()` for synchronization — watch/informers only
- **Never** ignore errors in finalizers — this blocks resource deletion
- **Never** store state outside of Kubernetes — controllers must be stateless
