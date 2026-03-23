# Changelog

All notable changes to the 5-Spot project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

## [2025-12-12 15:45] - Remove Kubernetes Version Field from MachineSpec

**Author:** Erick Bourgeois

### Removed
- `src/crd.rs`: **BREAKING** - Removed `version` field from MachineSpec
  - Kubernetes version is a cluster-level concern, not machine-level
  - Version is defined by bootstrap/infrastructure refs (KubeadmConfigTemplate, etc.)
  - Aligns with CAPI conventions where Machines inherit version from templates

- `src/reconcilers/helpers.rs`: Removed version from Machine creation logic
  - No longer passes version to CAPI Machine resource
  - Version is determined by the bootstrap configuration

- `examples/*.yaml`: Removed version field from all examples
- Test files: Updated all MachineSpec initializations

### Why
The Kubernetes version is **not** a property of individual machines in CAPI. It's defined at:
1. Cluster level (Cluster resource)
2. ControlPlane level (KubeadmControlPlane)
3. Bootstrap template level (KubeadmConfigTemplate)

Having `version` in MachineSpec created a conceptual mismatch with CAPI architecture and could lead to version conflicts. Machines should inherit version information from their bootstrap and infrastructure references.

### Impact
- [x] Breaking change: CRD schema updated (version field removed)
- [x] Users should specify K8s version in their bootstrap templates, not in ScheduledMachine
- [x] Aligns with standard CAPI patterns

---

## [2025-12-12 15:30] - Code Quality Improvements and Configuration Enhancements

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Added configurable Kubernetes version field to MachineSpec
  - **Added** `version` field with default "v1.28.0"
  - Allows users to specify K8s version per machine instead of hardcoding

- `src/reconcilers/helpers.rs`: Code quality and maintainability improvements
  - **Removed** `#[allow(dead_code)]` from `resolve_file_contents()` and `ResolvedFile` (now actively used)
  - **Extracted** hardcoded CAPI strings to global constants for consistency
  - Uses `CAPI_GROUP`, `CAPI_MACHINE_API_VERSION_FULL`, `CAPI_CLUSTER_NAME_LABEL`, etc.

- `src/constants.rs`: Added CAPI-specific constants
  - `CAPI_GROUP`: "cluster.x-k8s.io"
  - `CAPI_MACHINE_API_VERSION`: "v1beta1"
  - `CAPI_MACHINE_API_VERSION_FULL`: "cluster.x-k8s.io/v1beta1"
  - `CAPI_CLUSTER_NAME_LABEL`: "cluster.x-k8s.io/cluster-name"
  - `CAPI_RESOURCE_MACHINES`: "machines"
  - `API_VERSION_FULL`: "capi.5spot.io/v1alpha1"

- `src/main.rs`: Implemented actual Prometheus metrics
  - **Replaced** stub metrics endpoint with proper `prometheus::gather()` integration
  - Returns all registered Prometheus metrics in standard text format
  - Added error handling for metric encoding failures

- `examples/*.yaml`: Updated examples with version field
  - `scheduledmachine-basic.yaml`: version: v1.28.0
  - `scheduledmachine-weekend.yaml`: version: v1.29.0

- All test files: Updated MachineSpec initializations to include version field

### Why
Addresses critical TODOs from project guidelines:
1. **Global Constants**: Eliminates magic strings/numbers per "no magic numbers" rule
2. **Configuration**: Makes K8s version user-configurable instead of hardcoded
3. **Metrics**: Provides proper observability via Prometheus
4. **Code Cleanliness**: Removes dead code annotations from actively used functions

### Impact
- [ ] Breaking change: CRD schema updated (version field added with default)
- [ ] Users can now specify Kubernetes version per machine
- [ ] Better code maintainability with single source of truth for strings
- [ ] Prometheus metrics now available at /metrics endpoint

---

## [2025-12-10 08:00] - Complete CAPI Machine Integration

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Implemented full CAPI Machine lifecycle integration
  - **Added** `validate_references()`: Validates bootstrap and infrastructure refs exist before Machine creation
  - **Implemented** `add_machine_to_cluster()`: Creates cluster.x-k8s.io/v1beta1 Machine resources with:
    - File content resolution from ConfigMaps/Secrets
    - Bash script generation for userData field
    - Owner references linking Machine to ScheduledMachine
    - Cluster labels for CAPI compliance
  - **Implemented** `remove_machine_from_cluster()`: Deletes CAPI Machine resources with 404 handling
  - Uses kube::core::DynamicObject with kube::discovery::ApiResource for dynamic CAPI resource access

