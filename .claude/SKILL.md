# Claude Skills Reference

Reusable procedural skills extracted from CLAUDE.md.
Each skill has a canonical name (kebab-case), trigger conditions, ordered steps,
and a verification check. Invoke a skill by name: *"run the cargo-quality
skill"* or *"do a verify-crd-sync"*.

---

## `verify-crd-sync`

**When to use:**
- Before investigating reconciliation loops or infinite loops
- Before debugging "field not appearing in kubectl output" issues
- After ANY modification to structs in `src/crd.rs`
- When status patches succeed but data doesn't persist
- When user reports unexpected controller behavior

**Steps:**
```bash
# 1. Check deployed CRD schema in cluster
kubectl get crd scheduledmachines.5spot.eribourg.dev -o yaml | grep -A 20 "<field-name>:"

# 2. Check Rust struct definition
rg -A 10 "pub struct <StructName>" src/crd.rs

# 3. If mismatch detected, regenerate CRDs
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml

# 4. Apply updated CRDs
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

**Verification:** Field appears in `kubectl get` output after patch; no infinite reconciliation loop.

---

## `regen-crds`

**When to use:**
- After ANY edit to Rust types in `src/crd.rs`
- Before deploying CRD changes to a cluster

**Steps:**
```bash
# 1. Regenerate CRD YAML file from Rust types
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml

# 2. Verify generated YAML
kubectl apply --dry-run=client -f deploy/crds/scheduledmachine.yaml

# 3. Update examples to match new schema (see validate-examples skill)

# 4. Deploy
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

**Verification:** `kubectl apply --dry-run=client -f deploy/crds/scheduledmachine.yaml` succeeds.

---

## `regen-api-docs`

**When to use:**
- After all CRD changes, example updates, and validations are complete (run this LAST)
- Before any documentation release

**Steps:**
```bash
# Regenerate API reference from CRD types.
# Prefer the Makefile target — it writes to the mdBook source path that
# actually renders on the doc site.
make crddoc
# Equivalent:
#   cargo run --bin crddoc > docs/src/reference/api.md
```

**Verification:** `docs/src/reference/api.md` reflects the current CRD schema.

---

## `cargo-quality`

**When to use:**
- After adding or modifying ANY `.rs` file
- Before committing any Rust code changes
- At the end of EVERY task involving Rust code (NON-NEGOTIABLE)

**Steps:**
```bash
# 0. Ensure cargo is in PATH
source ~/.zshrc

# 1. Format
cargo fmt

# 2. Lint with strict warnings (fix ALL warnings)
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions

# 3. Test (ALL tests must pass)
cargo test

# 4. Security audit (optional, if installed)
cargo audit 2>/dev/null || true
```

**Verification:** All three commands exit with code 0. No warnings, no test failures.

---

## `tdd-workflow`

**When to use:**
- Adding any new feature or function
- Fixing a bug
- Refactoring existing code

**Steps:**

**RED — Write failing tests first (before any implementation):**
```bash
# Edit src/<module>_tests.rs — add test(s) that define expected behavior
cargo test <test_name>   # Must FAIL at this point
```

**GREEN — Implement minimum code to pass tests:**
```bash
# Edit src/<module>.rs — write simplest code that makes tests pass
cargo test <test_name>   # Must PASS now
```

**REFACTOR — Improve while keeping tests green:**
```bash
# Extract constants, add docs, improve error handling
cargo test               # Must still PASS
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions
```

**Test file pattern:**
- Source: `src/foo.rs` → declare `#[cfg(test)] mod foo_tests;` at the bottom
- Tests: `src/foo_tests.rs` → wrap in `#[cfg(test)] mod tests { use super::super::*; ... }`

**Verification:** All tests pass, clippy is clean, test covers success path + error paths + edge cases.

---

## `update-changelog`

**When to use:**
- After ANY code modification (mandatory for auditing in a regulated environment)

**Steps:**

Open `.claude/CHANGELOG.md` and prepend an entry in this exact format:

```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** <Name of requester or approver>

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

**Verification:** Entry has `**Author:**` line (MANDATORY — no exceptions), timestamp, and at least one `### Changed` item.

