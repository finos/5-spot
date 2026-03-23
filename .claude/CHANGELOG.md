# Changelog

All notable changes to the 5spot project will be documented in this file.

The format is based on the regulated environment requirements:
- **Author attribution is MANDATORY** for all entries
- Changes are logged in reverse chronological order
- Each entry must include impact assessment

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
