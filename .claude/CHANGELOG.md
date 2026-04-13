# Changelog

All notable changes to the 5spot project will be documented in this file.

The format is based on the regulated environment requirements:
- **Author attribution is MANDATORY** for all entries
- Changes are logged in reverse chronological order
- Each entry must include impact assessment

---

## [2026-04-10 08:50] - Extend Cosign image signing to main-branch pushes

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`: Changed Cosign signing step condition from `github.event_name == 'release'` to `github.event_name != 'pull_request'`

### Why
Main-branch images are tagged `latest` and `main-YYYY-MM-DD` and may be deployed to staging. Signing them allows `cosign verify` to work on staging images, not just production releases. PR images remain unsigned — they are ephemeral, tagged `pr-{number}`, and not deployed anywhere.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-10 08:45] - Fix attest job: add GHCR login before push-to-registry

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`: Added `docker/login-action@v3` step to the `attest` job before `actions/attest-build-provenance@v2`

### Why
`push-to-registry: true` in `actions/attest-build-provenance` pushes the attestation bundle as an OCI artifact to GHCR, which requires registry credentials. Each job runs in a fresh environment — the Docker login performed by `firestoned/github-actions/docker/setup-docker` in the `docker` job does not carry over to the `attest` job.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-10 08:30] - Consolidate pr.yaml, main.yaml, release.yaml into single build.yaml

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`: New consolidated workflow replacing the three separate files; triggers on `pull_request`, `push` to main, and `release: published`; uses `if:` at job and step level to gate event-specific behaviour
- `.github/workflows/pr.yaml`: Deleted
- `.github/workflows/main.yaml`: Deleted
- `.github/workflows/release.yaml`: Deleted

### Why
Three workflows shared the same build matrix, env vars, and most job logic, requiring the same fix to be applied in three places (e.g. the linker override, the attest job). A single file is easier to maintain and gives a complete picture of CI behaviour in one place.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

> **Key design decisions:**
> - `extract-version` serves as the quality gate: it runs after all checks that apply to the current event (`verify-commits`, `license-check`, `format`) and all downstream jobs depend on it.
> - Docker metadata uses three separate `docker/metadata-action` steps gated by `if:`; `docker/build-push-action` concatenates all outputs and filters empty lines.
> - Cosign signing, Docker SBOM generation, `sign-artifacts`, SLSA provenance, and `upload-release-assets` are guarded by `if: github.event_name == 'release'`.
> - `test` and `format`/`clippy` are guarded by `if: github.event_name == 'pull_request'`.
> - `trivy` is guarded by `if: github.event_name != 'pull_request'`.
> - Artifact retention: PR/push = 1 day (two upload steps); release = default.

---

## [2026-04-10 07:45] - Add GitHub artifact attestation job to all three CI/CD workflows

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/pr.yaml`: Added `id: docker_build` to docker build step; added `outputs:` block to docker job (per-variant digests); added `export-digest` step; added `attest` job depending on `docker` and `extract-version`
- `.github/workflows/main.yaml`: Same additions to docker job and new `attest` job
- `.github/workflows/release.yaml`: Same additions to `docker-release` job and new `attest` job (depends on `docker-release`)

### Why
GitHub's `actions/attest-build-provenance` generates a signed SLSA provenance attestation stored natively in GitHub Artifact Attestations and optionally pushed to the OCI registry alongside the image. This is queryable with `gh attestation verify` and complements the existing Cosign signatures in the release workflow. Requires the matrix digest-export pattern to pass per-image digests from a matrix job to a downstream job.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-08 17:30] - Add documentation build and GitHub Pages deployment workflow

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/docs.yaml`: New workflow — builds MkDocs documentation (including `make docs` which runs `cargo run --bin crddoc`) and deploys to GitHub Pages on push to main; runs link checks on PRs

### Why
The project has a full MkDocs documentation site under `docs/` but no automated build or publishing pipeline. This workflow closes that gap by building on every relevant change, checking for broken links on PRs, and publishing to GitHub Pages on every merge to main.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

> **Note:** GitHub Pages must be enabled in the repository settings (`Settings → Pages → Source: GitHub Actions`) for the deploy job to succeed. The `poetry.lock` file should be committed after the first `poetry install` run to improve cache efficiency and build reproducibility.

---

## [2026-04-09 11:00] - Fix CI linker error caused by .cargo/config.toml on Linux runners

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/pr.yaml`: Added `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER: cc` and `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: cc` to the top-level `env:` block
- `.github/workflows/main.yaml`: Same
- `.github/workflows/release.yaml`: Same