---

## `sync-docs`

**When to use:**
- At the end of EVERY task (MANDATORY — same requirement as `cargo-quality`)
- Before opening a PR
- Any time docs may be out of sync with code

**Steps:**

For each of the following files, check the YAML examples and field descriptions
against `src/crd.rs` (source of truth) and `examples/*.yaml` (reference examples):

1. `docs/src/installation/quickstart.md` — YAML example in "Create Your First ScheduledMachine"
2. `docs/src/reference/api.md` — all Spec Fields, Status Fields, and the top-level example (auto-generated — regenerate with `make crddoc`, do not hand-edit)
3. `docs/src/advanced/capi-integration.md` — Bootstrap/Infrastructure sections and provider examples
4. `docs/src/concepts/scheduled-machine.md` — field tables (types, defaults, required flags)
5. Any other `.md` file under `docs/` that contains a YAML snippet with `kind: ScheduledMachine`

**What to check:**

- Field names match the Rust struct (remember: `snake_case` in Rust → `camelCase` in YAML)
- No non-existent fields (common culprits: `machine`, `bootstrapRef`/`infrastructureRef` at spec level)
- `bootstrapSpec` and `infrastructureSpec` use inline `spec:` — NOT `name:`/`namespace:` refs
- `priority` range is 0-255 (not 0-100)
- Phase values match: `Pending`, `Active`, `ShuttingDown`, `Inactive`, `Disabled`, `Terminated`, `Error`
- Status field names: `lastScheduledTime`, `nextActivation`, `nextCleanup`, `inSchedule`
- `machineRef` has `apiVersion`, `kind`, `name`, `namespace` — no `uid`

**Verification:** No field in any doc example diverges from `src/crd.rs`.

---

## `update-docs`

**When to use:**
- After any code change in `src/`
- After CRD changes, API changes, configuration changes, or new features

**Steps:**
1. Identify what changed (feature, CRD field, behavior, error condition).
2. Update `.claude/CHANGELOG.md` (see `update-changelog` skill).
3. Update affected pages in `docs/`:
   - User guides, quickstart guides, configuration references, troubleshooting guides
4. Update `examples/*.yaml` to reflect schema or behavior changes.
5. If CRDs changed: run `regen-api-docs` skill (LAST step).
6. If README getting-started or features changed: update `README.md`.

**Verification checklist:**
- [ ] `.claude/CHANGELOG.md` updated with author
- [ ] All affected `docs/` pages updated
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] API docs regenerated if CRDs changed

---

## `validate-examples`

**When to use:**
- After any CRD schema change
- Before committing changes to `examples/`
- As part of the `pre-commit-checklist`

**Steps:**
```bash
# Validate all example YAML files
kubectl apply --dry-run=client -f examples/

# Or validate individually
for file in examples/*.yaml; do
  echo "Validating $file"
  kubectl apply --dry-run=client -f "$file"
done
```

**Verification:** All files pass dry-run with no errors. No `unknown field` or `required field missing` errors.

---

## `add-new-crd`

**When to use:**
- When adding a new Custom Resource Definition to the operator

**Steps:**
1. Add the new `CustomResource` struct to `src/crd.rs`:
   ```rust
   #[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
   #[kube(
       group = "5spot.eribourg.dev",
       version = "v1alpha1",
       kind = "MyNewResource",
       namespaced
   )]
   #[serde(rename_all = "camelCase")]
   pub struct MyNewResourceSpec {
       pub field_name: String,
   }
   ```
2. Register it in `src/bin/crdgen.rs`.
3. Run `regen-crds` skill.
4. Add examples to `examples/`.
5. Run `validate-examples` skill.
6. Add documentation in `docs/`.
7. Run `regen-api-docs` skill (LAST).
8. Run `cargo-quality` skill.
9. Run `update-changelog` skill.

**Verification:** `kubectl apply --dry-run=client -f deploy/crds/<newresource>.yaml` succeeds; API docs include the new resource.

---

## `pre-commit-checklist`