- `src/reconcilers/scheduled_machine.rs`: Updated phase handlers to use CAPI functions
  - **Modified** `handle_pending_phase()`: Calls validate_references before add_machine_to_cluster
  - **Modified** `handle_shutting_down_phase()`: Calls remove_machine_from_cluster after grace period
  - Added error handling and status condition updates for CAPI operations

### Why
Completes the CAPI integration after schema changes in previous commit. The operator can now:
1. Validate that bootstrap and infrastructure references exist before creating Machines
2. Create real cluster.x-k8s.io/v1beta1 Machine resources in Kubernetes
3. Provision files using userData bash scripts generated from ConfigMap/Secret content
4. Delete Machines when scheduled window ends or resource is removed
5. Maintain proper ownership and labeling for CAPI compliance

### Impact
- [ ] No breaking changes to CRD schema
- [ ] Runtime behavior change: operator now creates/deletes actual CAPI Machine resources
- [ ] Requires CAPI (cluster-api) to be installed in the cluster
- [ ] Bootstrap and infrastructure providers must be available

---

## [2025-12-10 06:20] - Replace clusterDeploymentRef with clusterName

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: **BREAKING** - Replaced `cluster_deployment_ref` with `cluster_name` (String)
  - Removes vendor-specific `ClusterDeploymentRef` type (Mirantis/k0smotron/k0rdent specific)
  - Makes CRD agnostic to CAPI cluster management approach
  - `cluster_name` is now required by bootstrap and infrastructure refs

- `src/constants.rs`: Removed `KIND_CLUSTER_DEPLOYMENT` constant (no longer needed)

- `examples/*.yaml`: Updated all examples to use `clusterName` instead of `clusterDeploymentRef`

- `docs/reference/api.md`: Updated documentation to reflect new field

### Why
The `ClusterDeploymentRef` was specific to Mirantis/k0smotron/k0rdent and not a standard CAPI concept.
Using a simple `clusterName` string makes the CRD vendor-agnostic and aligns with standard CAPI practices
where the cluster name is used by bootstrap and infrastructure providers.

### Impact
- [x] **Breaking change** - Requires updating all existing ScheduledMachine manifests
- [x] Config change only

---

## [2025-12-09 19:15] - Convert to CAPI-Based Machine Scheduling

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: **BREAKING** - Converted from k0smotron to CAPI (Cluster API) based architecture
  - Changed API group from `5spot.eribourg.dev` to `capi.5spot.io`
  - Added `bootstrap_ref` and `infrastructure_ref` for CAPI Machine creation
  - Made `files` field non-optional in `MachineSpec`
  - Renamed `FileContentFrom` types to `ContentSource` and `KeySelector` for consistency
  - Added `ObjectReference` type for generic Kubernetes object references
  - Changed status structure to use string phases instead of enum
  - Added `next_activation` and `next_cleanup` fields to status
  - Changed `machine_ref` to use `ObjectReference` instead of custom `MachineRef` type

- `src/constants.rs`: Updated all constants for CAPI architecture
  - Changed API group constants to `capi.5spot.io`
  - Updated finalizer name to use new API group
  - Added CAPI Machine phase constants (Pending, Active, ShuttingDown, Inactive, Disabled, Terminated, Error)
  - Added CAPI API version constants (`cluster.x-k8s.io/v1beta1`)
  - Added new condition types: `ReferencesValid`
  - Added new condition reasons: `ReferencesInvalid`, `FileResolutionFailed`, `ScheduleDisabled`
  - Renamed `K0SMOTRON_*` constants to `CAPI_*`
  - Added ConfigMap and Secret kind constants

- `src/reconcilers/helpers.rs`: Added file content resolution functionality
  - New `resolve_file_contents()` function to fetch content from ConfigMap/Secret references
  - New `ResolvedFile` struct to represent files with resolved content
  - Validates file paths (must be absolute) and permissions format (4-digit octal)
  - Handles base64 decoding for Secret data

- `src/reconcilers/scheduled_machine.rs`: Updated error types for CAPI
  - Renamed `K0smotronError` to `CapiError`
  - Added `FileResolutionError` for file content resolution failures
  - Added `ReferenceValidationError` for bootstrap/infrastructure ref validation