### Why
`.cargo/config.toml` specifies `linker = "x86_64-unknown-linux-gnu-gcc"` and `linker = "aarch64-unknown-linux-gnu-gcc"` for the respective targets — Homebrew cross-compilers needed when a macOS developer uses `cargo build --target <linux-triple>` locally (the Makefile fallback path). On Linux CI runners, `cargo build --release` with no explicit target resolves to the **native** triple (e.g., `x86_64-unknown-linux-gnu` on `ubuntu-latest`), which picks up the same override and fails because the cross-compiler is not installed on GitHub Actions runners. `CARGO_TARGET_*_LINKER` environment variables take precedence over `config.toml`, restoring `cc` (the system linker) in CI without modifying the config file.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-09 10:00] - Replace cross-compilation with native cargo builds in CI

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/pr.yaml`: Replaced `firestoned/github-actions/rust/setup-rust-build@v1.3.6` + `build-binary@v1.3.6` + `generate-sbom@v1.3.6` with `dtolnay/rust-toolchain@stable` + `cargo build --release` + `cargo-cyclonedx`; switched ARM64 build to `ubuntu-24.04-arm` native runner; updated artifact paths from `target/$target/release/` to `target/release/`; fixed `license-id: "MIT"` → `"Apache-2.0"`
- `.github/workflows/main.yaml`: Same build and license-check changes
- `.github/workflows/release.yaml`: Same build and license-check changes; replaced `setup-rust-build` in `package-deploy-manifests` job with `dtolnay/rust-toolchain@stable`

### Why
The `firestoned/github-actions/rust/build-binary@v1.3.6` action internally sets `-C linker=x86_64-unknown-linux-gnu-gcc`, which is not installed on GitHub Actions `ubuntu-latest` runners, causing all build jobs to fail. Native `cargo build --release` on arch-appropriate runners eliminates cross-compilation entirely. License-id was stale after the FINOS Apache-2.0 migration.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-09 03:00] - Phase 2 (P2-4): leader election via kube-lease-manager

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Added `kube-lease-manager = "0.11"` dependency
- `src/constants.rs`: Added `DEFAULT_LEASE_NAME`, `DEFAULT_LEASE_DURATION_SECS`, `DEFAULT_LEASE_RENEW_DEADLINE_SECS`, `DEFAULT_LEASE_RETRY_PERIOD_SECS`, `DEFAULT_LEASE_GRACE_SECS`, `DEFAULT_LEASE_NAMESPACE` constants
- `src/reconcilers/scheduled_machine.rs`: Added `is_leader: Arc<AtomicBool>` to `Context` (defaults to `true` for backward-compatible single-instance mode); added leader guard in `reconcile_guarded` — non-leaders return `Action::await_change()` immediately
- `src/main.rs`: Added `enable_leader_election`, `lease_name`, `lease_namespace`, `lease_duration_secs`, `lease_renew_deadline_secs` CLI args; when enabled, sets `is_leader = false` at startup and spawns a background `kube-lease-manager` task that flips `is_leader` on acquisition/loss
- `deploy/deployment/deployment.yaml`: Fixed `POD_NAME` → `CONTROLLER_POD_NAME` env var (aligns with `Context::new` and leader election holder identity)
- `src/reconcilers/scheduled_machine_tests.rs`: Added 2 TDD tests — `test_context_new_defaults_is_leader_to_true` and `test_reconcile_guarded_awaits_change_when_not_leader`
- `docs/src/operations/configuration.md`: Added all leader election env vars, CLI args, Leader Election section, Lease RBAC rules

### Why
Basel III HA (P2-4): a single-replica controller is a single point of failure. With `ENABLE_LEADER_ELECTION=true` and `replicas: 2`, only the lease holder reconciles resources. Standby replicas react within one `LEASE_DURATION_SECONDS` window on leader failure. `Context::is_leader` defaults to `true` so existing single-replica deployments continue without any config change.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — set `ENABLE_LEADER_ELECTION=true` and `replicas: 2`; RBAC for `leases` already in `clusterrole.yaml`
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-09 02:00] - Add SPDX license headers to all .github YAML files

**Author:** Erick Bourgeois

### Changed
- `.github/ISSUE_TEMPLATE/bug_report.yml`: Added `# Copyright (c) 2025 Erick Bourgeois, finos` + `# SPDX-License-Identifier: Apache-2.0` header
- `.github/ISSUE_TEMPLATE/feature_request.yml`: Same
- `.github/ISSUE_TEMPLATE/meeting_minutes.yml`: Same
- `.github/ISSUE_TEMPLATE/support_question.yml`: Same