**When to use:**
- Before committing any change (mandatory gate)

**Checklist:**

### If the change is architecturally significant (ADD — do FIRST):
See `.claude/rules/architecture-driven-development.md`. Applies to new CRDs / CRD-field contract changes, controllers/reconcilers/binaries, CAPI-interaction changes, deploy/admission/GitOps topology, and security/RBAC/scheduling concerns.
- [ ] ADR written/updated in `docs/adr/NNNN-*.md` (Status/Context/Decision/Consequences) and indexed in `docs/adr/README.md`
- [ ] CALM model updated, `make calm-validate` + `make calm-diagrams` pass — OR the ADR states "no CALM impact (process-only)"
- [ ] ADR was written BEFORE the implementation (ADR → CALM → TDD order)

### If ANY `.rs` file was modified:
- [ ] Tests updated/added/deleted to match changes (TDD — see `tdd-workflow`)
- [ ] All new public functions have tests
- [ ] All deleted functions have tests removed
- [ ] `cargo fmt` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes (fix ALL warnings)
- [ ] `cargo test` passes (ALL tests green)
- [ ] Rustdoc comments on all public items, accurate to actual behavior
- [ ] `docs/` updated for user-facing changes

### If `src/crd.rs` was modified:
- [ ] `cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml` run
- [ ] `examples/*.yaml` updated to match new schema
- [ ] `docs/` documentation updated
- [ ] `kubectl apply --dry-run=client -f examples/` passes
- [ ] `make crddoc` run (LAST) — writes to `docs/src/reference/api.md` (mdBook source)

### If `src/reconcilers/` was modified:
- [ ] Reconciliation flow diagrams updated in `docs/`
- [ ] `docs/scheduledmachine-lifecycle.rst` updated if phase transitions changed
- [ ] New behaviors documented in user guides
- [ ] Troubleshooting guides updated for new error conditions

### Always:
- [ ] `sync-docs` skill passed — no doc/code divergence
- [ ] `.claude/CHANGELOG.md` updated with **Author:** line (MANDATORY)
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] `kubectl apply --dry-run=client -f deploy/crds/` succeeds
- [ ] No secrets, tokens, credentials, internal hostnames, or IP addresses committed
- [ ] No `.unwrap()` in production code

### If preparing a release:
- [ ] Every open Trivy finding has a corresponding statement in `.vex/` (triaged into one of: `not_affected`, `affected`, `fixed`, `under_investigation`). No silent "unknown" CVEs leave the door.
- [ ] `make vex-validate` exits 0 (every `.vex/*.json` parses cleanly via `vexctl merge`).
- [ ] `.vex/` changes referenced in the release notes / changelog.

**Verification:** Every checked box above passes. A task is NOT complete until the full checklist is green.

---

## `create-roadmap`

**When to use:**
- When planning a new feature or multi-phase implementation
- When documenting future work or optimization strategies

**Steps:**
1. Create file in `docs/roadmaps/` with **lowercase, hyphenated** filename
2. Include header with date, status, and impact
3. Structure with phases, milestones, or steps
4. Add success criteria and verification steps

**File naming rules:**
- ✅ CORRECT: `docs/roadmaps/integration-test-plan.md`
- ✅ CORRECT: `docs/roadmaps/phase-1-implementation.md`
- ❌ WRONG: `ROADMAP.md` (root directory)
- ❌ WRONG: `docs/roadmaps/PHASE_1.md` (uppercase, underscores)

**Verification:** File exists in `docs/roadmaps/`, filename is lowercase with hyphens only.

---

## `search-codebase`

**When to use:**
- Finding code definitions, usages, or patterns
- Investigating where a function or type is used

**Steps:**
```bash
# Search in Rust files only
rg -trs "<pattern>" . -g '!target/'

# Find function definitions
rg -trs "fn <function_name>" . -g '!target/'

# Find struct definitions
rg -trs "pub struct <StructName>" . -g '!target/'

# Find all usages of a constant
rg -trs "<CONSTANT_NAME>" . -g '!target/'
```

**Verification:** Results show relevant matches without target/ directory noise.
