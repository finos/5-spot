# Project Instructions for Claude Code

> 5Spot Machine Scheduler - Time-Based Kubernetes Machine Scheduling
> Environment: Kubernetes / k0smotron / Physical Infrastructure Management
>
> **Project Focus**: Time-based scheduling of physical machines in k0smotron clusters. This controller manages machine lifecycle (add/remove from cluster) based on configurable time schedules with timezone support.
>
> **CRITICAL Coding Patterns**:
> - **Test-Driven Development (TDD)**: ALWAYS write tests FIRST before implementing functionality. Write failing tests that define the expected behavior, then implement code to make tests pass. This ensures all code is testable and has comprehensive test coverage from the start.
> - **Event-Driven Programming**: In Kubernetes controller development, ALWAYS use event-driven programming (e.g., "watch" on kube API) as opposed to polling. Controllers must react to cluster state changes efficiently.
> - **Early Returns**: Use as few `else` statements as possible. Return from functions as soon as you can to minimize nesting and improve code clarity (see Early Return / Guard Clause Pattern section).
> - **ALWAYS Run cargo fmt**: At the end of EVERY task or phase involving Rust code, you MUST run the `cargo-quality` skill. This is NON-NEGOTIABLE and MANDATORY.

---

## ⚙️ Claude Code Configuration

### 🚨 CRITICAL: Always Verify CRD Schema Sync

**MANDATORY REQUIREMENT:** Before investigating any Kubernetes-related issue, ALWAYS verify that deployed CRDs match the Rust code definitions.

**Why This Matters:**
- CRD YAML files in `deploy/crds/` are **AUTO-GENERATED** from Rust types in `src/crd.rs`
- If CRDs are not regenerated after code changes, schema mismatches cause silent failures
- Kubernetes API server may accept patches (HTTP 200) but ignore fields not in the CRD schema
- This leads to confusing bugs where code works but data doesn't persist

> **How:** Run the `verify-crd-sync` skill.

### 🚨 CRITICAL: Always Review Official Documentation When Unsure

**MANDATORY REQUIREMENT:** When you are unsure of a decision, DO NOT take the easiest or fastest path. ALWAYS review the official documentation for the product, tool, or framework you are working with.

### 🔍 MANDATORY SEARCH TOOL: ripgrep (rg)

**OBLIGATORY RULE:** ALWAYS use `ripgrep` (command: `rg`) as your PRIMARY and FIRST tool for ANY code search, pattern matching, or grepping task.

**Rust-Specific Flags:** For Rust projects, use `rg -trs <PATTERN>` to search only Rust files (`-trs`) recursively.

---

## 🚫 CRITICAL: Docker and Kubernetes Operations Restrictions

**NEVER build or push Docker images yourself. The user handles all Docker image operations.**

### Allowed kubectl Operations (Read-Only + Annotations):
- ✅ `kubectl get` - Read resources
- ✅ `kubectl describe` - View resource details
- ✅ `kubectl logs` - Read pod logs
- ✅ `kubectl annotate` - Add/modify annotations
- ✅ Any other read-only operations

### FORBIDDEN Operations:
- ❌ `docker build` - NEVER build Docker images
- ❌ `docker push` - NEVER push images to registries
- ❌ `kubectl rollout restart` - NEVER restart deployments/pods
- ❌ `kubectl delete pods` - NEVER delete pods to trigger restarts
- ❌ `kubectl apply` - NEVER apply manifests (unless explicitly requested)

---

## 🚨 Critical TODOs

### CRITICAL: Plans and Roadmaps Location

**Status:** ✅ MANDATORY REQUIREMENT

**ALWAYS add plans or roadmaps to `docs/roadmaps/`, NO WHERE ELSE.**

**Naming Convention:**
- **ALWAYS** use **lowercase** filenames (MANDATORY)
- **ALWAYS** use **hyphens** (`-`) to separate words, NEVER underscores (`_`)

> **How:** Run the `create-roadmap` skill.

### Code Quality: Use Global Constants for Repeated Strings

**Status:** 🔄 Ongoing
**Impact:** Code maintainability and consistency

When a string literal appears in multiple places across the codebase, it MUST be defined as a global constant and referenced consistently.

**When to Create a Global Constant:**
- String appears 2+ times in the same file
- String appears in multiple files
- String represents a configuration value (paths, filenames, keys, etc.)
- String is part of an API contract or protocol