### Why
Supply-chain provenance and automated license scanning (NIST SA-4) require SPDX headers on all project-owned files. The three workflow files and both composite actions already had headers from P2-10; these four issue templates were the remaining `.github/` YAML files without them. `dco.yml` was intentionally left untouched — it is managed by FINOS and carries an explicit "Do not edit" notice.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-09 01:00] - Phase 2 (P2-5): exponential back-off in error policy

**Author:** Erick Bourgeois

### Changed
- `src/constants.rs`: Added `MAX_RECONCILE_RETRIES: u32 = 10` constant
- `src/reconcilers/scheduled_machine.rs`: Added `retry_counts: Arc<Mutex<HashMap<String, u32>>>` to `Context`; updated `Context::new` to initialise it; updated `reconcile_guarded` to clear the retry count on successful reconciliation
- `src/reconcilers/helpers.rs`: Added `compute_backoff_secs(retry_count: u32) -> u64` (pure, capped exponential); replaced fixed-delay `error_policy` with retry-count-aware implementation that increments the per-resource counter and computes `ERROR_REQUEUE_SECS * 2^n` capped at `MAX_BACKOFF_SECS`
- `src/reconcilers/helpers_tests.rs`: Added 5 TDD tests for `compute_backoff_secs` (base, doubling, cap at retry 4, cap at MAX_RECONCILE_RETRIES, large count)

### Why
Basel III HA resilience (P2-5): a fixed 30 s retry interval can cause thundering-herd pressure when many resources fail simultaneously. Bounded exponential back-off distributes retry load while ensuring eventual recovery. Retry counts are cleared on success so transient failures do not permanently elevate delay. Aligns with NIST SI-2 flaw remediation by limiting retry storms.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-09 00:00] - Phase 2 (P2-3/P2-7): reconciliation correlation IDs and Condition status enum

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/scheduled_machine.rs`: Added `generate_reconcile_id()` — derives a short correlation ID from the resource's UID last segment + nanosecond hex timestamp; refactored `reconcile_scheduled_machine` into `reconcile_guarded` wrapped in a `tracing::info_span!` carrying `reconcile_id`, `resource`, and `namespace` — every log line in a reconciliation now carries these fields in JSON output (NIST AU-3 / SOX §404 P2-3)
- `src/reconcilers/scheduled_machine_tests.rs`: Added 5 TDD tests for `generate_reconcile_id()` covering non-empty output, UID-last-segment prefix, hex timestamp suffix, unknown-fallback when no UID, and uniqueness across calls
- `src/crd.rs`: Added `condition_status_schema()` and wired it to `Condition.status` via `#[schemars(schema_with = "...")]` — constrains the CRD field to `enum: [True, False, Unknown]` (NIST CM-5 / P2-7)
- `src/crd_tests.rs`: Added 5 TDD tests for `Condition.status` schema enum: constraint exists, all three values present, and runtime `Condition::new()` still accepts string status unchanged
- `deploy/crds/scheduledmachine.yaml`: Regenerated — `Condition.status` now has `enum: [True, False, Unknown]` in the CRD OpenAPI schema
- `docs/reference/api.md`: Regenerated to reflect schema change

### Why
- **P2-3**: Every reconciliation now emits a unique `reconcile_id` on all log lines via a `tracing` span, enabling full end-to-end correlation in a SIEM or log aggregation platform. Closes the NIST AU-3 / SOX §404 correlation ID gap.
- **P2-7**: The `Condition.status` field previously accepted any string; the CRD schema now enforces the Kubernetes-standard `True`/`False`/`Unknown` enum as required by NIST CM-5 configuration change control. Runtime behaviour is unchanged — the constraint is schema-only.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — CRD must be reapplied (`kubectl apply -f deploy/crds/scheduledmachine.yaml`); existing CRs with valid status values are unaffected
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-08 15:00] - Add Security section to MkDocs with Admission Validation guide

**Author:** Erick Bourgeois

