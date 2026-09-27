# Testing Standards

> Adapted from the sibling banlieue project's testing rule; this file is the
> binding statement of 5-Spot's testing discipline. The `tdd-workflow` skill holds the
> step-by-step procedure.

## CRITICAL: Test-Driven Development (TDD) Workflow

**MANDATORY: ALWAYS write tests FIRST before implementing functionality.**

Follow the Red-Green-Refactor cycle for ALL code changes.

> **How:** Follow the `tdd-workflow` skill (RED → GREEN → REFACTOR).

### When to Write Tests First

- ✅ **New features**: tests defining the behavior, then the implementation
- ✅ **Bug fixes**: a failing test that reproduces the bug, then the fix —
  and log the bug per `rules/openwolf.md`
- ✅ **Refactoring**: existing tests pass throughout; add edge-case tests
- ✅ **Performance work**: a test that pins the behavior, then optimize

### Exceptions to TDD

- Exploratory/prototype code (marked as such, removed before merging)
- Behavior-preserving refactors (existing tests verify correctness)

**REMEMBER**: If you're writing implementation code before tests, STOP and
write tests first.

---

## After Modifying Any `.rs` File

**CRITICAL: at the end of EVERY task that modifies Rust files, run the
`cargo-quality` skill** (fmt + clippy `-D warnings` + test — all three must
pass). Then verify:

1. **Rustdoc is accurate** — `# Arguments`, `# Errors`, examples all match
   the code as it now is.
2. **Tests are accurate** — assertions match the new behavior; new paths have
   new tests.
3. **End-user docs are updated** — `docs/src/`, `examples/`,
   `.claude/CHANGELOG.md`; example YAML still validates
   (`validate-examples` skill). CRD doc-comment changes also require the
   `regen-crds` → `regen-api-docs` regeneration.

---

## Test File Organization (hard rule, no exceptions)

**Unit tests live in separate `_tests.rs` files — never an inline
`mod tests { … }` body in a source file**, not even for one small test, for
ANY target: library modules, `src/bin/*.rs` binaries, and submodules alike.

The source file carries only the three-line declaration:

```rust
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;
```

and `src/foo_tests.rs` contains
`#[cfg(test)] mod tests { use super::super::*; … }`.

**Examples in this codebase:**
- `src/main.rs` → `src/main_tests.rs`
- `src/crd.rs` → `src/crd_tests.rs`
- `src/reconcilers/scheduled_machine.rs` → `src/reconcilers/scheduled_machine_tests.rs`
- `src/providers/time_based.rs` → `src/providers/time_based_tests.rs`

## Coverage bar

**Every function — public AND private — carries happy-path, negative, and
error-path tests.** Adding, changing, or deleting a function means adding,
changing, or deleting its tests in the same change. If you add code you
genuinely cannot test, document WHY in the code.

### Test quality

- Descriptive names (`test_reconcile_holds_state_when_provider_unresolved`)
- Arrange-Act-Assert; table-driven where a reconciler has many transitions
- Mock external dependencies (kube API, k0smotron, CAPI) behind traits
- Deterministic — no timing-dependent flakes, no real clocks in schedule
  evaluation tests

---

## The tiers, and which one to reach for

Each tier proves things the one below cannot; none substitutes for another.

| Tier | Where | Needs | Proves | Run with |
| --- | --- | --- | --- | --- |
| **Unit** | `src/*_tests.rs` | nothing | every decision as a pure function over a snapshot — schedule evaluation, phase transitions, parsing | `cargo test` |
| **Integration** | `tests/integration_*.rs` | nothing (mocked API) | reconciler flows against a mocked kube API: kata config, spot schedule, child kubeconfig, taints, reclaim | `cargo test --test <name>` |
| **kind e2e** | `make kind-setup` → `kind-deploy` → `kind-example` | a kind cluster + built image | the seams: CRDs install, RBAC suffices, the controller really reconciles | `make kind-*` targets |

Tests that need a real docker daemon (docker API `load`) do not work against
podman's compat socket — leave those to CI.

### Three rules the tiers exist to enforce

1. **A fake that is more permissive than the real thing hides bugs.** When a
   mock accepts what the real system rejects, every unit test passes while
   the second reconcile fails in a cluster. When a higher tier finds a
   behavior the fake got wrong, **fix the fake in the same change**.
2. **A test that skips must never report success.** An `#[ignore]`d live test
   that returns early when its cluster is missing prints `ok` while proving
   nothing — worse than no test, because it answers "is this covered?" with
   a confident yes. Missing fixtures fail loudly and name what is missing.
3. **A wait that stale state can satisfy is not a wait.** Wait on the thing
   that is actually *new* (the condition that could only exist after the
   action), not on a predicate that was already true before it — otherwise
   the assertion races the property under test.

---

## Integration Tests

Place in `/tests/` as `integration_<area>.rs`:
- Mock external services (k0smotron API, CAPI, provider CRs)
- Test failure scenarios, not just the happy path
- Test end-to-end workflows (create → update → delete)
- Verify finalizers and cleanup logic

---

## Test Execution

> **How:** Run the `cargo-quality` skill. For a specific module:
> `cargo test --lib <module_path>`. Verbose: `cargo test -- --nocapture`.

**ALL tests MUST pass before code is considered complete.**
