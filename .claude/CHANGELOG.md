# Changelog

All notable changes to the 5spot project will be documented in this file.

The format is based on the regulated environment requirements:
- **Author attribution is MANDATORY** for all entries
- Changes are logged in reverse chronological order
- Each entry must include impact assessment

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
- `src/reconcilers/helpers.rs`: Added `validate_labels()` — rejects label/annotation keys using reserved prefixes (`kubernetes.io/`, `k8s.io/`, `cluster.x-k8s.io/`, `5spot.io/`) before merging into CAPI Machine resources
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