### Added
- `docs/src/security/index.md`: New security section landing page — security posture at a glance table, compliance mapping summary, and links to sub-pages
- `docs/src/security/admission-validation.md`: Comprehensive user-facing guide for the `ValidatingAdmissionPolicy` covering: VAP vs. webhook comparison table, Mermaid admission flow sequence diagram, full 13-rule reference table with per-rule detail and examples, deployment instructions, rollout strategy (Audit → Deny → AuditAndDeny), four concrete kubectl test examples, namespace scoping guidance, and Kubernetes version compatibility table
- `docs/mkdocs.yml`: Added `Security` top-level nav section (between Advanced Topics and Developer Guide) containing Overview, Admission Validation, and Threat Model pages

### Why
The `ValidatingAdmissionPolicy` deployed in the previous entry had no user-facing documentation. Operators need to know what is validated, how to deploy it, how to do a safe rollout, and how to test it. The new Security section also surfaces the threat model in the main navigation — previously it existed only in the repo but was not reachable from the docs site.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-08 14:00] - Phase 2 (P2-6/P2-8/P2-9/P2-10): eviction correctness, JSON logging, supply-chain provenance

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Fixed P2-6 — `evict_pod` 429 PDB-blocked arm now returns `Err(ReconcilerError::CapiError(...))` instead of silently returning `Ok(())`; log level raised from `info` to `warn`; doc comment updated to remove the incorrect "429 is not an error" statement
- `src/reconcilers/helpers_tests.rs`: Added 5 TDD mock API tests for `evict_pod` covering: success (200), already-deleted (404 → Ok), PDB-blocked (429 → CapiError), server error (500 → CapiError), and forbidden (403 → CapiError)
- `src/main.rs`: Wired P2-8 — added `--log-format` CLI arg mapped to `RUST_LOG_FORMAT` env var (default `"json"`); tracing subscriber now uses `.json()` layer for `json` and plain text layer for `text`/anything else
- `deploy/deployment/deployment.yaml`: Changed `RUST_LOG_FORMAT` default from `"text"` to `"json"` so production pods emit structured JSON for SIEM ingestion
- `src/**/*.rs` (all 18 files): Added P2-10 SPDX supply-chain provenance headers to every Rust source file:
  ```
  // Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
  // SPDX-License-Identifier: Apache-2.0
  ```

### Why
- **P2-6**: A PDB-blocked eviction (HTTP 429) was silently treated as success, causing the drain loop to believe the pod was evicted when it wasn't — a data-integrity bug that could leave a node non-empty. Now propagated as `CapiError` so the caller can decide to retry or abort.
- **P2-8**: Structured JSON logging is required for SIEM ingestion and NIST AU-3 compliance; `text` format was only appropriate for local development.
- **P2-9**: `cargo-audit 0.22.0` was already running via `firestoned/github-actions/rust/security-scan@v1.3.6` on all PRs and main — no code change required, marked ✅ in roadmap.
- **P2-10**: SPDX headers enable automated license scanning and supply-chain provenance tracking per NIST SA-4.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — `RUST_LOG_FORMAT=json` default; existing log parsers expecting plain text must be updated
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-08 13:00] - Add ValidatingAdmissionPolicy for ScheduledMachine (NIST CM-5)

**Author:** Erick Bourgeois

### Added
- `deploy/admission/validatingadmissionpolicy.yaml`: `ValidatingAdmissionPolicy` with 13 CEL validation rules covering: `clusterName` non-empty; `gracefulShutdownTimeout`/`nodeDrainTimeout` duration format (`^\d+[smh]$`); `cron` XOR `daysOfWeek`/`hoursOfDay` mutual exclusivity; `daysOfWeek` day-name/range item format; `hoursOfDay` hour/range item format; `bootstrapSpec`/`infrastructureSpec` apiVersion namespaced-group requirement; bootstrap/infrastructure provider API group allowlist (mirrors `ALLOWED_BOOTSTRAP_API_GROUPS` / `ALLOWED_INFRASTRUCTURE_API_GROUPS` in `src/constants.rs`); `bootstrapSpec.kind`/`infrastructureSpec.kind` non-empty
- `deploy/admission/validatingadmissionpolicybinding.yaml`: `ValidatingAdmissionPolicyBinding` with `validationActions: [Deny]` applied cluster-wide