### CRITICAL: Always Run cargo fmt and clippy After Code Changes

**Status:** ✅ Required Standard
**Impact:** Code quality, consistency, and CI/CD pipeline success

**MANDATORY REQUIREMENT:** Whenever you add or modify code in tests or source files, run the `cargo-quality` skill before considering the task complete.

> **How:** Run the `cargo-quality` skill.

### High Priority: CRD Code Generation

**Status:** ✅ Implemented
**Impact:** Automated - CRD YAMLs are generated from Rust types

The Rust types in `src/crd.rs` are the **source of truth**. CRD YAML files in `deploy/crds/` are **auto-generated** from these types.

> **How:** Run the `regen-crds` skill, then the `regen-api-docs` skill (LAST).

⚠️ **IMPORTANT**: Always run `crddoc` **LAST** after all CRD changes, example updates, and validations are complete.

---

## 🔒 Compliance & Security Context

This codebase operates in a **regulated banking environment**. All changes must be:
- Auditable with clear documentation
- Traceable to a business or technical requirement
- Compliant with zero-trust security principles

**Never commit**:
- Secrets, tokens, or credentials (even examples)
- Internal hostnames or IP addresses
- Customer or transaction data in any form
- **RBC/internal URLs, addresses, or references in ANY docs or code**

**🚨 CRITICAL: Internal References Rule**

**NEVER add RBC-specific or internal references (URLs, hostnames, artifactory paths, etc.) to any code, documentation, or configuration files.**

If you believe an internal reference is necessary, **ASK THE USER FIRST** before adding it.

---

## 📝 Documentation Requirements

### 🚨 CRITICAL: Always Verify if Documentation Needs Updates

**MANDATORY: Before considering ANY task complete, ALWAYS ask yourself: "Does documentation need to be updated?"**

> **How:** Run the `update-docs` skill.

### Mandatory: Update Changelog on Every Code Change

After **ANY** code modification, update `.claude/CHANGELOG.md` with the following format:

```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** [Author Name]

### Changed
- `path/to/file.rs`: Description of the change

### Why
Brief explanation of the business or technical reason.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only
```

> **How:** Run the `update-changelog` skill.

**CRITICAL REQUIREMENT**:
- The `**Author:**` line is **MANDATORY** for ALL changelog entries
- **NO exceptions** - every changelog entry must have an author attribution

### Code Comments

All public functions and types **must** have rustdoc comments:

```rust
/// Reconciles the ScheduledMachine custom resource.
///
/// # Arguments
/// * `resource` - The ScheduledMachine CR to reconcile
/// * `ctx` - Controller context with client and instance info
///
/// # Errors
/// Returns `ReconcilerError` if schedule evaluation fails, k0smotron API is unreachable,
/// or machine lifecycle operations fail.
pub async fn reconcile_scheduled_machine(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
```

---

## 🦀 Rust Workflow

### Developer Prerequisites

**Required Tools:**
- Rust toolchain (via rustup)
- `cross` - Cross-compilation tool for building Linux binaries on macOS

```bash
# Install cross for cross-compilation
cargo install cross --git https://github.com/cross-rs/cross
```

### CRITICAL: Test-Driven Development (TDD) Workflow

**MANDATORY: ALWAYS write tests FIRST before implementing functionality.**

This project follows strict Test-Driven Development practices. You MUST follow the Red-Green-Refactor cycle for ALL code changes.

> **How:** Follow the `tdd-workflow` skill (RED → GREEN → REFACTOR).

**TDD is MANDATORY except for:**
- Exploratory/prototype code (must be marked as such and removed before merging)
- Simple refactoring that doesn't change behavior (existing tests verify correctness)

**REMEMBER**: If you're writing implementation code before tests, STOP and write tests first!

### After Modifying Any `.rs` File

**CRITICAL: At the end of EVERY task that modifies Rust files, run the `cargo-quality` skill.**

> **How:** Run the `cargo-quality` skill. Fix ALL clippy warnings. Task is NOT complete until all three commands pass.

### Unit Testing Requirements

**CRITICAL: When modifying ANY Rust code, you MUST update, add, or delete unit tests accordingly:**