### Why
Complete architectural shift from k0smotron-specific machine management to standard Cluster API (CAPI) machine scheduling. This enables:
- Standard CAPI Machine lifecycle management
- Integration with any CAPI infrastructure provider
- File provisioning via ConfigMap/Secret content resolution
- ClusterDeployment modification for machine references
- Priority-based machine scheduling
- Time-based scheduling with graceful shutdown

### Impact
- [x] **Breaking change** - Incompatible with existing CRDs and resources
- [x] Requires cluster rollout - New CRD must be deployed
- [x] Config change only - No backward compatibility
- [ ] Documentation only

### Migration Required
**WARNING: This is a breaking change that requires full migration:**

1. **Backup all existing ScheduledMachine resources**
2. **Delete old CRD**: `kubectl delete crd scheduledmachines.5spot.eribourg.dev`
3. **Deploy new CRD**: `kubectl apply -f deploy/crds/scheduledmachine.yaml`
4. **Update all ScheduledMachine manifests** to new schema:
   - Change `apiVersion` from `5spot.eribourg.dev/v1alpha1` to `capi.5spot.io/v1alpha1`
   - Add `bootstrapRef` and `infrastructureRef` fields
   - Update `files` structure (now required, not optional)
   - Ensure all file paths are absolute and start with `/`
5. **Recreate all resources** with new manifests

### Status
✅ **COMPILATION COMPLETE** - All code compiles and tests pass. CAPI integration pending:
- ✅ CRD schema updated and compiles
- ✅ Constants updated for CAPI
- ✅ File content resolution implemented (ConfigMap/Secret)
- ✅ Error types updated
- ✅ Reconciler rewrite complete (phase-based state machine)
- ✅ Main.rs updated and compiles
- ✅ All Rust code compiles without warnings (`cargo clippy` passes with strict flags)
- ✅ All unit tests pass (38 tests)
- ✅ Tests updated with new schema (bootstrap_ref, infrastructure_ref)
- ✅ CRD YAML regenerated: `deploy/crds/scheduledmachine.yaml`
- ✅ API documentation regenerated: `docs/reference/api.md`
- ✅ Examples updated with new schema and validated
- ⏳ Reference validation logic (PLACEHOLDER - needs CAPI implementation)
- ⏳ CAPI Machine creation logic (PLACEHOLDER - needs CAPI API calls)
- ⏳ ClusterDeployment modification logic (PLACEHOLDER - needs implementation)
- ⏳ Cleanup and shutdown logic (PLACEHOLDER - needs CAPI deletion)

**⚠️ IMPORTANT**: Code compiles and tests pass, but CAPI integration is incomplete. The reconciler has placeholders marked with `TODO:` and `#[allow(dead_code)]` comments. Machine creation and deletion will not function until CAPI API calls are implemented.

### Code Quality
- All clippy warnings fixed (doc comments, must_use attributes, wildcard imports, casting)
- Early return/guard clause pattern applied throughout
- Magic numbers eliminated (all numeric literals defined as constants)
- Unit tests updated and passing for all modules
- Documentation updated with proper formatting

### Next Steps
1. Implement reference validation (bootstrapRef, infrastructureRef, clusterDeploymentRef exist in cluster)
2. Implement CAPI Machine creation with resolved file contents in userData
3. Implement ClusterDeployment patching to add machine references
4. Implement cleanup logic with graceful shutdown and CAPI Machine deletion
5. Add timeout and retry logic for Kubernetes API calls
6. Create migration guide documentation
7. Update quickstart guide with CAPI-specific instructions
8. Update examples in `/examples/` directory
9. Regenerate API documentation: `cargo run --bin crddoc > docs/reference/api.md`
10. Run full test suite: `cargo test`
11. Validate all changes with `cargo fmt`, `cargo clippy`, and `cargo audit`

---

## [2025-12-09 18:45] - Add Early Return / Guard Clause Coding Style Guidelines

**Author:** Erick Bourgeois

### Changed
- `.github/copilot-instructions.md`: Added comprehensive "Early Return / Guard Clause Pattern" section to Rust Style Guidelines

### Why
To establish and document the preferred coding style for handling control flow in the 5spot codebase. The early return pattern reduces nesting, improves readability, and makes code easier to test and maintain.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

### Details
The new section includes:
- Key principles of early return/guard clause pattern
- Benefits (reduced nesting, clearer code flow, easier testing)
- Comprehensive Rust code examples (good vs. bad)
- Guidance on when to use and when NOT to use early returns
- Integration with Rust's Result and ? operator patterns