### Changed
- `docs/roadmaps/compliance-sox-basel3-nist.md`: Marked P3-4, CM-5, and all CRD schema validation gaps as resolved; updated gap table and compliance control mapping
- `docs/roadmaps/project-roadmap-2026.md`: Updated Phase 3.1 Admission Webhooks → Admission Validation; checked off all implemented rules; noted future mutating webhook and reference-existence check as separate items
- `docs/src/security/threat-model.md`: Updated Deployment-Layer Controls table to reflect VAP deployed (was a recommendation)

### Why
`ValidatingAdmissionPolicy` (Kubernetes ≥ 1.26) enforces spec constraints at API-server admission time without requiring a separate webhook server, TLS certificate, or additional binary. Closes the NIST CM-5 gap: invalid specs that previously reached the reconciler are now rejected before being persisted to etcd.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — apply `deploy/admission/` manifests; requires Kubernetes ≥ 1.26 (alpha), ≥ 1.28 (beta), ≥ 1.30 (GA)
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-08 12:00] - Complete rustdoc coverage across all Rust source files

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Expanded thin one-liner docs on all remaining functions — `add_finalizer`, `handle_deletion`, `handle_kill_switch`, `check_grace_period_elapsed`, `update_phase_with_last_schedule`, `update_phase_with_grace_period`, `bootstrap_resource_name`, `infrastructure_resource_name`, `machine_resource_name`, `create_dynamic_resource`, `parse_api_version`, `remove_machine_from_cluster`, `should_evict_pod`, `evict_pod`, and `error_policy` — with full `///` docs covering purpose, behaviour details, and `# Errors` sections
- `src/bin/crdgen.rs`: Replaced `//` comment header with `//!` module doc explaining purpose, usage, and regeneration requirement; added `///` on `main()` with `# Panics` note
- `src/bin/crddoc.rs`: Added `//!` module doc explaining purpose, usage, and implementation note about static-println generation vs schema-driven approach; added `///` on `main()`

### Why
All public items and binary entry points now have complete rustdoc coverage to satisfy the project's documentation standard (CLAUDE.md §Code Comments) and to provide clear in-IDE guidance for future contributors.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-08 00:03] - Phase 2 (P2-1/P2-2): Kubernetes Event audit trail and before/after phase logging

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/scheduled_machine.rs`: Added `Recorder` field to `Context` struct (created from `Reporter` with controller name and pod name); removed separate `client`/`recorder` args from all `update_phase*` call sites — now pass `&ctx` directly
- `src/reconcilers/helpers.rs`: Added `build_phase_transition_event()` pure function that constructs a `KubeEvent` from phase transition parameters (`Warning` for `Error`/`Terminated`, `Normal` otherwise); updated `update_phase()`, `update_phase_with_last_schedule()`, and `update_phase_with_grace_period()` to accept `&Context` (replacing separate `&Client` + `&Recorder` params), log `from → to` phase transition at `INFO` level, and publish an immutable Kubernetes Event via the recorder (best-effort — failures emit `WARN` but do not abort the transition)
- `deploy/deployment/rbac/clusterrole.yaml`: Added `events.k8s.io` / `events` create+patch rule alongside the existing core `""` events rule (kube-rs Recorder uses the `events.k8s.io/v1` API)
- `src/reconcilers/helpers_tests.rs`: Added 7 unit tests for `build_phase_transition_event()` covering Normal/Warning event types, note format, unknown from-phase fallback, action field, and reason field

### Why
P2-1 and P2-2 from the SOX/Basel III/NIST compliance roadmap. Every machine phase transition now writes an immutable Kubernetes Event visible via `kubectl describe scheduledmachine <name>`, providing an auditable record of state changes required by SOX §404 (immutable audit trail) and NIST AU-2/AU-3 (event recording and audit record content). Before/after logging closes the gap against AU-3 by making the previous phase explicit in each log line.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-08 00:02] - Phase 1 Compliance Remediation (SOX/Basel III/NIST SP 800-53)

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/scheduled_machine.rs`: Replaced 7 `unwrap()` calls with `ok_or_else` error propagation in all phase handlers (`reconcile_inner`, `handle_pending_phase`, `handle_active_phase`, `handle_shutting_down_phase`, `handle_inactive_phase`, `handle_disabled_phase`, `handle_error_phase`) — NIST SI-3, P1-1
- `src/reconcilers/helpers.rs`: Replaced 3 `unwrap()` calls with `ok_or_else` error propagation in `add_finalizer`, `handle_deletion`, and `handle_kill_switch` — NIST SI-3, P1-1
- `src/metrics.rs`: Replaced 11 `.expect()` panics with graceful fallback pattern using private helper functions (`fallback_counter_vec`, `fallback_gauge`, `fallback_gauge_vec`, `fallback_histogram_vec`); metrics initialization failures now log a warning and continue rather than crash — NIST SI-3, P1-2
- `deploy/deployment/networkpolicy.yaml`: Created Kubernetes NetworkPolicy implementing NIST SC-7 boundary protection — ingress restricted to Prometheus scrape (port 8080, monitoring namespace only) and kubelet probes (port 8081); egress restricted to DNS (port 53) and Kubernetes API server (port 6443) — NIST SC-7, P1-3
- `src/main.rs`: Explicitly applied `K8S_API_TIMEOUT_SECS` constant to `kube::Config` `read_timeout` and `write_timeout` fields to enforce connection timeouts against Kubernetes API server — Basel III operational resilience, P1-4

