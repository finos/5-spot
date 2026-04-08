# Changelog

All notable changes to the 5spot project will be documented in this file.

The format is based on the regulated environment requirements:
- **Author attribution is MANDATORY** for all entries
- Changes are logged in reverse chronological order
- Each entry must include impact assessment

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