1. **Adding New Functions/Methods:** MUST add unit tests for ALL new public functions
2. **Modifying Existing Functions:** MUST update existing tests to reflect changes
3. **Deleting Functions:** MUST delete corresponding unit tests
4. **Refactoring Code:** Update test names and assertions to match refactored code

**Test File Organization:**
- **CRITICAL**: ALWAYS place tests in separate `_tests.rs` files
- NEVER embed large test modules directly in source files
- Follow the pattern: `foo.rs` → `foo_tests.rs`

### Rust Style Guidelines

- Use `thiserror` for error types, not string errors
- Prefer `anyhow::Result` in binaries, typed errors in libraries
- Use `tracing` for logging, not `println!` or `log`
- Async functions should use `tokio`
- All k8s API calls must have timeout and retry logic
- **No magic numbers**: Any numeric literal other than `0` or `1` MUST be declared as a named constant
- **Use early returns/guard clauses**: Minimize nesting by handling edge cases early and returning

#### Early Return / Guard Clause Pattern

**CRITICAL: Prefer early returns over nested if-else statements.**

```rust
// ✅ GOOD - Early return for validation
pub async fn reconcile(resource: Arc<ScheduledMachine>, ctx: Arc<Context>) -> Result<()> {
    // Guard clause: Check if work is needed
    if !needs_reconciliation {
        debug!("Spec unchanged, skipping reconciliation");
        return Ok(());  // Early return - no work needed
    }

    // Main logic continues here (happy path)
    perform_reconciliation(&resource, &ctx).await?;
    Ok(())
}

// ❌ BAD - Nested if-else
pub async fn reconcile(resource: Arc<ScheduledMachine>, ctx: Arc<Context>) -> Result<()> {
    if needs_reconciliation {
        perform_reconciliation(&resource, &ctx).await?;
        Ok(())
    } else {
        debug!("Spec unchanged, skipping reconciliation");
        Ok(())
    }
}
```

#### Magic Numbers Rule

**CRITICAL: Eliminate all magic numbers from the codebase.**

```rust
// ✅ GOOD - Named constants
const DEFAULT_REQUEUE_SECS: u64 = 300;
const ERROR_REQUEUE_SECS: u64 = 30;

fn reconcile() -> Action {
    Action::requeue(Duration::from_secs(DEFAULT_REQUEUE_SECS))
}

// ❌ BAD - Magic numbers
fn reconcile() -> Action {
    Action::requeue(Duration::from_secs(300))  // Why 300?
}
```

---

## ☸️ Kubernetes Operator Patterns

### CRD Development - Rust as Source of Truth

**CRITICAL: Rust types in `src/crd.rs` are the source of truth.**

CRD YAML files in `deploy/crds/` are **AUTO-GENERATED** from the Rust types.

> **How:** Run the `regen-crds` skill, then `regen-api-docs` skill (LAST).

**Never edit the YAML files directly** - your changes will be overwritten on next generation.

### Controller Best Practices

#### Event-Driven Programming (Watch, Not Poll)

**CRITICAL: Kubernetes controllers MUST use event-driven programming, NOT polling.**

Controllers should react to cluster state changes via the Kubernetes watch API, not poll resources on a timer.

```rust
// ✅ CORRECT - Event-Driven with Watch
use kube::runtime::Controller;

Controller::new(api, Config::default())
    .run(reconcile, error_policy, context)
    .for_each(|_| futures::future::ready(()))
    .await;

// ❌ WRONG - Polling Pattern
loop {
    let resources = api.list(&ListParams::default()).await?;
    for resource in resources {
        reconcile(resource).await?;
    }
    tokio::time::sleep(Duration::from_secs(30)).await; // Polling!
}
```

**General Controller Best Practices:**
- Always set `ownerReferences` for child resources
- Use finalizers for cleanup logic
- Implement exponential backoff for retries
- Set appropriate `requeue_after` durations
- Log reconciliation start/end with resource name and namespace

### Status Conditions

Always update status conditions following Kubernetes conventions:

```rust
Condition {
    type_: "Ready".to_string(),
    status: "True".to_string(),
    reason: "ReconcileSucceeded".to_string(),
    message: "Machine scheduled successfully".to_string(),
    last_transition_time: Some(Utc::now().to_rfc3339()),
    observed_generation: Some(machine.metadata.generation.unwrap_or(0)),
}
```

---

## 🧪 Testing Requirements

### Unit Tests

**MANDATORY: Every public function MUST have corresponding unit tests.**