### Why
Phase 1 of the SOX/Basel III/NIST SP 800-53 compliance remediation roadmap (`docs/roadmaps/compliance-sox-basel3-nist.md`). All `unwrap()`/`expect()` calls in production code paths represent potential uncontrolled panics that violate NIST SI-3 (Malicious Code Protection) and operational resilience requirements. The NetworkPolicy enforces least-privilege network access per NIST SC-7. Explicit API timeouts align with Basel III operational resilience requirements for bounded failure modes.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-08 00:01c] - Convert all remaining ASCII diagrams to Mermaid

**Author:** Erick Bourgeois

### Changed
- `docs/src/concepts/schedules.md`: Converted cron field reference ASCII art to `flowchart LR` Mermaid diagram (5 labelled field nodes)

### Why
Project standard requires all diagrams to use Mermaid. This was the last remaining ASCII diagram across all docs.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-08 00:01b] - Convert threat model ASCII diagrams to Mermaid

**Author:** Erick Bourgeois

### Changed
- `docs/src/security/threat-model.md`: Converted Section 2 system overview ASCII art to `flowchart TB` Mermaid diagram; converted Section 4 trust boundaries text block to `flowchart LR` Mermaid diagram

### Why
Project standard is Mermaid for all diagrams (consistent with architecture.md and other docs/src files).

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-08 00:01] - Add threat model document

**Author:** Erick Bourgeois

### Changed
- `docs/src/security/threat-model.md`: New STRIDE threat model covering all controller components, trust boundaries, threat actors, 30+ threats with likelihood/impact ratings, full mitigations matrix, and 6 residual risk items with remediation guidance