#### Test File Organization

**CRITICAL: ALWAYS place unit tests in separate `_tests.rs` files, NOT embedded in the source file.**

**Correct Pattern:**
`src/foo.rs` → declare `#[cfg(test)] mod foo_tests;` at the bottom;
`src/foo_tests.rs` → `#[cfg(test)] mod tests { use super::super::*; ... }`.

> **See:** `tdd-workflow` skill for the full file pattern and Arrange-Act-Assert examples.

**Examples in This Codebase:**
- `src/main.rs` → `src/main_tests.rs`
- `src/crd.rs` → `src/crd_tests.rs`
- `src/reconcilers/scheduled_machine.rs` → `src/reconcilers/scheduled_machine_tests.rs`

### Integration Tests

Place in `/tests/` directory:
- Use `k8s-openapi` test fixtures
- Mock external services (k0smotron API, etc.)
- Test failure scenarios, not just happy path
- Test end-to-end workflows (create → update → delete)
- Verify finalizers and cleanup logic

### Test Execution

> **How:** Run the `cargo-quality` skill. For a specific module: `cargo test --lib <module_path>`. For verbose output: `cargo test -- --nocapture`.

**ALL tests MUST pass before code is considered complete.**

---

## 📁 File Organization

```
.claude/
├── CLAUDE.md                    # Project instructions (this file)
├── SKILL.md                     # Reusable procedural skills
└── CHANGELOG.md                 # Change log with author attribution

src/
├── main.rs                      # Entry point, CLI setup, health/metrics servers
├── main_tests.rs                # Tests for main.rs
├── lib.rs                       # Library exports
├── crd.rs                       # Custom Resource Definitions (ScheduledMachine)
├── crd_tests.rs                 # Tests for crd.rs (parsing, validation)
├── constants.rs                 # Global constants (timing, labels, conditions)
├── labels.rs                    # Standard Kubernetes labels
├── reconcilers/                 # Reconciliation logic
│   ├── mod.rs                   # Module exports
│   ├── scheduled_machine.rs     # ScheduledMachine reconciler (phase handlers)
│   ├── scheduled_machine_tests.rs # Tests for scheduled_machine.rs
│   ├── helpers.rs               # Helper functions (schedule eval, finalizers)
│   └── helpers_tests.rs         # Tests for helpers.rs
└── bin/
    ├── crdgen.rs                # CRD YAML generator
    └── crddoc.rs                # CRD documentation generator

docs/
├── roadmaps/                    # CRITICAL: All roadmaps and implementation planning docs MUST go here
│   └── *.md                     # Future feature plans, optimization strategies, design proposals
├── reference/                   # API documentation
└── ...
```

**Test File Pattern:**
- Every `foo.rs` has a corresponding `foo_tests.rs`
- Test files are in the same directory as the source file
- Source file declares: `#[cfg(test)] mod foo_tests;`
- Test file contains: `#[cfg(test)] mod tests { ... }`

---

## 🚫 Things to Avoid

- **Never** use `unwrap()` in production code - use `?` or explicit error handling
- **Never** hardcode namespaces - make them configurable
- **Never** use `sleep()` for synchronization - use proper k8s watch/informers
- **Never** ignore errors in finalizers - this blocks resource deletion
- **Never** store state outside of Kubernetes - controllers must be stateless

---

## 💡 Helpful Commands

See `.claude/SKILL.md` for full step-by-step procedures. Quick one-liners:

```bash
# Run controller locally against current kubeconfig
RUST_LOG=debug cargo run

# Validate all manifests
kubectl apply --dry-run=server -f config/
```

Skills for common operations: `regen-crds`, `regen-api-docs`, `validate-examples`, `cargo-quality`, `update-docs`, `create-roadmap`.

---

## 📋 PR/Commit Checklist

**MANDATORY: Run this checklist at the end of EVERY task before considering it complete.**

> **How:** Follow the `pre-commit-checklist` skill in `.claude/SKILL.md` for the full gated checklist.

**A task is NOT complete until the pre-commit-checklist passes.**

**Documentation is NOT optional** - it is a critical requirement equal in importance to the code itself.

---

## 🔗 Project References

- [kube-rs documentation](https://kube.rs/)
- [Kubernetes API conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md)
- [Operator pattern](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- Internal: k0rdent platform docs (check Confluence)