### Why
Regulatory requirement in a banking environment: all security-significant components must have a documented threat model traceable to identified controls. This also captures the rationale behind the security hardening changes made in the same session.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-08 00:00] - Add GitHub Actions CI/CD Workflows

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/pr.yaml`: Pull Request CI — lint, test, Linux binary builds, Docker build/push, security scan
- `.github/workflows/main.yaml`: Main branch CI/CD — builds, Docker push (latest + date tags), security scan, Trivy container scan
- `.github/workflows/release.yaml`: Release workflow — versioned Docker images with Cosign signing, SLSA provenance, binary signing, deploy manifest packaging, release asset upload

### Why
Establish baseline CI/CD pipeline for the 5-spot operator using the same firestoned GitHub Actions patterns as bindy. Linux-only builds (x86_64 + ARM64) since this is a Kubernetes operator.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-08 00:00] - Security hardening: namespace isolation, input validation, RBAC narrowing

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Removed `namespace` field from `EmbeddedResource` — bootstrap and infrastructure resources are now always created in the ScheduledMachine's own namespace, preventing cross-namespace attacks
- `src/crd.rs`: Added `timezone_schema()` with `maxLength: 64` and character-class pattern constraint to block log injection via the timezone field
- `src/reconcilers/helpers.rs`: Fixed integer overflow in `parse_duration()` — now uses `checked_mul` and rejects durations exceeding 24 hours (`MAX_DURATION_SECS`)
- `src/reconcilers/helpers.rs`: Added `validate_labels()` — rejects label/annotation keys using reserved prefixes (`kubernetes.io/`, `k8s.io/`, `cluster.x-k8s.io/`, `5spot.finos.org/`) before merging into CAPI Machine resources
- `src/reconcilers/helpers.rs`: Added `validate_api_group()` — enforces an allowlist of permitted API groups for bootstrap and infrastructure embedded resources; blocks core Kubernetes APIs (`v1`, `rbac.authorization.k8s.io/v1`, etc.)
- `src/reconcilers/helpers.rs`: Wrapped `remove_machine_from_cluster` in `tokio::time::timeout` inside `handle_deletion` — finalizer cleanup now has a hard 10-minute deadline, preventing indefinite namespace deletion blocks
- `src/reconcilers/scheduled_machine.rs`: Added `ValidationError` and `TimeoutError` variants to `ReconcilerError`
- `src/constants.rs`: Added `MAX_DURATION_SECS`, `MAX_TIMEZONE_LEN`, `FINALIZER_CLEANUP_TIMEOUT_SECS`, `RESERVED_LABEL_PREFIXES`, `ALLOWED_BOOTSTRAP_API_GROUPS`, `ALLOWED_INFRASTRUCTURE_API_GROUPS`
- `deploy/deployment/rbac/clusterrole.yaml`: Narrowed `k0smotron.io` resources from wildcard to explicit list (`k0sworkerconfigs`, `remotemachines` and their `/status` subresources)
- `src/reconcilers/helpers_tests.rs`: New test file — 25 security-focused tests covering overflow protection, reserved label rejection, and API group allowlist enforcement
- `src/crd_tests.rs`, `src/reconcilers/scheduled_machine_tests.rs`: Removed `namespace: None` from `EmbeddedResource` test fixtures (field removed)

### Why
Comprehensive security audit identified: cross-namespace resource creation via user-controlled namespace overrides, integer overflow in duration parsing, label injection into CAPI resources, unbounded apiVersion/kind inputs, and missing finalizer cleanup timeouts. These are now all addressed to meet zero-trust security requirements for a regulated banking environment.

### Impact
- [x] Breaking change — `EmbeddedResource.namespace` field removed from CRD schema (existing CRs with this field: Kubernetes ignores unknown fields, no action required)
- [x] Requires cluster rollout — CRDs must be regenerated (`regen-crds` skill) before deploying
- [ ] Config change only
- [ ] Documentation only

---

## [2026-03-21 12:00] - Adopt .claude Skills Structure

**Author:** Erick Bourgeois

### Changed
- Created `.claude/` directory with SKILL.md and CHANGELOG.md
- Adopted skills-based workflow from bindy project
- Updated documentation structure for better organization

### Why
Standardize project instructions and skills across projects, improving consistency and making procedures reusable and discoverable.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-01-18 12:00] - Add VMware cloud-init preparation script

**Author:** Unknown

### Added
- `scripts/install-cloud-init.sh`: Linux-only script to convert VMDK→raw, mount LVM with conflict-safe handling, chroot to install `cloud-init` and `open-vm-tools`, optional initramfs rebuild, raw→streamOptimized VMDK, and import as vSphere template via `govc`.

### Why
Enable automated preparation and deployment of a cloud-init-enabled RHEL image on a VMware VM. Credentials and vSphere target configuration are provided via environment variables to avoid storing secrets in code.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

---

## [2026-01-18 17:45] - Harden govc VM existence check in upload script

**Author:** Unknown

### Changed
- `scripts/install-cloud-init.sh`: Replaced fragile `govc vm.info`-based existence check with robust `govc find -type m -name <name>` logic; iterates over matched inventory paths, converts templates to VMs when needed, and destroys them before import.

### Why
`govc vm.info` can return exit code 0 with no output, leading to false positives. Using `govc find` and inspecting inventory paths provides reliable detection of existing VMs/templates with the target name and avoids confusing "not found" errors.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

---

## [2026-01-18 18:30] - Simplify LVM VG handling with isolated system directory

**Author:** Unknown

### Changed
- `scripts/install-cloud-init.sh`: Use `LVM_SYSTEM_DIR` to isolate loop device LVM metadata to a separate directory (`/tmp/lvm-loop-$$`); use temporary VG name (`vg00_loop`) if host has same VG name to avoid device-mapper conflicts in `/dev/mapper/`.

### Why
Device-mapper device names in `/dev/mapper/` are global at the kernel level, even with isolated LVM metadata via `LVM_SYSTEM_DIR`. If both host and loop device have `vg00` with LVs named `root`, `var`, etc., device-mapper refuses to create duplicate devices ("Device or resource busy"). By using `vgimportclone -n vg00_loop` when a conflict exists, we give the loop device VG a unique name for device-mapper while keeping metadata isolated. No rename needed after deactivation since the isolated metadata directory is simply deleted.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only
