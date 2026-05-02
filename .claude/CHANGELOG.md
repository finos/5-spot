# Changelog

All notable changes to the 5spot project will be documented in this file.

The format is based on the regulated environment requirements:
- **Author attribution is MANDATORY** for all entries
- Changes are logged in reverse chronological order
- Each entry must include impact assessment

---

## [2026-05-02 12:00] - warp 0.4.2 migration (PR #52 follow-up)

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: pin `warp = { version = "0.4", default-features = false, features = ["server"] }`.
  warp 0.4 made every previously-default feature opt-in; we only need
  `server` (used by `src/main.rs::run_metrics_server` and
  `src/health.rs::start_health_server`). `default-features = false`
  also drops `multipart`, `websocket`, and `test`, which we don't
  use, shrinking the supply-chain surface visible to cargo-deny /
  Grype / auto-VEX.
- `src/health.rs`: every closure body that returned `&'static str`
  now returns `String` (warp 0.4's `Reply` impl on `&str` is gated
  to `&'static str` only and the if/else inference would otherwise
  pick a non-'static lifetime). Final filter chain calls `.boxed()`
  to type-erase into `BoxedFilter` — without it the filter type
  carries a higher-ranked `AsRef<str>` bound from the new generic
  `path::path<P: AsRef<str>>` signature, which propagates through
  `tokio::spawn` and fails `Send + 'static`.
- `src/main.rs`: `warp::serve(metrics.boxed())` for the same
  HRTB / Send reason as health.rs.
- `Cargo.lock`: regenerated (warp 0.3.7 → 0.4.2; drops the legacy
  hyper 0.14 / http 0.2 / h2 0.3 / base64 0.21 / encoding_rs /
  displaydoc duplicate-stack now that warp pulls hyper 1).

### Why
PR #52 (Dependabot) bumped warp 0.3 → 0.4; the upstream release is
breaking (server/test feature-gated, hyper 1 underneath, narrower
Reply bounds, generic `path()`). The bump is the unfinished tail of
the auto-VEX Phase 3 work in `5spot-automated-vex-generation.md` —
warp 0.3 sits on hyper 0.14 which is end-of-life upstream and visible
to the supply-chain scanners tracked in
`5spot-code-scanning-remediation.md`.

### Impact
- [ ] Breaking change (no API surface change; internal-only)
- [x] Requires cluster rollout (new binary)
- [ ] Config change only
- [ ] Documentation only

### Verification
- `cargo build` clean.
- `cargo test`: 397 passed, 0 failed.
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.

---

## [2026-04-26 00:30] - Phase 4 of security audit: reclaim-agent host-identity verification

**Author:** Erick Bourgeois

### Changed
- `src/reclaim_agent.rs`: new pure helpers `read_host_machine_id(path)`
  and `compare_machine_ids(node, node_name, expected_host_id)` plus a
  `HostIdentityError` enum with four variants (`ReadFailed`, `Empty`,
  `Mismatch`, `NodeMachineIdMissing`). Both helpers are I/O-light and
  unit-testable without a kube client.
- `src/bin/reclaim_agent.rs`: read `/etc/machine-id` once at startup
  via `read_host_machine_id`; cache as `Option<String>`. Before each
  `annotate_node` PATCH, fetch the target Node and call
  `compare_machine_ids` against the cached host id. On mismatch the
  agent logs `error!` and bails (no PATCH). New `--machine-id-path`
  (env `MACHINE_ID_PATH`, default `/etc/machine-id`) and
  `--skip-host-id-check` (env `SKIP_HOST_ID_CHECK`, default false /
  strict) CLI flags.
- `deploy/node-agent/daemonset.yaml`:
  - Single-file read-only mount of `/etc/machine-id` →
    `/host/etc/machine-id`. `hostPath.type: File` so kubelet fails to
    schedule the pod if the host file is missing (fail-fast over
    silent skip-check).
  - `MACHINE_ID_PATH=/host/etc/machine-id` env var to point the agent
    at the mounted location.
  - Commented `SKIP_HOST_ID_CHECK` env var documenting the bypass for
    operators who deploy in environments without `/etc/machine-id`.
- `src/reclaim_agent_tests.rs`: 9 new tests covering both helpers —
  `read_host_machine_id` happy path / missing file / empty file /
  whitespace-only file; `compare_machine_ids` match / mismatch (asserts
  both ids appear in the error message for forensics) / no status /
  empty machine-id / whitespace-only machine-id.
- `docs/src/concepts/emergency-reclaim.md`: new "Host-identity
  verification" section documenting the contract, the two modes
  (strict default vs `--skip-host-id-check` opt-out), the mount shape
  in the DaemonSet, and what the check does NOT defend against
  (TPM-grade attestation is out of scope).

### Why
Phase 4 of the 2026-04-25 security audit roadmap. Without the
cross-check, an attacker with `update daemonsets` in `5spot-system`
could override the agent's `NODE_NAME` env var (downward API value →
hard-coded victim) and cause the agent to trigger emergency-reclaim on
a Node it has no business touching. The exploit precondition is
cluster-admin-equivalent in most clusters, so this is filed Low
severity / defence-in-depth, but the binding cost is small and removes
the impersonation primitive entirely.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (DaemonSet manifest now mounts
      `/etc/machine-id`; existing deployments need `kubectl apply` of
      the new manifest. Hosts that lack `/etc/machine-id` will fail
      pod scheduling — operators must either populate the host file
      or set `SKIP_HOST_ID_CHECK=true` for those nodes.)
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-25 22:00] - Phase 3 of security audit: route Node→SM via canonical CAPI Machine

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: new
  `node_to_scheduled_machines_via_machine(node, machines)` mapper. Routes
  Node events to owning ScheduledMachines via the canonical CAPI Machine
  ownership chain — `Machine.status.nodeRef.name` (written only by the
  CAPI controller) plus the controller-stamped
  `LABEL_SCHEDULED_MACHINE` label on the Machine. Both legs are
  write-controlled by trusted controllers, not by the tenant.
- `src/reconcilers/helpers.rs`: `node_to_scheduled_machines` (the
  pre-existing mapper that trusted `SM.status.nodeRef.name`) marked
  `#[deprecated(since = "0.1.2")]` with a note pointing at the
  replacement. Kept exported and tested for one release so any external
  caller (none expected) gets a compile-time pointer.
- `src/reconcilers/mod.rs`: re-export the new symbol; gate the legacy
  re-export with `#[allow(deprecated)]`.
- `src/main.rs`: wire a standalone `kube::runtime::reflector` for CAPI
  Machines (filtered by `LABEL_SCHEDULED_MACHINE`). Spawn its watcher in
  the background; pass the reader handle into the Node watch closure.
  Switch the Node mapper from `node_to_scheduled_machines` to
  `node_to_scheduled_machines_via_machine`. The
  `Controller::watches_with` Machine watch keeps its own internal
  reflector for the existing Machine→SM route — kube-rs does not expose
  that internal store, so the secondary watch is intentional. Doubled
  watch traffic is minor (same kind, same label selector).
- `src/reconcilers/helpers_tests.rs`: 9 new tests covering the
  canonical-Machine mapper:
  - happy-path single-Machine match returns owning SM
  - Node with no owning Machine → NOT enqueued (closes spoof window)
  - Machine for a different node → NOT enqueued (canonical wins over
    spoofed SM status)
  - unlabelled / namespace-less / no-nodeRef Machine → ignored
  - Node without metadata.name → ignored
  - whitespace-only label value → ignored (mirrors
    `machine_to_scheduled_machine`'s defensive trim)
  - multiple Machines for same Node → both owning SMs enqueued
- Existing `test_node_to_sms_*` tests for the deprecated mapper kept
  with per-test `#[allow(deprecated)]` annotations so the legacy
  surface stays covered until removal.

### Why
Phase 3 of the 2026-04-25 security audit roadmap. The pre-existing
`node_to_scheduled_machines` mapper filtered SMs by
`status.nodeRef.name`, which is writable by anyone with `patch
scheduledmachines/status` on the resource. A tenant could write a
fast-changing node name (e.g. a busy worker that's frequently
updating) into status to amplify how many SM reconciliations they
caused per Node update — a CPU DoS amplifier. The drain target itself
was never affected (the reconciler always reads canonical
`Machine.status.nodeRef` via `get_node_from_machine`), so this was
verified as Low severity, but the routing change closes the
amplification regardless of how the reconciler evolves.

### Impact
- [ ] Breaking change (deprecated symbol still exported)
- [ ] Requires cluster rollout (no schema or RBAC changes)
- [x] Config change only (in-binary; one extra Machine watch on the
      same selector that already exists)
- [ ] Documentation only

---

## [2026-04-25 18:00] - Phase 2 of security audit: finalizer cleanup force-remove on PDB stall

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs` (`handle_deletion`): refactor the timeout
  branch from `tokio::time::timeout(...).await.map_err(...)??` (which
  silently propagated the `TimeoutError` and **prevented** the finalizer
  from being removed) into an explicit 3-arm match over a new
  `CleanupOutcome` enum. The default mode now force-removes the
  finalizer on timeout so namespace deletion is unblocked, surfaces a
  `FinalizerCleanupTimedOut` Warning event on the SM, and increments
  `fivespot_finalizer_cleanup_timeouts_total`. A real cleanup error (as
  opposed to a timeout) still propagates and is retried — the finalizer
  is kept in place so a transient API failure does not orphan
  resources.
- `src/reconcilers/helpers.rs`: new pure helpers `run_cleanup_with_timeout`
  (returns `CleanupOutcome::{Completed, Failed(err), TimedOut}`) and
  `build_finalizer_timeout_event`. Both are unit-testable without the
  kube API; the timeout test runs in microseconds via
  `#[tokio::test(start_paused = true)]`.
- `src/reconcilers/scheduled_machine.rs` (`Context`): new
  `force_finalizer_on_timeout: bool` field defaulting to `true`, plus a
  `with_force_finalizer_on_timeout(bool)` builder for `main.rs` to
  override from CLI/env. Strict-cleanup mode (`false`) keeps the
  finalizer and propagates `TimeoutError` so reconciliation retries —
  only safe when an external sweep garbage-collects stuck SMs.
- `src/main.rs`: new `--force-finalizer-on-timeout` flag (env
  `FORCE_FINALIZER_ON_TIMEOUT`, default `true`) wired through `Context`.
- `src/metrics.rs`: new `FINALIZER_CLEANUP_TIMEOUTS_TOTAL` counter +
  `record_finalizer_cleanup_timeout()` helper + label-less
  `fallback_counter` constructor. Documented as an operator-alert
  signal for orphan-resource detection.
- `deploy/deployment/deployment.yaml`: added `FORCE_FINALIZER_ON_TIMEOUT`
  env var with `value: "false"` (strict-cleanup mode) per operational
  preference; the binary default remains `true`. Block-comment in the
  manifest documents the trade-off and references the troubleshooting
  runbook.
- `docs/src/operations/troubleshooting.md`: new "Orphan resources after
  finalizer timeout" section with the runbook (find orphan Machines via
  ownerRef walk, inspect bootstrap/infra refs, cascading delete via
  Machine, prevention guidance).
- `src/reconcilers/helpers_tests.rs` / `src/metrics_tests.rs`: 10 new
  tests covering `run_cleanup_with_timeout` (Completed/Failed/TimedOut),
  `build_finalizer_timeout_event` (severity, reason, action,
  note-content invariants), `Context.force_finalizer_on_timeout`
  defaulting, and the metric-increments-on-call contract.

### Why
Finding F-007 from the 2026-04-25 adversarial security audit (filed in
`~/dev/roadmaps/5spot-security-audit-2026-04-25.md`, Phase 2). A
namespace-tenant with `create pods` + `create poddisruptionbudgets` —
common tenant grants — could plant a `minAvailable: 999` PDB on a
workload they own, schedule a machine on the same node, then delete
the SM. Drain blocks indefinitely on the impossible eviction; with the
old code the finalizer was never removed, the SM was stuck with
`deletionTimestamp`, and namespace deletion stalled. The fix unblocks
namespace deletion in the default mode while preserving
strict-cleanup-or-stall semantics for operators with an external sweep.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (new env var; behaviour change for clusters
      that hit the timeout path — they'll now succeed at deletion
      where they previously stalled)
- [ ] Documentation only

---

## [2026-04-25 06:30] - `gitleaks-install` now wires the local pre-commit hook

**Author:** Erick Bourgeois

### Changed
- `Makefile` (`gitleaks-install`): after installing the binary, invokes
  `install-git-hooks` so a single `make gitleaks-install` leaves the
  developer with both the tool AND the local secret-scanning hook in
  place. Previously the two targets were independent and the hook had
  to be set up via a second command.
- `Makefile` (`install-git-hooks`): no longer depends on
  `gitleaks-install` (would create a circular dependency now that
  `gitleaks-install` calls it). The hook only invokes gitleaks at
  *commit* time, so install-time independence is correct.
- `Makefile` (`install-git-hooks`): now idempotent and non-destructive.
  A `5spot-managed-gitleaks-hook` sentinel embedded in the hook lets
  re-runs detect "already installed" and leave the file alone. If a
  custom pre-commit hook is found (no sentinel), it is preserved at
  `.git/hooks/pre-commit.bak` rather than overwritten silently.
- `Makefile` (hook content): the generated hook now checks for
  gitleaks on PATH and emits a clear "run make gitleaks-install"
  message if it is missing, instead of failing with a cryptic
  "command not found".

### Why
Onboarding friction: developers ran `make gitleaks-install` expecting
the secret scan to fire on commit, then committed secrets because the
hook had never been wired. Banking-environment requirement (no
secrets in commits) was therefore enforced only by CI — too late.
Wiring hook setup into the install target makes the local guard the
default for every dev, while idempotency + backup keeps re-runs safe.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (developer tooling)
- [ ] Documentation only

---

## [2026-04-24 15:00] - Security audit remediation: grace period, retry-count poisoning, input bounds

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs` (`check_grace_period_elapsed`): guard
  against negative `elapsed` durations produced by clock-skew (NTP step,
  VM freeze-thaw, backward wall-clock adjustment). The prior comparison
  `elapsed.num_seconds() >= timeout.as_secs() as i64` silently returned
  `false` when `elapsed` was negative, potentially bypassing the
  graceful-drain window entirely. Negative elapsed is now treated as
  "timeout reached" so the reconciler forces progress rather than
  stalling on a misbehaving clock.
- `src/reconcilers/helpers.rs` (`error_policy`): replace
  `ctx.retry_counts.lock().unwrap_or_else(PoisonError::into_inner)`
  with an explicit `match` that aborts the error-policy pass and
  requeues after `ERROR_REQUEUE_SECS` on poison. The prior pattern
  silently recovered a potentially-inconsistent map and continued
  mutating it, corrupting the exponential-backoff schedule for every
  subsequent error.
- `src/reconcilers/helpers.rs`: new `validate_cluster_name()` and
  `validate_kill_if_commands()` with bounds from `src/constants.rs`.
  Called early in `reconcile_inner` (defence-in-depth against clusters
  that have not enabled the ValidatingAdmissionPolicy) and re-called in
  `add_machine_to_cluster` before touching CAPI.
- `src/constants.rs`: new `MAX_CLUSTER_NAME_LEN = 63` (RFC-1123 DNS
  label — the effective CAPI cluster-name cap), plus
  `MAX_KILL_IF_COMMANDS_COUNT = 100` and `MAX_KILL_IF_COMMAND_LEN = 256`.
- `src/crd.rs`: attach bounded JSON-schema generators
  (`cluster_name_schema`, `kill_if_commands_schema`) so the generated
  CRD enforces the same limits at the kube API level.
- `src/metrics.rs`: replace `unreachable!()` in the metric-registration
  fallback constructors with `panic!()` carrying
  `FALLBACK_METRIC_BUG_MSG` — a pointed diagnostic that identifies the
  offending metric name instead of a generic "entered unreachable code"
  message.
- `deploy/admission/validatingadmissionpolicy.yaml`: add CEL expressions
  1b/1c/1d/1e mirroring the new runtime validators — `clusterName`
  length/charset, `killIfCommands` count/per-entry-length — so invalid
  specs are rejected at admission instead of only at reconciliation
  time.
- `src/reconcilers/helpers_tests.rs`: 20 new tests covering the grace-
  period clock-skew fix, `validate_cluster_name` (happy path, empty,
  over-cap, control chars, non-ASCII), and `validate_kill_if_commands`
  (None/empty/typical/cap-boundary/over-cap/empty-entry).
- `deploy/crds/scheduledmachine.yaml`: regenerated via `cargo run --bin
  crdgen` to pick up the new schema constraints.
- `docs/src/reference/api.md`: regenerated via `make crddoc`.

### Why
Findings from the deep security audit of the codebase (see
conversation transcript 2026-04-24). Three issues were actionable: a
clock-skew bypass of the graceful-drain window (critical; violates the
contract of graceful shutdown under SOX §404 / NIST AC-3), an
unbounded `killIfCommands` that could balloon Prometheus label
cardinality and pin reclaim-agent CPU (high; DoS vector), and
silent recovery from a poisoned retry-count mutex (medium; corrupts
the exponential-backoff schedule). Cluster-name was also unbounded;
the 63-char cap matches the effective CAPI constraint (cluster names
flow into DNS labels downstream). Metric `unreachable!()` is a
cosmetic bug-diagnostic improvement.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (new CRD schema constraints; existing
      CRs that violate them will be rejected on next `kubectl apply`)
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-23 08:00] - Fix vexctl-install Linux path (wrong asset filename)

**Author:** Erick Bourgeois

### Changed
- `Makefile` (`vexctl-install` target): rewrote the Linux branch. The
  original was modelled on the `gitleaks-install` target, which
  assumes a `name_version_os_arch.tar.gz` tarball convention with a
  versioned `name_version_checksums.txt`. vexctl uses neither
  convention: releases ship **raw binaries** named
  `vexctl-<os>-<arch>` (no tarball, no version in the filename), and
  the checksums file is `vexctl_checksums.txt` (version-less). The
  branch now downloads the raw binary, downloads the correct
  checksums file, greps with a two-space-prefix + end-of-line
  anchor (standard `sha256sum` format), verifies, and `install`s
  directly to `/usr/local/bin/vexctl`. No tarball extraction step.
- Help string updated from "GitHub release tarball on Linux" to
  "pinned raw binary + sha256 on Linux" to match reality.

### Why
The first CI run that exercised the `vex-validate` job on a
GitHub-hosted Linux runner failed end-to-end:

```
Downloading vexctl_0.4.1_linux_amd64.tar.gz...
curl: (22) The requested URL returned error: 404
curl: (22) The requested URL returned error: 404
grep: vexctl_checksums.txt: No such file or directory
sha256sum: vexctl_checksum_file.txt: no properly formatted checksum lines found
tar (child): /tmp/vexctl_0.4.1_linux_amd64.tar.gz: Cannot open: No such file or directory
install: cannot stat '/tmp/vexctl': No such file or directory
✓ vexctl installed: /bin/sh: 41: vexctl: not found
make: *** [Makefile:498: vex-validate] Error 127
```

Root cause: I shipped the Makefile target in Phase 1 without
verifying the vexctl release asset naming — it differs from the
gitleaks convention I copied from. Checked the actual
`/releases/tags/v0.4.1` API response and confirmed the correct
asset names.

The macOS branch (`brew install vexctl`) was always correct and is
untouched; local `make vex-validate` on the maintainer's Mac
continued working throughout, which is why the bug didn't surface
until CI ran.

### Verification
- Re-downloaded `vexctl_checksums.txt` from the v0.4.1 release and
  confirmed the new `grep "  $${BINARY}$$" vexctl_checksums.txt`
  pattern matches the `vexctl-linux-amd64` line with its SHA
  (`51753968975448c521b693e91a52f7e30b7d427da668375218fa64392ffb93c5`).
- Local `make vex-validate` on macOS continues to pass — brew
  branch untouched.
- The Linux branch was eyeballed-verified against the actual asset
  URL and checksum-format; will be proven by the next CI run on
  this branch.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only — Makefile fix, no runtime or API change.
- [ ] Documentation only

---

## [2026-04-22 21:45] - Pin CodeQL languages (fallout from Phase 1 Python deletion)

**Author:** Erick Bourgeois

### Added
- `.github/workflows/codeql.yaml`: explicit CodeQL Advanced Setup
  workflow with a matrix over `rust`, `actions`, and
  `javascript-typescript`. `build-mode: none` for all three (Rust
  supports it per the CodeQL compiled-languages docs; the other two
  don't require a build). Runs on PR, push to main, and a weekly
  Sunday 07:17 UTC full re-scan. Uploads SARIF per-language category
  to Code Scanning.
- `.github/codeql/codeql-config.yml`: companion config file with
  `paths-ignore` for `target/**` and `docs/site/**` (the generic
  generated-output exclusions). No Python-adjacent paths — since
  `python` isn't in the workflow's language matrix, the analyzer
  never looks at the Poetry manifests in `docs/`, so explicit
  exclusions there would be dead weight sending a misleading "we
  still configure for Python" signal.

### Why
Phase 1 (`.claude/CHANGELOG.md` 2026-04-22 19:45) deleted the `tools/`
directory including `assemble_openvex.py` and `validate_vex.py`. After
that change, GitHub's **default setup** CodeQL run started failing on
the next push with:

```
CodeQL detected code written in GitHub Actions, Rust and
JavaScript/TypeScript, but not any written in Python. Confirm that
there is some source code for Python in the project.
```

Root cause: default setup auto-detected Python because
`docs/pyproject.toml` (a Poetry manifest for MkDocs tooling) still
exists — but no `.py` source files remain, so the Python database
finalize step errors with exit code 32.

The fix is to take over from default setup with an explicit workflow
that pins the language list, which is what this entry adds. Default
setup will be automatically superseded once any workflow calls
`github/codeql-action/init`, so no Settings-UI change is needed.

The single JS file in scope is `docs/src/javascripts/mermaid-init.js`
(MkDocs Mermaid diagram init); that's why JS/TS stays on the list.
Poetry manifests are explicitly path-ignored — they are metadata, not
source, and would produce false-empty Python databases.

### Verification
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/codeql.yaml'))"`
  and `yaml.safe_load(open('.github/codeql/codeql-config.yml'))` both
  parse clean.
- Action pins match the rest of the repo
  (`github/codeql-action@95e58e9a2cdfd71adc6e0353d5c52f41a045d225 # v4.35.2`,
  `actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2`).
- Dependabot will continue updating the codeql-action pin
  (`.github/dependabot.yml:41` already lists `github/codeql-action`
  in the allow-list).

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only — adds a workflow file and a sibling config,
  no runtime change.
- [ ] Documentation only

### Operator follow-up
If the repo's Code Scanning default setup was previously enabled via
the UI, it will automatically defer to this workflow — no toggle
needed. If a partial status appears under *Settings → Code security →
Code scanning*, it's cosmetic: the advanced-setup workflow takes
precedence.

---

## [2026-04-22 21:15] - Drop AUTO_VEX_PRESENCE gate; Security team is the verifier

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`:
  - `build-vex` job: removed the `vars.AUTO_VEX_PRESENCE == '1'`
    conditional on the auto-presence download step. The artifact is
    now downloaded unconditionally, and the merge command includes
    `auto/vex.auto-presence.json` on every build. Added an explicit
    `test -f auto/vex.auto-presence.json || exit 1` guard so a
    missing artifact fails the job (rather than silently producing
    hand-authored-only VEX).
  - Comment headers on both `auto-vex-presence` and `build-vex`
    rewritten to describe the new trust model: CI emits aggressively,
    Security team verifies downstream and counter-signs.
- `.vex/README.md`: removed the "parallel-run gate / review-only"
  language. Auto-presence statements now merge into the signed VEX on
  every build.
- `docs/src/security/vex.md`:
  - CI flow step 3: removed the `AUTO_VEX_PRESENCE=1` gate clause.
  - Automation-split section: the "Automated" bullet no longer
    mentions a feature flag.
  - New top-level "Trust model" section describing the two-signature
    workflow: CI-side Cosign attestation (keyless OIDC via GitHub)
    first, Security-team Cosign attestation (their own OIDC identity)
    second. Each sits in the Sigstore transparency log; downstream
    consumers can require both via twin
    `--certificate-identity-regexp` invocations.
- `~/dev/roadmaps/5spot-automated-vex-generation.md`:
  - Top-of-file status line updated: Phase 1 ✅, Phase 2 ✅ (always-on),
    Phase 3 ⏳ scoped.
  - New "Trust model (load-bearing)" section at the top — replaces
    the implicit assumption in the earlier draft that our side would
    be the verification gate.
  - Phase 2 "Status" line updated to reflect the gate removal.
  - Phase 3 tradeoff section simplified: "two independent call-graph
    analyses must agree" and "multi-release parallel-run" removed;
    the Security team's re-verification is the second check.
  - Rollout section rewritten: no per-phase feature flags; each
    phase activates on merge.

### Why
Policy flip requested by the user. The original Phase 2 rollout plan
had an `AUTO_VEX_PRESENCE` repo variable that gated whether
auto-generated statements flowed into the signed VEX document; the
intent was a parallel-run validation window where maintainers could
diff the auto-set against `.vex/` before authorizing it. The revised
trust model moves that validation downstream to the Security team,
which re-derives each machine-authored claim from the signed evidence
(SBOM, call-graph attestations, SLSA provenance) and counter-signs if
they agree. That makes our side's gate redundant — if we get it
wrong, the VEX ships with only one attestation instead of two, which
is a discoverable condition for any downstream consumer running a
two-signature verify gate.

The trust model extends to the not-yet-implemented Phase 3
(reachability-based auto-VEX): it also ships unconditionally when
built, with call-graph attestations providing the evidence for
Security to verify.

### Verification
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/build.yaml'))"`
  → valid YAML.
- `grep "AUTO_VEX_PRESENCE" .github/workflows/build.yaml` → no
  residual references.
- No Rust code touched; `cargo` checks unchanged from the previous
  entry.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only — specifically, removes a config knob that
  existed for ~1 hour and was never documented as user-facing.
- [x] Documentation only

---

## [2026-04-22 20:30] - Presence-based auto-VEX (roadmap Phase 2)

**Author:** Erick Bourgeois

### Added
- `src/auto_vex_presence.rs` (library module, ~180 LOC) + `src/auto_vex_presence_tests.rs`
  (20 unit tests, 100% positive/negative/exception coverage per project rule):
  - `compute_presence_vex` — pure function that diffs Grype findings against
    SBOMs and the already-triaged CVE set, emitting
    `not_affected + component_not_present` statements for CVEs whose purl
    is absent from every SBOM.
  - `build_document` — wraps statements in the OpenVEX envelope.
  - `load_triaged_from_vex_dir` — reads hand-authored `.vex/*.json` and
    collects the set of `vulnerability.name` values (permissive on
    missing dir, strict on malformed JSON).
  - Typed deserializers for Grype JSON and CycloneDX SBOM subsets —
    tolerant of unknown upstream fields.
  - Output sorted by CVE id for deterministic, diffable artifacts.
- `src/bin/auto_vex_presence.rs` — thin clap-driven CLI wrapping the
  library. Reads `--grype-json`, one or more `--sbom` files, a
  `--vex-dir`, a `--product-purl`, `--id`, optional `--author` and
  `--timestamp`, writes an OpenVEX JSON to `--output` or stdout.
- `Cargo.toml`: `[[bin]] auto-vex-presence` entry.
- `Makefile`: `vex-auto-presence` target (defaults `GRYPE_JSON=grype.json`
  and `SBOM_FILES=target/release/*.cdx.json docker-sbom-*.json`; both
  overridable). Added to `.PHONY`.
- `.github/workflows/build.yaml`: two new jobs gated on
  `github.event_name != 'pull_request'`:
  - `grype-triage` — runs Grype against each image variant without VEX
    suppression, uploads per-variant JSON (pinned GRYPE_VERSION 0.87.0
    matching the existing `grype` job).
  - `auto-vex-presence` — builds the new bin, downloads both triage
    scans + Docker SBOMs, runs the bin once per variant, merges the
    per-variant outputs via `vexctl merge`, uploads
    `vex-auto-presence` artifact.

### Changed
- `.github/workflows/build.yaml`:
  - Docker SBOM generation (`Generate Docker SBOM for ${{ matrix.variant.name }}`):
    gate loosened from `if: github.event_name == 'release'` to
    `if: github.event_name != 'pull_request'` so push-to-main also
    produces SBOMs for auto-vex-presence to consume.
  - `build-vex` job: `needs` extended to include `auto-vex-presence`.
    Added a feature-flagged download step (`if: vars.AUTO_VEX_PRESENCE == '1'`)
    and conditional merge inclusion. When the repo variable is
    `AUTO_VEX_PRESENCE=1`, `vex.auto-presence.json` is merged alongside
    `.vex/*.json`; otherwise hand-authored only. Artifact is produced
    regardless — only the merge is gated.
- `src/lib.rs`: re-export `pub mod auto_vex_presence`.
- `docs/src/security/vex.md`: CI flow section renumbered to 6 steps;
  added step 2 for auto-presence. Rewrote "Why we did not auto-generate
  statements" section as "What we automate, and what stays human" — the
  policy now distinguishes `component_not_present` (mechanical,
  automatable) from every other triage decision (human-authored).
- `.vex/README.md`: added "Automated statements (roadmap Phase 2)"
  section explaining the review-artifact vs flag-gated-merge split.

### Why
Phase 2 of the automated VEX generation roadmap
(`~/dev/roadmaps/5spot-automated-vex-generation.md`). Most of the
hand-authored statements under `.vex/` exist because Grype flags
CVEs on libc / glibc / zlib code paths that the Rust binary never
reaches — i.e. the vulnerable component is present in the image
filesystem but never invoked. A subset of those (plus most
base-image CVEs going forward) can be suppressed mechanically by
observing that the flagged package's purl is not in the SBOM at
all: the SBOM is authoritative for what's in the product, so
"component_not_present" is the one OpenVEX justification with a
purely verifiable definition. Automating that specific case shrinks
the `.vex/` maintenance queue without compromising the trust model
for any other justification.

The implementation is strictly SBOM-driven — no reachability analysis,
no source-code inspection. Reachability is Phase 3.

### Rollout / feature-flag
Per the roadmap rollout plan, the auto-presence artifact is
produced and uploaded on every push + release, but it is **only
merged into the signed VEX document when `vars.AUTO_VEX_PRESENCE=1`
is set** at the repository or organization level. This gives
maintainers at least one release of parallel-run validation: the
artifact is visible for review, but consumers see only the
hand-authored VEX until the flag is flipped. This matches the
roadmap's explicit guidance ("Run in parallel with hand-authored VEX
for one release; diff the auto-set against .vex/ and confirm no
surprises before flipping it on").

### Verification
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo test --lib`: 349 tests pass (329 pre-existing + 20 new for
  `auto_vex_presence`).
- Smoke test: ran the bin against a synthetic Grype JSON (3 matches),
  synthetic SBOM (covering 1 of the 3 purls), and the current `.vex/`
  directory (covering 1 of the 3 CVEs). The bin emitted exactly the
  one statement for the remaining CVE whose purl was absent, and
  `vexctl merge` of the auto-presence output + `.vex/*.json`
  produced a 16-statement document with the auto statement correctly
  included.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (no — CI only)
- [ ] Config change only
- [x] Documentation only

*(Rollout note: when maintainers want to flip `AUTO_VEX_PRESENCE=1`,
that's a repo-variable change in GitHub settings, no code deploy.)*

---

## [2026-04-22 19:45] - Replace Python VEX tooling with vexctl (roadmap Phase 1)

**Author:** Erick Bourgeois

### Removed
- `tools/` — entire directory deleted. Removed `assemble_openvex.py`,
  `validate_vex.py`, `validate-vex.sh`, `tests/assemble-openvex-tests.sh`,
  `tests/validate-vex-tests.sh`, and all 18 fixture directories under
  `tests/fixtures/`. ~400 LOC of bespoke Python + shell tests replaced
  by upstream `vexctl` invocations.

### Changed
- `.vex/*.toml` → `.vex/*.json` (15 files): migrated every statement
  from the bespoke TOML dialect to the native OpenVEX v0.2.0 JSON
  shape. Each file is now a single-statement OpenVEX document that
  `vexctl merge` can consume directly; no translation layer.
- `Makefile`: added `VEXCTL_VERSION ?= 0.4.1`, `vexctl-install` target
  (brew on macOS, pinned GitHub release tarball with checksum
  verification on Linux; errors on other OSes), `vex-validate`
  (parses every `.vex/*.json` via `vexctl merge` — successful parse
  is the validation), and `vex-assemble` (prints the merged document
  for local preview). `.PHONY` list updated.
- `.github/workflows/build.yaml`:
  - `validate-vex` job: dropped Python setup + custom validator calls;
    now runs `make vexctl-install` then `make vex-validate`.
  - `build-vex` job: dropped Python setup + `assemble_openvex.py`;
    now runs `vexctl merge --id <per-event-id> --author <actor>
    .vex/*.json` directly. Per-event `@id` logic (release / push /
    PR) preserved verbatim. Cosign attestation, artifact upload,
    and `attest-build-provenance` steps unchanged.
  - PR `paths:` filter: removed `tools/**` (directory no longer
    exists); `.vex/**` retained.
- `.vex/README.md`: rewritten to document native OpenVEX JSON
  authoring and `make vex-validate` / `make vex-assemble` instead of
  the old TOML schema and shell validator.
- `docs/src/security/vex.md`: CI flow section updated — step 1
  ("validate") and step 2 ("assemble") now describe `vexctl merge`
  behaviour; dropped the obsolete "cross-check with `vexctl validate`"
  step (vexctl has no `validate` subcommand). Maintainer-authoring
  section rewritten with the JSON shape in place of the TOML shape.
- `.claude/SKILL.md`: release checklist item for VEX updated from the
  old three-script check to `make vex-validate`.

### Why
Phase 1 of the automated VEX generation roadmap
(`~/dev/roadmaps/5spot-automated-vex-generation.md`). The custom
Python toolchain duplicated functionality that `vexctl` — the official
OpenVEX CLI — provides upstream. Maintaining a bespoke TOML dialect,
its validator, its assembler, and two fixture-based test suites
added ~400 LOC of surface area with no offsetting benefit once we
accept OpenVEX JSON as the authoring format. Replacing the whole
stack with `vexctl merge` (which doubles as the validator via
successful parse) removes the Python runtime from CI entirely,
eliminates a class of drift between our validator and the upstream
spec, and prepares the pipeline for Phases 2–3 (automated VEX
generation from SBOM and call-graph reachability).

The TOML-over-JSON ergonomics hit is real but small: 15 files in
`.vex/`, each ~15 lines of JSON, with no inline comments. In
exchange, future maintainers work with the same format upstream
tooling speaks and any OpenVEX-aware tool can consume the source
directory directly.

### Verification
- `make vex-validate` parses all 15 `.vex/*.json` successfully.
- `vexctl merge --id <x> --author <y> .vex/*.json` produces a
  15-statement document structurally identical to what the Python
  assembler would have produced (confirmed via key-by-key diff of
  `@context`, `@id`, `author`, `version`, and every statement's
  `products`, `status`, `justification`, `impact_statement`,
  `action_statement`, `timestamp`).

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

*(Not really "documentation only" — code deletion is substantive —
but no runtime, CRD, or API-surface change. Closest pre-existing
box.)*

---

## [2026-04-21 15:00] - Model emergency-reclaim in CALM — ADD retrofit

**Author:** Erick Bourgeois

### Added
- `docs/architecture/calm/architecture.json`: 4 new nodes — `service-workload-kubernetes-api` (workload cluster API server as a distinct trust boundary from the management cluster API), `service-reclaim-agent` (node-side DaemonSet), `data-asset-reclaim-request-annotation` (agent→controller signalling surface on the Node), `data-asset-reclaim-agent-configmap` (per-node projection in `5spot-system`).
- `docs/architecture/calm/architecture.json`: 4 new relationships — `rel-controller-workload-kube-api` (controller writes into the workload cluster: ConfigMap projection, Node label, Node annotation clear; audit-attribution control NIST AU-2/AU-10 via distinct field manager `5spot-controller-reclaim-agent`), `rel-reclaim-agent-workload-kube-api` (agent watches its ConfigMap + annotates its own Node; node-scoped-credentials control NIST AC-6), `rel-reclaim-configmap-stored-in-workload-api` (composed-of), `rel-reclaim-annotation-on-node` (composed-of).
- `docs/architecture/calm/architecture.json`: new `flow-emergency-reclaim` with 7 transitions mapping the load-bearing ordering contract (drain → Machine delete → `spec.schedule.enabled=false` → annotation clear → phase Disabled). Flow-level `ordering-idempotence` control (NIST SI-7) documents why step 5 must precede step 6 and how the `PHASE_EMERGENCY_REMOVE` match arm closes the crash window.
- Per-node controls on `service-reclaim-agent`: `least-privilege` (NIST AC-3/AC-6, evidence `deploy/node-agent/rbac.yaml`) and `container-hardening` (NIST SP 800-190 CM-6/SI-7, evidence `deploy/node-agent/daemonset.yaml` + `.trivyignore` architectural-necessity suppressions).

### Changed
- `docs/architecture/calm/architecture.json`: extended `rel-workload-cluster-contains-node` to include `service-workload-kubernetes-api` and `service-reclaim-agent` alongside the existing `network-physical-node`.
- `docs/src/architecture/{flows.md,system.md}`: regenerated by `make calm-diagrams` from the updated model (gitignored — CI regenerates).

### Why
The user just committed the ADD (Architecture Driven Development) rule: any architectural change must land in CALM before code. An audit against the two most-recent features found the **emergency-reclaim work is architecturally material** (new node-side component, new trust boundary, new cross-cluster signalling protocol, new `EmergencyRemove` phase, new RBAC scope in `5spot-system`) but was not reflected in CALM. This commit closes that gap — it is the shakedown test for the ADD rule, done now so the workflow is proven before the rule applies to future features. The separately-audited node-taints feature is not architectural under ADD (CRD-field + existing Node-watch pattern only) and is intentionally **not** retrofitted here.

## [2026-04-21 17:00] - Integration test for node-taint reconcile on a real cluster (Phase 8)

**Author:** Erick Bourgeois

### Changed
- `tests/integration_node_taints.rs`: New integration test file. Two `#[tokio::test] #[ignore]` cases exercise `five_spot::reconcilers::reconcile_node_taints` against a real kube cluster (kind). Case 1 (`apply_update_shrink_cycle`) applies two taints, shrinks to one, asserts only the removed one is gone and the kept one survives. Case 2 (`admin_conflict_is_reported_and_admin_taint_preserved`) seeds an admin-owned taint with the same `(key, effect)` as a desired one, then asserts the reconcile returns `Conflict` and leaves the admin-owned taint untouched. Skips gracefully (not fails) when no cluster is reachable, so `cargo test` stays hermetic. All test-applied taints use the `integration-test.5spot.local/` key prefix so the cleanup helper can scrub on both entry and exit without ever touching admin state. Deliberately bypasses CAPI/k0smotron entirely — we pick an already-Ready Node from the cluster rather than provisioning one, because the only piece this test needs to exercise is the Node-side patch/annotate path.
- `src/reconcilers/mod.rs`: Re-exported `reconcile_node_taints`, `ReconcileNodeTaintsInput`, `NodeTaintReconcileOutcome` from the `helpers` submodule at the `reconcilers` level so integration tests (which can only see the public API) can call them.

### Why
Phase 8 of the user-defined Node taints roadmap — real-cluster verification. The unit tests in `helpers_tests.rs` already cover the pure logic in `diff_node_taints` and drive `reconcile_node_taints` through a mocked kube service; the integration test proves the same function works end-to-end against a real API server, including server-side apply semantics, annotation round-tripping, and admin-conflict detection against live `spec.taints`. Per the user's decision earlier in this session, we deliberately mock the `status.nodeRef` side (by picking an existing Node) rather than stand up CAPI + k0smotron + RemoteMachine + SSH-reachable hardware — that's out of scope for this roadmap and the Node-patch code path is the only new behavior.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

New tests are gated behind `#[ignore]`; running `cargo test` without a cluster still shows 328/328 lib tests plus `0 passed; 2 ignored` on the integration target. Developers who want to exercise Phase 8 run `cargo test --test integration_node_taints -- --ignored --test-threads=1` against a kind cluster (single-threaded because both cases mutate the same Node). `cargo fmt` ✓ · `cargo clippy --all-targets --all-features -- -D warnings` ✓ · `cargo test` 328/328 ✓.

---

## [2026-04-21 16:00] - Docs + examples + crddoc regen for nodeTaints (Phase 7)

**Author:** Erick Bourgeois

### Changed
- `docs/src/concepts/scheduled-machine.md`: Added a `nodeTaints` row to the spec-fields table, a new "Node Taints" subsection (shape, ownership model, `NodeTainted` condition vocabulary, what-it-does-not-manage), and an `appliedNodeTaints` row in the status-fields table. Links from both directions so operators reading the status field land on the ownership explainer.
- `docs/src/operations/troubleshooting.md`: Added a new "Node Taints" section with "Taints not appearing on Node" walkthrough, one subsection per `NodeTainted` condition reason (`NoNodeYet`, `NodeNotReady`, `TaintOwnershipConflict`, `PatchFailed`) with the exact `kubectl` commands operators need and the fix for each.
- `README.md`: Added a one-line bullet in the features list referencing `spec.nodeTaints` and the ownership/drift-reconcile story.
- `src/bin/crddoc.rs`: Added `nodeTaints` to the example YAML in the generated API doc, a `#### nodeTaints` section with the full NodeTaint field schema + reserved-prefix + uniqueness rules, and a `#### appliedNodeTaints` section in Status Fields explaining the ownership-record-of-truth role.
- `docs/src/reference/api.md`: Regenerated via `cargo run --bin crddoc` (LAST step per CLAUDE.md order-of-operations) — now 231 lines, with spec and status taint fields documented.

### Why
Phase 7 of the user-defined Node taints roadmap — docs last. Operators need to find the condition vocabulary *before* they hit a production issue, and the troubleshooting entry is keyed off the condition reason so a `kubectl get scheduledmachine ... -o jsonpath='{.status.conditions[?(@.type=="NodeTainted")]}'` walk leads directly to the fix. The crddoc regen runs last per the CLAUDE.md mandate because it incorporates everything upstream.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

`cargo fmt` ✓ · `cargo clippy --all-targets --all-features -- -D warnings` ✓ · `cargo test --lib` 328/328 ✓ · `cargo run --bin crddoc` ✓ (regenerated `docs/src/reference/api.md`).

---

## [2026-04-21 15:30] - ValidatingAdmissionPolicy CEL rules for nodeTaints (Phase 6)

**Author:** Erick Bourgeois

### Changed
- `deploy/admission/validatingadmissionpolicy.yaml`: Added six validation rules (14–19) covering `spec.nodeTaints`: RFC-1123 qualified-name regex on `key`, 253-char cap on `key`, 63-char cap on `value`, reserved `5spot.finos.org/` prefix rejection, reserved kubelet prefixes (`kubernetes.io/`, `node.kubernetes.io/`, `node-role.kubernetes.io/`) rejection, and uniqueness of `(key, effect)` pairs via O(n²) `filter().size() == 1` scan. Each rule guards `!has(object.spec.nodeTaints)` so empty/absent fields pass trivially.
- `examples/scheduledmachine-bad-taint.yaml`: New worked-negative example — four taints, each one tripping a different rule (reserved 5spot prefix, reserved kubelet prefix, duplicate key+effect). Lets operators verify the VAP is actually installed by running `kubectl apply -f` and watching the rejection fire.

### Why
Phase 6 of the user-defined Node taints roadmap — defense in depth. The Rust-side validator in `src/crd.rs` is the authoritative guard (it runs in every cluster regardless of whether the VAP is installed); the VAP is the faster-feedback layer that rejects bad input at API-server admission before the reconciler queues a hopeless work item. Both layers intentionally cover the same rules so a cluster without the VAP still has correct semantics, and a cluster with the VAP gets the sharper error message earlier.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (VAP is re-applied via `kubectl apply -f deploy/admission/validatingadmissionpolicy.yaml`; existing CRs are untouched because the new rules only fire on CREATE/UPDATE)
- [ ] Documentation only

No Rust changes. YAML parse verified via `python3 -c "import yaml; yaml.safe_load_all(...)"` — 19 validations, 1 doc. End-to-end test (kubectl reject) requires a cluster and is covered by Phase 8.

---

## [2026-04-21 15:00] - Termination guards on node-taint reconcile (Phase 5)

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/scheduled_machine.rs`: Added two defensive guards at the top of `reconcile_node_taints_best_effort` — one short-circuits on `metadata.deletion_timestamp.is_some()` (the Node is about to be drained and deleted; applying taints mid-delete leaves a confusing post-mortem state), the other on `spec.kill_switch` (kill-switch routes to `handle_kill_switch`, and taints are not a supported shortcut for eviction — drain is the sanctioned path). Both guards log at debug and return without touching the apiserver.
- `src/reconcilers/scheduled_machine_tests.rs`: Added 5 tests using `tower_test::mock::Handle` with a `next_request()` vs 50ms-sleep race — the watcher task panics if any HTTP request arrives. Covers deletion_timestamp, kill_switch, absent nodeRef, empty nodeRef name, and empty desired+applied. The "no HTTP call" watcher pattern mirrors `test_patch_machine_refs_status_both_none_is_noop_no_http_call`.

### Why
Phase 5 of the user-defined Node taints roadmap. The natural reconcile flow already short-circuits on deletion (deletion_timestamp check at the top of `reconcile` routes to `handle_deletion`) and on kill_switch (routes to `handle_kill_switch` before the Active phase handler). These guards are defense-in-depth: they pin the contract at the function boundary so a future caller that invokes `reconcile_node_taints_best_effort` from a new phase handler cannot accidentally apply taints mid-termination. The tests prove the guards block the apiserver call, not just the return value.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (no runtime behaviour change for the Active path; termination paths now have explicit guards instead of relying on dispatch-level short-circuits)
- [ ] Documentation only

`cargo fmt` ✓ · `cargo clippy --all-targets --all-features -- -D warnings` ✓ · `cargo test --lib` 328/328 ✓.

---

## [2026-04-21 14:30] - Wire node-taint reconcile into `Active` phase (Phase 4)

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/scheduled_machine.rs`: Added `reconcile_node_taints_best_effort` and call it from `handle_active_phase` right after `provision_reclaim_agent_best_effort` (so both side-effects key off the same resolved `nodeRef`). Persists the new applied list via `patch_applied_node_taints_status` only when it differs from `status.applied_node_taints`. `NoNodeYet` / `NodeNotReady` log at debug; `Conflict` logs at warn; all failures are logged-and-returned so taint reconcile cannot block machine scheduling. Early-returns on empty `nodeRef`, empty desired ∧ empty previously-applied, and missing namespace.
- `src/reconcilers/helpers.rs`: Added `ReconcileNodeTaintsInput<'a>` struct, `NodeTaintReconcileOutcome { NoNodeYet | NodeNotReady | Applied { applied } | Conflict { conflicts } }`, `reconcile_node_taints` (GET node → check Ready → diff → apply) and `patch_applied_node_taints_status` (merge-patch wrapper mirroring `patch_machine_refs_status`).
- `src/reconcilers/helpers_tests.rs`: Added 5 tests using `tower_test::mock::pair` — 404 → `NoNodeYet`, Ready=False → `NodeNotReady`, Ready + no-op → `Applied` with zero PATCHes, Ready + add → two GETs + one PATCH + `Applied`, admin collision → `Conflict`.

### Why
Phase 4 of the user-defined Node taints roadmap. With Phase 3's pure `diff_node_taints` + `apply_node_taints` in place, this phase plumbs them into the actual reconcile loop behind the `Active` phase's existing `nodeRef` resolution. Best-effort semantics match `provision_reclaim_agent_best_effort`: taint drift is a workload-scheduling concern, not an availability concern, and must never wedge the reconcile.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (new controller behaviour triggers only when a CR declares `spec.nodeTaints`; CRs without the field see no change)
- [ ] Documentation only

`cargo fmt` ✓ · `cargo clippy --all-targets --all-features -- -D warnings` ✓ · `cargo test --lib` 323/323 ✓.

---

## [2026-04-21 14:00] - Node-taint diff helper + SSA apply IO (Phase 3)

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Added `NodeTaintPlan { to_add, to_update, to_remove, unchanged, conflicts }` with `is_noop()`, pure `diff_node_taints(current, desired, previously_applied)` that identifies taints by `(key, effect)`, treats `value` as mutable, and routes admin-owned collisions into `conflicts` instead of overwriting, and `apply_node_taints(client, node_name, plan)` which GETs the Node, rebuilds `spec.taints` (controller-owned minus `to_remove`, plus admin-owned, plus `to_add`/`to_update`), and SSA-patches under field manager `5spot-controller-node-taints` with the `5spot.finos.org/applied-taints` ownership annotation.
- `src/reconcilers/helpers_tests.rs`: Added 14 tests — 11 pure-diff cases (empty/no-op/add/update-value/remove-previously-applied/keep-admin/admin-collision-on-add/admin-collision-on-remove-value-drift/duplicate-desired-key-across-effects/full-cycle/remove-many) plus 3 IO cases via `tower_test::mock::pair` covering no-op (no PATCH), add (GET + PATCH with correct field manager + annotation payload), and 404-on-GET (error bubbles, no PATCH).

### Why
Phase 3 of the user-defined Node taints roadmap. The diff logic is isolated from IO so that the ownership model — identity on `(key, effect)`, value mutable, no cross-tenant overwrites — is verified exhaustively in pure tests before it meets a real kube client. The apply side uses SSA + annotation so a later `kubectl get node -o json` can trace every controller-owned taint back to the CR.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (new helpers are dead code until Phase 4 wires them into `handle_active_phase`; no runtime behaviour change yet)
- [ ] Documentation only

`cargo fmt` ✓ · `cargo test --lib` 318/318 ✓. (Clippy deferred to Phase 4 — the new `pub fn`s are unused until wired, which trips `-D warnings` `dead_code`; Phase 4 wiring resolves it.)

---

## [2026-04-19 22:30] - Node-taint status surface + `NodeTainted` condition (Phase 2)

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Added `applied_node_taints: Vec<NodeTaint>` to `ScheduledMachineStatus` (defaults to empty, omitted from JSON when empty). This is the controller's record of truth for ownership — only entries it listed here are eligible for removal; admin-added taints colliding on `(key, effect)` surface as a condition instead of being overwritten.
- `src/constants.rs`: Added `CONDITION_TYPE_NODE_TAINTED = "NodeTainted"` and five condition reasons covering every state machine transition for the `NodeTainted` condition: `REASON_NODE_TAINTS_APPLIED` ("Applied"), `REASON_NODE_NOT_READY` ("NodeNotReady"), `REASON_NODE_TAINT_PATCH_FAILED` ("PatchFailed"), `REASON_NO_NODE_YET` ("NoNodeYet"), and `REASON_TAINT_OWNERSHIP_CONFLICT` ("TaintOwnershipConflict").
- `src/crd_tests.rs`: Added five new tests — empty-default behaviour, serialisation-omit-when-empty, round-trip with two taints, deserialise-from-absent, and a hard-coded enforcement of all six condition constant string values so a future rename cannot silently ship.
- `deploy/crds/scheduledmachine.yaml`: Regenerated via `cargo run --bin crdgen` — now publishes the `appliedNodeTaints` status array.

### Why
Phase 2 of the user-defined Node taints roadmap. Separating the status type + condition vocabulary from the reconciler IO (Phase 3) means the diff helper and the apply IO can be built against a stable status contract. The condition reasons are constants (not strings inline in the reconciler) so that the dashboard / alerting layer has a fixed vocabulary to match on.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [x] Documentation-only code change (CALM model + rendered mermaid only; no Rust, no deploy manifests touched)

### Verification
- `make calm-validate` → 0 errors, 0 warnings, 0 info/hints.
- `make calm-diagrams` → regenerates `docs/src/architecture/flows.md` (now 3 flows: activation, deactivation, emergency-reclaim) and `docs/src/architecture/system.md` (14 nodes including the new 4, workload-cluster subgraph now contains API server + reclaim agent).
- `make docs` → `mkdocs build` passes; every hand-authored mermaid component in `docs/src/concepts/emergency-reclaim.md` has a corresponding CALM node.
- [x] Config change only (CRD re-apply picks up the `appliedNodeTaints` status schema; existing CRs are unaffected because the field defaults to empty)
- [ ] Documentation only

`cargo fmt` ✓ · `cargo clippy --all-targets --all-features -- -D warnings` ✓ · `cargo test --lib` 304/304 ✓.

---

## [2026-04-19 22:00] - Add `spec.nodeTaints` CRD schema + validator (Phase 1)

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Added `NodeTaint { key, value?, effect }` struct and `TaintEffect { NoSchedule | PreferNoSchedule | NoExecute }` enum. Added `node_taints: Vec<NodeTaint>` to `ScheduledMachineSpec` (defaults to empty, omitted from serialisation when empty). Added `validate_node_taints()` plus private `validate_taint_key` / `validate_taint_value` / `is_qualified_name` / `is_dns_subdomain` helpers, with reserved-prefix checks for `5spot.finos.org/`, `kubernetes.io/`, `node.kubernetes.io/`, and `node-role.kubernetes.io/`.
- `src/crd_tests.rs`: Added 24 new tests covering serde round-trip (with/without `value`), `TaintEffect` variants, invalid-variant rejection, hash/eq identity, empty-list default, spec omission when empty, and every rejection path of `validate_node_taints` (empty key, leading/trailing hyphen, invalid char, 64-char key, 64-char value, duplicate `(key, effect)`, and each reserved prefix).
- `src/reconcilers/helpers_tests.rs`, `src/reconcilers/scheduled_machine_tests.rs`: Added `node_taints: vec![]` to existing `ScheduledMachineSpec` constructors so they compile against the new field.
- `deploy/crds/scheduledmachine.yaml`: Regenerated via `cargo run --bin crdgen`; now includes the `nodeTaints` array schema with `key`/`value`/`effect` properties and the enum restriction on `effect`.
- `docs/src/reference/api.md`: Regenerated via `cargo run --bin crddoc` (picked up pre-existing `killIfCommands` doc drift in the static generator; `nodeTaints` prose is deferred to Phase 7 per the roadmap's docs-last policy).
- `examples/scheduledmachine-tainted.yaml`: New worked example showing two taints (`workload=batch:NoSchedule`, `dedicated=ml:NoExecute`).

### Why
Phase 1 of the user-defined Node taints roadmap (`~/dev/roadmaps/5spot-user-defined-node-taints.md`). This lands the schema + CR-level validator in isolation so the reconciler work in Phases 2–4 has a stable type to build against. Validation runs at the reconciler boundary for clusters without a ValidatingAdmissionPolicy; the VAP (Phase 6) is the defense-in-depth layer.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CRD re-apply to pick up new `nodeTaints` schema — absent = empty, so existing CRs are unaffected)
- [ ] Documentation only

`cargo fmt` ✓ · `cargo clippy --all-targets --all-features -- -D warnings` ✓ · `cargo test --lib` 299/299 ✓.

---

## [2026-04-21 13:00] - Defer Phase-2-rung-2 netlink proc connector to GitHub issue

**Author:** Erick Bourgeois

### Changed
- `/Users/erick/dev/roadmaps/5spot-emergency-reclaim-by-process-match.md`: Updated the Phase-2 rung-2 (netlink proc connector) status row from "⏳ Not started" to "⏳ Deferred — tracked as [finos/5-spot#40](https://github.com/finos/5-spot/issues/40)". Row body rewritten to name the concrete tradeoff (detection latency ~1s → <10ms; lower idle CPU; deterministic worst case) and note the counter-tradeoff (rung 1 is cheaper under heavy-exec workloads) so future readers don't re-re-evaluate from scratch.
- Opened [finos/5-spot#40](https://github.com/finos/5-spot/issues/40) (label: `enhancement`) carrying the full scope, dependency-choice tradeoff (`nix` vs `neli` vs `netlink-proto`), deployment delta (`CAP_NET_ADMIN` add + suppression-rationale updates), out-of-scope markers (eBPF, cross-node), and acceptance criteria.

### Why
Rung 2 is optimization, not correctness — rung 1 already meets the <5s SLA and integration/stopwatch testing will be the actual critical-path work. Keeping it in the roadmap as "⏳ Not started" was misleading because the decision is already made (defer), not open. Moving the live tracking to a GitHub issue gets it in front of contributors who may want to pick it up, and keeps the roadmap narrative truthful about what's "active work" vs "filed follow-up".

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

No code or manifest changes. `cargo-quality` not re-run (no Rust touched).

---

## [2026-04-21 12:30] - Phase-2.5 async-orchestrator mock-API tests + roadmap alignment

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers_tests.rs`: 4 new async-orchestrator tests for `reconcile_reclaim_agent_provision` using the existing `tower_test::mock` harness. Non-empty commands → Node label merge-patch then ConfigMap server-side apply (asserting `fieldManager=5spot-controller-reclaim-agent` + `force=true` query params, the `enabled` label string in the patch body, and the `data.reclaim.toml` key present in the apply body). Empty commands → Node label patch (JSON-null value) then ConfigMap DELETE. 404-on-delete → benign `Ok(())` so a re-run after partial tear-down completes. Label-PATCH 500 → `ReconcilerError::KubeError` propagation with no second request issued. Also fixed one stale test comment in `test_build_reclaim_agent_configmap_data_key_is_reclaim_toml` that still referenced the old `/etc/5spot/reclaim.toml` mount path — rewrote to describe the watch-based contract.
- `/Users/erick/dev/roadmaps/5spot-emergency-reclaim-by-process-match.md`: Flipped Phase-2.5 status row from 🟡 Partial to ✅ Shipped (agent-side watch consumption + new mock-API tests close the loop; the DaemonSet ConfigMap mount is gone entirely, so the "residual" that was the blocker no longer exists). Flipped Phase-4 status row from 🟡 Partial to ✅ Shipped for unit scope. Updated test count to **275 green** (was 271). Promoted the three runtime-dependent Phase-4 items (kind integration, manual stopwatch, re-enable loop protection) to explicit `TODO-*` follow-ups with their own names so they stop reading as "open Phase 4 unit gaps". Header status line and cumulative-commits reference updated to include the 2026-04-21 increments.

### Why
Two intents bundled: (1) close the last tractable Phase-4 unit-test gap that was pure-Rust and macOS-viable — the async orchestrator paths of `reconcile_reclaim_agent_provision` had pure-helper tests but no request-sequence pinning, which meant a refactor could reverse the label-PATCH → ConfigMap-apply order (or drop force=true, or rename the field manager) without any red test. (2) Align the roadmap narrative with what's actually shipped: with the agent now watching the per-node ConfigMap via kube API and the DaemonSet mount removed, the Phase-2.5 "residual blocker" narrative is genuinely resolved — leaving it as 🟡 Partial would mislead the next reader.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Test count:** 275 green (was 271). `cargo clippy --all-targets --all-features -- -D warnings` clean.

**Still open (runtime-dependent, explicit follow-ups):** Phase 2 rung-2 netlink proc connector (Linux-only, multi-session), kind-cluster integration test, real-hardware stopwatch verification. These are not Phase 4 unit-test gaps — they need infrastructure that isn't appropriate to stub.

---

## [2026-04-21 12:00] - Suppress Trivy KSV-0023 / KSV-0121 / KSV-0012 / KSV-0020 / KSV-0021 / KSV-0105 on reclaim-agent DaemonSet

**Author:** Erick Bourgeois

### Changed
- `.trivyignore`: New `AVD-KSV-0023` entry. The DaemonSet mounts host `/proc` read-only at `/host/proc` so the agent can read `/proc/<pid>/{comm,cmdline}` for every host process — this is the detection contract. There is no Kubernetes-native substitute (Downward API, projected volumes, and CSI drivers do not expose kernel `/proc`). Scope is already minimal: single path (`/proc`), `readOnly: true`, `type: Directory`, and the agent never writes back. Durable: Phase 2 rung-2 (netlink proc connector) still needs host `/proc` for the initial match fan-out.
- `.trivyignore`: New `AVD-KSV-0121` entry. HIGH-severity variant of KSV-0023 that names the disallowed host paths explicitly (`/proc` in our case). Same architectural justification — the detection contract is built around reading host `/proc`, which is exactly what the rule flags.
- `.trivyignore`: New `AVD-KSV-0012` entry. Container-level variant of the existing KSV-0118 root-user finding. Running as UID 0 is required so the agent can read `/proc/<pid>/{comm,cmdline}` under hardened `hidepid=2` /proc mounts. A non-root build would need `CAP_SYS_PTRACE` added back, which is strictly broader than running as root with every other capability dropped, `readOnlyRootFilesystem: true`, `allowPrivilegeEscalation: false`, and seccomp `RuntimeDefault`. Scope is bounded by the opt-in `5spot.finos.org/reclaim-agent: enabled` nodeSelector.
- `.trivyignore`: New `AVD-KSV-0021` entry. Follows directly from KSV-0012 — UID 0 implies GID 0, so `runAsGroup > 10000` cannot be satisfied without abandoning the root requirement already justified. The rule's purpose (avoid a shared high-GID supplementary group leaking filesystem access across workloads) is not a risk here: the container mounts only host `/proc` read-only, has `readOnlyRootFilesystem: true`, and drops all capabilities.
- `.trivyignore`: New `AVD-KSV-0105` entry. Identical root cause to KSV-0012 / KSV-0118 — different rule wording, same finding (UID 0 is required for `hidepid=2` /proc reads; the non-root alternative needs `CAP_SYS_PTRACE`, which is strictly broader than root + all-caps-dropped + read-only rootfs + no-privilege-escalation + seccomp `RuntimeDefault`).
- `.trivyignore`: New `AVD-KSV-0020` entry. "High-UID" strictness variant of KSV-0105 (rule wants UID > 10000, not just > 0). Moot at UID 0: the collision with `root` is deliberate, not accidental. Same architectural justification as the rest of this block.

### Why
Six Trivy findings all flow from the same architectural root cause: the reclaim-agent must read host process state, and the only mechanism Kubernetes offers is a hostPath `/proc` mount + UID 0. Scanner can't express "root is required, everything else is hardened" as a single rule, so each sub-finding needs its own suppression with the shared rationale. Same pattern as the pre-existing KSV-0010 (`hostPID: true`) and KSV-0118 (pod-level default context) suppressions.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only (security-policy document — `.trivyignore` is input to the CI scanner, not a runtime artifact)

---

## [2026-04-21 11:30] - Reclaim-agent: watch per-node ConfigMap via kube API (Phase 2.5 residual)

**Author:** Erick Bourgeois

### Changed
- `src/constants.rs`: New `RECLAIM_CONFIG_DATA_KEY = "reclaim.toml"` — the literal was previously scattered across `helpers.rs` and the binary; centralising it removes the silent-drift risk if one side is renamed.
- `src/reclaim_agent.rs`: New pure helper `configmap_to_config(&ConfigMap) -> Result<Option<Config>, ConfigError>`. Three disciplined outcomes — `Ok(Some)` → arm scanner, `Ok(None)` → idle (CM exists but data key missing), `Err` → malformed payload, caller keeps last-good config. Also re-exports `RECLAIM_CONFIG_DATA_KEY` under the module path so tests and the bin can address it without reaching through `crate::constants::…`.
- `src/reclaim_agent_tests.rs`: 4 new tests covering the three outcomes above plus the `data: None` edge case. Brings reclaim-agent module test count to 33 (library total: **271 tests green**, up from 267).
- `src/bin/reclaim_agent.rs`: Rewrite. The agent no longer loads config from a file at startup — it runs a `kube::runtime::watcher` scoped by field selector `metadata.name=reclaim-agent-<NODE_NAME>` in `RECLAIM_AGENT_NAMESPACE` and bridges every `Event::Apply` / `Event::InitApply` / `Event::Delete` into a `tokio::sync::watch<Option<Config>>` channel. The scanner loop reads the current value each tick: `None` blocks on `rx.changed()` (with a 30s idle safety wakeup); `Some` scans `/proc` every `poll_interval_ms`. Hot-reload is automatic — a controller or operator edit to the ConfigMap propagates in at most one scanner tick. On a malformed CM edit the watcher logs and holds the previous config rather than disarming. `--config` flag removed; `--proc-root`, `--node-name`, `--oneshot` preserved.
- `src/reconcilers/helpers.rs`: `build_reclaim_agent_configmap` now writes under `RECLAIM_CONFIG_DATA_KEY` instead of the bare literal. Behaviour unchanged; the output CM is byte-identical.
- `deploy/node-agent/daemonset.yaml`: Removed the `config` ConfigMap volume and its `/etc/5spot` mount; removed the `--config=/etc/5spot/reclaim.toml` arg. The DaemonSet pod template no longer references any ConfigMap, which sidesteps the K8s limitation (`configMap.name` cannot be downward-API-templated per replica) that blocked the per-node mount previously.
- `deploy/node-agent/rbac.yaml`: New namespaced `Role` + `RoleBinding` (`5spot-reclaim-agent-configmaps` in `5spot-system`) granting the agent SA `get,list,watch` on ConfigMaps in that one namespace. Cluster-wide grant deliberately avoided — the agent only ever needs to see its own node's CM. Existing `ClusterRole` (nodes: get/patch) is unchanged.

### Why
The previous shape shared a single cluster-wide `reclaim-agent-config` ConfigMap across all nodes because the DaemonSet pod template cannot reference a per-replica ConfigMap name. That defeated the per-node specificity the controller was already projecting in the 24:15 increment: the operator could have `node-A` armed to kill `java` and `node-B` armed to kill `idea`, but both agents would see the same config. Moving the CM consumption into the agent (watch API instead of file mount) unblocks per-node arming end-to-end. User-confirmed direction over the alternative (DaemonSet-per-ScheduledMachine) to preserve the kata-deploy-style opt-in pattern and keep N agents at 1 DaemonSet regardless of fleet size.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (new RoleBinding + DaemonSet spec change; kubelet rolls the agent pods on apply)
- [ ] Config change only
- [ ] Documentation only

**Operator notes:**
- The legacy shared `reclaim-agent-config` ConfigMap (if present from a prior install) is no longer mounted and can be deleted by hand; nothing in this tree references it any more.
- Agent arming is now entirely driven by per-node `reclaim-agent-<NODE_NAME>` ConfigMaps — either projected automatically by the controller from `spec.killIfCommands`, or created by hand for manual drills. Deleting the ConfigMap puts the agent back to idle on the next scanner tick.

---

## [2026-04-21 10:15] - Explicit pod-level securityContext on reclaim-agent DaemonSet (Trivy KSV-0118)

**Author:** Erick Bourgeois

### Changed
- `deploy/node-agent/daemonset.yaml`: Added a pod-level `spec.securityContext` block on the DaemonSet pod template. Fields: `runAsNonRoot: false`, `runAsUser: 0`, `runAsGroup: 0`, `seccompProfile.type: RuntimeDefault`. Preceded by a comment explaining why each field is the value it is — in particular that `runAsNonRoot: false` is architecturally required because reading `/proc/<pid>/{comm,cmdline}` for every host pid under hardened `hidepid=2` mounts needs UID 0, and that the non-root alternative (adding back `CAP_SYS_PTRACE`) is a broader privilege than "drop ALL caps + run as root". The per-container `securityContext` on the `agent` container is unchanged (still drops ALL caps, read-only root FS, no privilege escalation, RuntimeDefault seccomp).

### Why
Trivy KSV-0118 ("Default security context configured") fires when a pod does not declare a pod-level `spec.securityContext`, regardless of how locked-down the per-container context is. The DaemonSet had a thorough container-level block but nothing at pod scope, so Trivy reported the pod as "using the default security context, which allows root privileges." An explicit pod-level block satisfies the rule by making the operator's intent auditable at pod scope, without weakening any privilege — it enumerates the same choices the container already made. Keeps parity with the Phase 2.5 architectural constraints documented in the emergency-reclaim roadmap.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (DaemonSet spec change — kubelet will restart the agent pods on rollout)
- [ ] Config change only
- [ ] Documentation only

**Follow-up watch:** if a future Trivy version flags the `runAsNonRoot: false` value itself (some versions tighten this under a different rule ID), the fallback is a `.trivyignore` entry with the same rationale already captured in this comment.

---

## [2026-04-21 10:00] - Suppress Trivy KSV-0010 for reclaim-agent DaemonSet (hostPID is required)

**Author:** Erick Bourgeois

### Changed
- `.trivyignore`: Added a new `DaemonSet — deploy/node-agent/daemonset.yaml` section with a single entry `AVD-KSV-0010` ("Access to host PID namespace"). The block comment explains the architectural rationale: the reclaim-agent watches host processes by scanning `/proc` for argv matches and signals host PIDs to execute graceful stops — both capabilities depend on the agent sharing the host's PID namespace, and pod-scoped `/proc` would make the feature inoperable. The suppression is intentionally narrow (one rule, one workload, one block comment) and follows the same pattern already established for `AVD-KSV-0046` / `AVD-KSV-0048` / `AVD-KSV-0041` on the RBAC `ClusterRole` and `AVD-KSV-0125` on the controller Deployment.

### Why
The IaC security scan job in `.github/workflows/build.yaml` runs `aquasecurity/trivy-action` in `config` mode against the whole repo and uploads SARIF to GitHub Code Scanning. Without an entry in `.trivyignore`, the `hostPID: true` on the reclaim-agent DaemonSet (required by design — see Phase 2.5 in the emergency-reclaim roadmap) would surface as a recurring HIGH finding in every PR and push. The workflow already references `./.trivyignore` (build.yaml:717), so the file is the right lever.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

---

## [2026-04-20 24:15] - Phase 2.5 remainder: controller-side label stamp + per-node ConfigMap projection

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Added four pure helpers — `render_reclaim_toml(&[String]) -> String` (emits a commented TOML body that round-trips through `reclaim_agent::parse_config`), `per_node_configmap_name(&str) -> String` (`reclaim-agent-<node>` from `RECLAIM_AGENT_CONFIGMAP_PREFIX`), `build_reclaim_agent_configmap(node, commands) -> ConfigMap` (carries `app.kubernetes.io/component=reclaim-agent` for `kubectl get -l` discovery; `data["reclaim.toml"]` is the projected body), and `build_reclaim_agent_label_patch(enable: bool) -> serde_json::Value` (merge-patch that sets the label to `enabled` or to JSON `null` for delete). Async orchestrator `reconcile_reclaim_agent_provision(&Client, node, &[String])` drives the full projection: label PATCH first (load-bearing — the DaemonSet's `nodeSelector` depends on it), then ConfigMap apply on non-empty commands or delete on empty (404 treated as benign for idempotent tear-down). Server-side apply uses field manager `5spot-controller-reclaim-agent` distinct from the main reconciler so audit logs can attribute writes.
- `src/reconcilers/scheduled_machine.rs`: New `provision_reclaim_agent_best_effort` helper runs from `handle_active_phase` right after `patch_machine_refs_status` — this is where we first hold a real `nodeRef` for the current Machine. Projection is deliberately best-effort with respect to the reconcile loop: a failed label PATCH or ConfigMap apply degrades the emergency-reclaim path but must not block day-to-day scheduling. An empty `spec.killIfCommands` still invokes the orchestrator, which hits the tear-down arm (label=null, delete CM) so clearing the spec cleans up past projections idempotently.
- `src/reconcilers/helpers_tests.rs`: 11 new unit tests — 3 for `render_reclaim_toml` (round-trip via `parse_config`, empty-commands round-trip still parses, quote-in-command is escaped), 2 for `per_node_configmap_name` (uses prefix constant, preserves node name verbatim without re-sanitisation), 3 for `build_reclaim_agent_configmap` (namespace + name contract, `reclaim.toml` data key required by DaemonSet volume mount, operator-discovery labels present), and 3 for `build_reclaim_agent_label_patch` (enable writes the `enabled` constant, disable writes JSON null rather than empty string, strategic-merge safety — only `metadata.labels.<one key>` is touched).

### Why
Phase 2.5 MVP landed on 2026-04-20 with the CRD field (`spec.killIfCommands`) and an in-pod DaemonSet, but projection was deferred — operators had to hand-label nodes and share a single cluster-wide `reclaim-agent-config` ConfigMap, which both defeats the opt-in intent and loses per-node pattern specificity. The increment here wires the controller to do that projection automatically: a `ScheduledMachine` with `killIfCommands = ["java"]` now gets the opt-in label + a named `ConfigMap` carrying that list appearing on its backing Node without operator action, and clearing the list cleanly tears both down. Matches the 2026-04-20 ShipDocs "As-implemented" contract in the roadmap's Phase 2.5 follow-up block.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (controller acquires PATCH on Nodes + CRUD on ConfigMaps in `5spot-system` — RBAC update required; tracked separately from this entry)
- [ ] Config change only
- [ ] Documentation only

**Note on the DaemonSet YAML switch:** `deploy/node-agent/daemonset.yaml` still references the single shared `reclaim-agent-config` ConfigMap. Switching the mount to the per-node `reclaim-agent-<node-name>` shape is blocked by a Kubernetes limitation (volume-level `configMap.name` cannot be templated by downward API); a future agent-side rewrite to fetch its own ConfigMap at startup will unblock that switch. The per-node ConfigMaps projected by this change are present in-cluster today — the DaemonSet will consume them once the agent side lands.

---

## [2026-04-20 23:45] - Phase 4 tests: pin check_emergency_reclaim early-return contract

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/scheduled_machine_tests.rs`: Added 4 regression tests covering the Phase-3 dispatch guard's early-return paths. Three cover `check_emergency_reclaim` returning `Ok(None)` without any apiserver call when (a) `status` is absent, (b) `status.node_ref` is `None`, or (c) `node_ref.name` is empty. The fourth pins `handle_emergency_remove_phase`'s fast-fail `InvalidConfig` guard for non-namespaced resources. Harness trick: the existing `le_mock_client()` has no response handle attached, so any outbound API request hangs forever — each test wraps the call in a 100ms `tokio::time::timeout` so a hang fails cleanly with a clear message rather than blocking CI.

### Why
Phase-3 dispatch shipped at 23:15 without unit-test coverage of the guard itself. The guard's contract — "do not touch the apiserver when there is no node to reclaim" — is load-bearing for two reasons: (1) a Node.get("") call 400s and would surface as a CAPI error rather than the benign None the caller expects, and (2) every pending `ScheduledMachine` (before CAPI populates `status.nodeRef`) would otherwise hit the apiserver on every reconcile. These tests pin that contract so a future refactor cannot regress the guard into an unconditional fetch.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only (test-only change; no production code touched)

---

## [2026-04-20 23:30] - kind: tag locally built image as `local-dev`; override Deployment image via `kubectl set image`

**Author:** Erick Bourgeois

### Changed
- `Makefile`: `KIND_IMAGE` default changed from `ghcr.io/finos/5-spot:v0.1.0` (which matched the pinned tag in `deploy/deployment/deployment.yaml`) to `ghcr.io/finos/5-spot:local-dev`. The tag unambiguously signals a developer build and decouples the local image from the Deployment manifest's release pin. `kind-deploy` now runs `kubectl -n 5spot-system set image deployment/5spot-controller controller=$(KIND_IMAGE)` immediately after `apply -R -f deploy/deployment/` to redirect the Deployment at the locally loaded image.

### Why
User preference: the locally built image should be labeled as a developer build, not impersonate the pinned release tag. The post-apply `kubectl set image` patch keeps `deploy/deployment/deployment.yaml` untouched (and therefore safe to check in / reflect production) while still letting `kind-setup` run end-to-end against the local build.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (build tooling only)
- [ ] Documentation only

---

## [2026-04-20 23:15] - Phase 3 dispatch: wire handle_emergency_remove into reconcile loop

**Author:** Erick Bourgeois

### Changed
- `src/constants.rs`: Added `PHASE_EMERGENCY_REMOVE = "EmergencyRemove"`, `REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE`, and `EMERGENCY_DRAIN_TIMEOUT_SECS = 60` (deliberately shorter than the 300s graceful drain — the agent's process-match has already decided the node must leave).
- `src/reconcilers/helpers.rs`: New pure builders — `build_emergency_reclaim_event`, `build_emergency_disable_schedule_event`, `emergency_reclaim_message` — plus the orchestrating `handle_emergency_remove` handler that executes the seven-step ordering contract (event → phase EmergencyRemove → drain → delete Machine → PATCH enabled=false → clear annotations → phase Disabled). Load-bearing step (5) propagates errors so the loop-breaker can retry; steps 1, 3, 6 are best-effort. Removed `#[allow(dead_code)]` from `ReclaimRequest`, `node_reclaim_request`, `build_clear_reclaim_patch`, `build_disable_schedule_patch` — all now have callers.
- `src/reconcilers/scheduled_machine.rs`: New `check_emergency_reclaim` guard runs after `kill_switch` but before schedule evaluation; fetches the Node, calls `node_reclaim_request`, and drives the full handler when annotated. New `handle_emergency_remove_phase` match arm catches a crash between annotation-clear (step 6) and phase-flip (step 7), finishing the transition to Disabled idempotently. Updated state-diagram doc comment with the EmergencyRemove → Disabled edge.
- `src/crd_tests.rs`: 3 new tests pinning the phase name, the disable-schedule event reason, and the drain-timeout bound (via `const` block).
- `src/reconcilers/helpers_tests.rs`: 10 new unit tests covering the three pure builders — warning severity, reason-constant use, note formatting with and without the optional reason/timestamp, and message-floor behaviour.
- Pre-existing clippy cleanups: backtick-wrapped `DaemonSet`/`ConfigMap` in rustdoc across `src/constants.rs`, `src/crd.rs`, `src/bin/reclaim_agent.rs`; collapsed duplicate `MatchSource` match arm to `|`; converted `match` to `let-else` in `scan_proc`; swapped a `Default::default()` for explicit `BTreeMap::default()` in reclaim_agent_tests.

### Why
The Phase-3 primitives landed in the previous session (helpers shipped behind `#[allow(dead_code)]`) — they had no caller. This change wires them into the reconcile loop so the annotation actually triggers the reclaim. Without this step, the node-side agent could paint the reclaim annotation but the controller would ignore it and keep following the schedule, re-adding the node at the next window. The seven-step ordering contract (disable-schedule *before* annotation-clear) ensures a crash at any point leaves state that the next reconcile can replay idempotently from the top.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (controller must ship the new dispatch path; agent manifests unchanged)
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-20 21:20] - kind-load: bypass `cross`, native cargo cross-compile + plain `docker build`; kind-deploy: apply namespace first

**Author:** Erick Bourgeois

### Changed
- `Makefile`: `kind-load` is now self-contained and no longer calls into the `docker-build-*` → `prepare-binaries-linux-*` → `build-linux-*` chain. New recipe: host-arch → triple/linker/docker-arch mapping; checks the cross-linker is on PATH (prints `brew tap messense/macos-cross-toolchains && brew install <triple>` hint if missing); ensures the rustup target is installed via `rustup target add`; runs `cargo build --release --target <triple>` (leveraging the `linker = "<triple>-gcc"` entry already in `.cargo/config.toml`); stages the binary at `binaries/<docker-arch>/5spot`; builds the image with plain `docker build --build-arg TARGETARCH=<docker-arch> -t $(KIND_IMAGE) .` (no buildx → no `~/.docker/buildx/buildkitd.toml` dependency, no `cross` Docker image pull); loads into kind.
- `Makefile`: `kind-deploy` now applies `deploy/deployment/namespace.yaml` first, polls for namespace existence (up to 10s), then runs `kubectl apply -R -f deploy/deployment/`. Prevents the `NotFound: namespaces "5spot-system" not found` race where the ConfigMap/Deployment admission requests arrive before the kube-apiserver's namespace controller has fully registered the namespace even though the Namespace object was already created in the same `apply -R` batch.

### Why
1. **cross bypass**: `cross 0.2.5` on Apple Silicon fails for both `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` targets with `toolchain 'stable-x86_64-unknown-linux-gnu' may not be able to run on this system` — rustup rejects installing a non-native-host toolchain even though the requested *target* is correct. The `build-linux-arm64` recipe tries `cross` first and only falls back to the Homebrew `messense/macos-cross-toolchains` path if `cross` is absent. Rather than editing that existing target (which CI relies on), `kind-load` now does its own simpler build that uses the Homebrew toolchain directly.
2. **namespace race**: `kubectl apply -R` does sort by install-order (namespaces before namespaced resources) but kind's admission stack can briefly NotFound a freshly-created namespace for namespace-scoped resources submitted in the same batch. An explicit namespace-first apply plus short readiness poll eliminates the race.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (build tooling only; `kind-load` now requires the Homebrew cross toolchain on macOS — prints an install hint if missing)
- [ ] Documentation only

---

## [2026-04-20 20:55] - kind-load: build image for host arch, not hardcoded amd64

**Author:** Erick Bourgeois

### Changed
- `Makefile`: `kind-load` no longer depends on `docker-build` (which forces `linux/amd64`). The recipe now inspects `uname -m`, dispatches to `docker-build-amd64` on `x86_64` and `docker-build-arm64` on `arm64`/`aarch64`, and retags the matching per-arch local image (`$(IMAGE_NAME):$(IMAGE_TAG)-$$ARCH`) as `$(KIND_IMAGE)` before `kind load`.

### Why
On Apple Silicon, `cross build --target x86_64-unknown-linux-gnu` invoked by `docker-build` → `build-linux-amd64` tries to install the `stable-x86_64-unknown-linux-gnu` host toolchain via rustup, which rejects it on an arm64 host (`toolchain may not be able to run on this system`). Even if the build succeeded via `--force-non-host`, kind nodes on Apple Silicon run arm64, so an amd64 image would fail at pod start with exec-format errors. Building for the host arch matches kind's node arch and sidesteps the cross-toolchain install entirely.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (build tooling only)
- [ ] Documentation only

---

## [2026-04-20 20:30] - Makefile targets for local kind-cluster testing

**Author:** Erick Bourgeois

### Changed
- `Makefile`: New `kind-*` target group — `kind-install` (downloads kind v0.24.0 with SHA-256 checksum verification, platform-detected), `kind-create` / `kind-delete` (idempotent lifecycle for a cluster named `$(KIND_CLUSTER_NAME)`, default `5spot-dev`, using `$(KIND_NODE_IMAGE)`, default `kindest/node:v1.31.0`), `kind-load` (docker-build + retag local image as `$(KIND_IMAGE)` → `ghcr.io/finos/5-spot:v0.1.0` to match the in-cluster `Deployment.spec.containers.image`, then `kind load docker-image`), `kind-deploy` (applies `deploy/crds/` + `deploy/deployment/` and waits on `rollout status deployment/5spot-controller -n 5spot-system` up to 180s), `kind-example` (applies `examples/scheduledmachine-basic.yaml`), `kind-status` (summary of cluster / controller pods / ScheduledMachines), `kind-setup` (meta target chaining `kind-create` → `kind-load` → `kind-deploy`). Added 5 new variables (`KIND_VERSION`, `KIND_CLUSTER_NAME`, `KIND_NODE_IMAGE`, `KIND_IMAGE`) and registered the 8 new targets in `.PHONY`.
- `docs/src/development/setup.md`: Local-Kubernetes/kind section now documents the new `make kind-*` targets (one-shot + individual steps + override examples) and keeps the raw `kind create cluster` invocation as a fallback for bespoke cluster topologies.

### Why
Developers testing `ScheduledMachine` locally currently have to hand-assemble the `kind create` → `docker build` → image-tag-align → `kind load` → `kubectl apply -f deploy/crds/` → `kubectl apply -R -f deploy/deployment/` sequence, and each step has a non-obvious gotcha (the deployment hardcodes `ghcr.io/finos/5-spot:v0.1.0`, `imagePullPolicy: IfNotPresent` only resolves if the loaded image matches, the controller namespace is `5spot-system`). The new targets encode that sequence so `make kind-setup` is the one command for a working test environment.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (build tooling only; no source, CRD, or deploy-manifest changes)
- [x] Documentation only (updated `docs/src/development/setup.md`)

---

## [2026-04-20 22:30] - Emergency reclaim — disable-schedule patch + lifecycle docs

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: New `build_disable_schedule_patch()` helper — returns a merge-patch body that flips `spec.schedule.enabled=false` on the owning `ScheduledMachine`. Marked `#[allow(dead_code)]` pending Phase-3 reconciler dispatch (same pattern as the sibling `build_clear_reclaim_patch`). Rustdoc documents the ordering contract: disable-schedule PATCH must run *before* the annotation-clear PATCH so that a crash between the two steps retries idempotently from the top.
- `src/reconcilers/helpers_tests.rs`: 3 new tests — `enabled=false` is emitted as the JSON literal `false` (not null, which would delete the key under merge-patch semantics); strategic-merge safety asserting only `spec.schedule.enabled` is addressed and no siblings are touched; belt-and-braces guard that the function never emits `enabled=true` (re-enable is a human action, never a controller decision).
- `docs/src/concepts/emergency-reclaim.md` (new): Dedicated lifecycle page for the process-match kill switch. Five mermaid diagrams — trigger chain flowchart, lifecycle state diagram showing `EmergencyRemove` integrated into the existing phase machine, sequence diagram for the end-to-end eject flow, opt-in installation flow, re-enable flow showing what happens when the user re-enables with the matched process still running. Covers why the exit is `Disabled` (not `Pending`), the two-kill-switch family (`killSwitch` vs `killIfCommands`), matching semantics (comm exact vs cmdline substring, case-sensitive), observability (two distinct Events, the `EmergencyReclaimDisabledSchedule` condition, transient Node annotations, agent logs), and explicit out-of-scope items.
- `docs/src/concepts/machine-lifecycle.md`: Added `EmergencyRemove` to the state diagram (transitions in from `Pending` / `Active` / `ShuttingDown`; exits to `Disabled`) and to the phase descriptions. New "Emergency Reclaim Flow" subsection alongside the existing "Kill Switch Flow" so readers can compare the two mechanisms side by side.
- `docs/src/concepts/scheduled-machine.md`: Added `killIfCommands` to the Other Fields table; updated the phase enumeration in Status Fields; added an emergency-reclaim cross-link under Related.
- `docs/src/operations/troubleshooting.md`: New "Emergency Reclaim (Kill Switch)" section — four scenarios (stuck in `EmergencyRemove`; eject loop on every schedule window; agent never fires on a known-matching process; `EmergencyReclaim` event fires but schedule is not disabled) each with concrete `kubectl` / `jq` diagnostics and resolution steps.
- `docs/mkdocs.yml`: Registered `Emergency Reclaim (Kill Switch): concepts/emergency-reclaim.md` under the Concepts nav section. Verified with `mkdocs build` — page renders with all five mermaid blocks.
- `docs/roadmaps/5spot-emergency-reclaim-by-process-match.md` (note: lives in `~/dev/roadmaps`, not in git per user convention): Phase 3 section now mandates the `spec.schedule.enabled=false` flip + the `EmergencyReclaimDisabledSchedule` condition/Event; new Open Question 6 captures the "explicit re-enable vs. auto-resume on process exit" trade-off with MVP rationale; Phase 4 tests include the flip assertion and a re-enable-loop-protection test; user-facing re-enable flow documented verbatim so Phase 3 code + operator docs stay in sync.

### Why
Follow-through on the roadmap review question: when emergency reclaim ejects a node, does it set `spec.schedule.enabled=false`? The correct answer is yes — otherwise the next schedule window silently re-adds the node, the agent sees the still-running matched process, and the eject→re-add→re-eject loop repeats every schedule boundary forever. The `build_disable_schedule_patch` helper gives the Phase-3 reconciler-dispatch work the primitive it needs to break that loop. Documentation was the second half of this commit because the kill-switch semantics (two mechanisms, different triggers, different exits, different reset paths) are not obvious from the code alone — a lifecycle feature this consequential needs a dedicated concepts page with diagrams, a troubleshooting section for the field, and cross-links from every page that mentions either kill switch.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout (the new helper is `#[allow(dead_code)]` until Phase 3 wires it into the reconciler — no runtime behaviour change in this commit)
- [ ] Config change only
- [x] Documentation only (the one code change — `build_disable_schedule_patch` — is dormant until Phase 3 calls it)

### Follow-up (not in this commit)
- Phase 3 dispatch wiring: call `build_disable_schedule_patch` after drain + Machine delete, before `build_clear_reclaim_patch`; emit the `EmergencyReclaimDisabledSchedule` Event and condition documented on the new concepts page; move the `#[allow(dead_code)]` off the helper once it has a caller.
- Integration test (in `tests/`): kind cluster + stub k0smotron, annotate Node, assert `spec.schedule.enabled` flips to `false`, assert the condition / Event appear, assert re-enabling while the trigger is still present re-fires the eject and re-flips `enabled=false`.

---

## [2026-04-20 19:45] - Emergency reclaim by process match (Phases 1–2.5 MVP)

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: New optional `kill_if_commands: Option<Vec<String>>` field on `ScheduledMachineSpec`. Opt-in semantics mirror kata-deploy — `None` / absent preserves existing behavior; `Some(vec![...])` opts the node in to the reclaim-agent DaemonSet. `Some(vec![])` is legal but inert (detector never matches) and reserved for future controller-side validation warnings.
- `src/crd_tests.rs`: 4 positive/negative tests for the new field + 5 tests for the reclaim annotation/label constants, plus `kill_if_commands: None` added to the existing spec-serialization test.
- `src/constants.rs`: 9 new constants — reclaim trigger / reason / timestamp annotation keys, literal `"true"` trigger value, the opt-in node label key + value, the agent namespace, the per-node ConfigMap prefix, and the `EmergencyReclaim` condition reason.
- `src/reclaim_agent.rs` (new): Library surface for the node-side agent — `Config` (TOML), `parse_config` / `load_config`, `scan_proc` (reads `/proc/<pid>/comm` exact + `/proc/<pid>/cmdline` substring), `Match` / `MatchSource`, `build_patch_body` (strategic-merge that only touches `metadata.annotations`), `already_requested` (strict literal-`"true"` check). Empty match lists return `None` by design; `poll_interval_ms = 0` is rejected at load time.
- `src/reclaim_agent_tests.rs` (new): 21 tests — config happy/error paths, synthetic `/proc` trees via `tempfile::TempDir`, case-sensitivity, mid-scan process-exit race tolerance, missing-`/proc`-root error, idempotence across `"true"` / `"false"` / `"0"` / empty values.
- `src/bin/reclaim_agent.rs` (new): `5spot-reclaim-agent` binary. Clap CLI with `--config`, `--proc-root`, `--node-name` (downward API, required), `--oneshot`. Reads NODE via downward API, builds in-cluster kube client, runs idempotence pre-check, then loops `scan_proc` at the configured interval and PATCHes the Node on first match. Field manager name is distinct (`5spot-reclaim-agent`) so audit logs can attribute writes.
- `src/reconcilers/helpers.rs`: `ReclaimRequest` struct + `node_reclaim_request(node)` detector (returns `None` unless the trigger annotation is literally `"true"`) + `build_clear_reclaim_patch()` (merge-patch that sets all three annotations to `null` — Kubernetes merge-patch semantics for key deletion).
- `src/reconcilers/helpers_tests.rs`: 7 tests covering the detector across all annotation shapes + clear-patch structure; added `kill_if_commands: None` to the existing test spec literal.
- `src/reconcilers/scheduled_machine_tests.rs`: Added `kill_if_commands: None` to `create_test_spec`.
- `src/lib.rs`: Exposed the new `reclaim_agent` module.
- `src/bin/crddoc.rs`: Added `killIfCommands` example snippet + full field description block (crddoc is static `println!`-driven, not schema-derived).
- `docs/reference/api.md`: Regenerated via `crddoc`; now documents `killIfCommands` (lines 55–57, 133–143).
- `deploy/crds/scheduledmachine.yaml`: Regenerated via `crdgen`; schema now exposes `killIfCommands` at line 92.
- `Cargo.toml`: Added `toml = "0.8"` dependency + a new `[[bin]]` block for `5spot-reclaim-agent` targeting `src/bin/reclaim_agent.rs`.
- `deploy/node-agent/daemonset.yaml` (new): Opt-in DaemonSet gated by `nodeSelector: { 5spot.finos.org/reclaim-agent: enabled }`. Runs as root (reads every pid) with `hostPID: true`, host `/proc` mounted read-only at `/host/proc`, `system-node-critical` priority, tolerates every taint, `terminationGracePeriodSeconds: 5`. Config comes from a ConfigMap-projected volume at `/etc/5spot/reclaim.toml`. Image is pinned to `v0.1.0` (no `:latest`).
- `deploy/node-agent/rbac.yaml` (new): Dedicated ServiceAccount + ClusterRole + ClusterRoleBinding. Scope is narrow by design — only `get` + `patch` on `nodes`, no `list` / `watch`, no access to Machine / Pod / ScheduledMachine. Separation of identity from the main controller (distinct SA) lets audit logs unambiguously attribute a PATCH to the node-side agent vs. the controller.
- `deploy/node-agent/reclaim.toml.example` (new): Reference config — the real config is projected per-node by the controller.
- `deploy/node-agent/kustomization.yaml` (new): `kubectl apply -k deploy/node-agent/` bundle.

### Why
Follow-through on the `docs/roadmaps/5spot-emergency-reclaim-by-process-match.md` roadmap: certain workloads on shared fleet nodes (interactive JVMs left overnight, game clients, IDE processes) can indefinitely delay graceful drain windows, blocking scheduled remove operations and stranding capacity. The emergency-reclaim path gives operators an opt-in, process-match trigger that bypasses `gracefulShutdownTimeout` / `nodeDrainTimeout` and moves the machine into a non-graceful remove phase immediately on first match. The MVP implements rung 1 of the two-rung detection ladder (`/proc` poll) with the same Node-annotation contract that rung 2 (netlink proc connector, future work) will reuse. The opt-in is doubly gated — a node must (a) be labeled `5spot.finos.org/reclaim-agent=enabled` by the controller (which only stamps when the parent `ScheduledMachine` has a non-empty `killIfCommands`) AND (b) have its per-node ConfigMap populated — so the feature has zero effect on clusters that do not configure it.

### Impact
- [ ] Breaking change (new field is opt-in; absent / `None` preserves all existing behavior)
- [x] Requires cluster rollout (`kubectl apply -f deploy/crds/scheduledmachine.yaml` to pick up `killIfCommands` in the CRD schema; `kubectl apply -k deploy/node-agent/` only if operators want the reclaim agent available — otherwise it remains inert)
- [ ] Config change only
- [ ] Documentation only

### Follow-up (not in this commit)
- Controller dispatch: wire `node_reclaim_request()` + `build_clear_reclaim_patch()` into the reconciler, add `Phase::EmergencyRemove`, emit a Kubernetes Event with `REASON_EMERGENCY_RECLAIM`, call `kubectl drain --grace-period=0 --force --disable-eviction`, delete the CAPI `Machine` immediately, clear the annotation last.
- Controller-side projection of the per-node `reclaim-agent-<node-name>` ConfigMap + label stamp from `spec.killIfCommands` — currently the manifests assume a single shared `reclaim-agent-config`; the controller work will switch this to per-node as the roadmap specifies.
- Rung 2 (netlink proc connector) — requires `CAP_NET_ADMIN`; out of MVP scope.
- OCI image build + release wiring for the `5spot-reclaim-agent` binary (tarball + SBOM + VEX path already exists for the controller; extend it).

---

## [2026-04-20 00:15] - Grant `update` on `scheduledmachines/finalizers` to unblock Machine creation

**Author:** Erick Bourgeois

### Changed
- `deploy/deployment/rbac/clusterrole.yaml`: New rule granting `update` on `scheduledmachines/finalizers` under the `5spot.finos.org` API group. Added as a separate rule (rather than widening the existing `scheduledmachines` / `scheduledmachines/status` rule) so the verb surface on the finalizers subresource stays exactly at the minimum the API-server admission check requires. Inline comment documents the link back to `src/reconcilers/helpers.rs` where `blockOwnerDeletion: true` is set, and quotes the exact API-server error that the missing rule produces.

### Why
`src/reconcilers/helpers.rs:859` stamps `blockOwnerDeletion: true` on the `ownerReference` it writes from every child CAPI `Machine` back to its parent `ScheduledMachine`. Kubernetes treats `blockOwnerDeletion` as functionally equivalent to holding a finalizer on the owner, so during admission it checks that the creating service account has `update` on `<owner-resource>/finalizers` — a separate subresource in RBAC terms from the main `scheduledmachines` resource and from `scheduledmachines/status`. The controller's ClusterRole has shipped since the initial commit with full CRUD on `scheduledmachines` and `scheduledmachines/status` but without any rule for the `finalizers` subresource, so every Machine creation is rejected by the API server with `machines.cluster.x-k8s.io "<name>" is forbidden: cannot set blockOwnerDeletion if an ownerReference refers to a resource you can't set finalizers on`. This is a latent RBAC bug, not a regression. The fix grants exactly the verb the API server checks (`update`); no broader verb set is needed and granting more would violate least-privilege.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (re-apply the ClusterRole; no controller restart required — RBAC changes are picked up on the next API request)
- [ ] Config change only
- [ ] Documentation only

### Follow-up (optional, not in this commit)
- Integration test that creates a `ScheduledMachine` against a kind cluster with the shipped ClusterRole and asserts the child `Machine` is admitted. Would catch any future regression in RBAC vs. `ownerReference` shape without waiting for a prod cluster rollout to expose it.

---

## [2026-04-19 21:50] - Triage GHSA-cq8v-f236-94qc (rand soundness); widen VEX identifier shapes

**Author:** Erick Bourgeois

### Changed
- `osv-scanner.toml` (new, at repo root): Ignore entry for `GHSA-cq8v-f236-94qc` with `ignoreUntil = 2026-10-19` and a reason that mirrors the VEX statement. This is the file shape Scorecard's `Vulnerabilities` check (and osv-scanner itself) expect — per the remediation text in the Scorecard finding.
- `.vex/GHSA-cq8v-f236-94qc.toml` (new): OpenVEX statement with `status = not_affected`, `justification = vulnerable_code_not_in_execute_path`, explicit impact statement. Applies to both the Chainguard and Distroless product identifiers.
- `tools/validate_vex.py`: Widened the accepted identifier set from `CVE-YYYY-NNNN+` only to also accept `GHSA-xxxx-xxxx-xxxx` (case-insensitive) and `RUSTSEC-YYYY-NNNN`. The TOML field is still named `cve` for backward compatibility with the existing 11 statement files. New `_is_accepted_id()` helper composes the three regexes; error message updated to list all three shapes.
- `tools/assemble_openvex.py`: Dropped the `.upper()` on `doc["cve"]` when rendering `vulnerability.name`. GHSA ID segments are canonically lowercase and upper-casing them breaks round-tripping against osv.dev / github.com/advisories. CVE IDs in existing files are already uppercase, so this is a no-op for them.
- `tools/tests/validate-vex-tests.sh`: Added happy-path cases `valid-ghsa` and `valid-rustsec`, plus negative cases `invalid-ghsa-format`, `invalid-rustsec-format`, and `duplicate-ghsa` (uniqueness check across case variants of a GHSA ID).
- `tools/tests/fixtures/{valid-ghsa,valid-rustsec,invalid-ghsa-format,invalid-rustsec-format,duplicate-ghsa}/` (new fixture dirs).
- `tools/tests/assemble-openvex-tests.sh`: Added regression guard asserting `GHSA-cq8v-f236-94qc` is emitted verbatim (not upper-cased) into `vulnerability.name`.
- `.vex/README.md`: Documented the expanded identifier format.

### Why
OpenSSF Scorecard's `Vulnerabilities` rule flagged `GHSA-cq8v-f236-94qc` (rand soundness when a custom `log` logger calls `rand::rng()` mid-reseed) on `rand 0.8.6`, pulled transitively via `warp 0.3.7` → `tokio-tungstenite 0.21` → `tungstenite 0.21` where rand is used exclusively for websocket frame masking. The vulnerable path requires the `log` logger itself to call into rand during a reseed, which 5-Spot's tracing-subscriber stack never does. The advisory ships without a CVE ID, which the existing VEX validator (`CVE-YYYY-NNNN+` only) rejected — making it the first advisory the VEX pipeline could not represent. Widening the validator to accept GHSA + RUSTSEC identifiers unblocks this triage and every future non-CVE advisory without forcing a field rename of the existing 11 files. The full fix (rand >= 0.9.3) requires crossing a warp 0.3.x semver boundary (warp 0.3 hard-pins tokio-tungstenite ^0.21) and is tracked as a follow-up, not attempted here.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI / supply-chain)
- [ ] Documentation only

### Follow-up (optional, not in this commit)
- Warp 0.3 → 0.4 or axum migration so `tokio-tungstenite` and the transitive `rand` can be bumped past the vulnerable range; remove `osv-scanner.toml` entry and the VEX statement when that lands.
- Consider renaming the TOML field from `cve` to `id` once every existing statement has been regenerated; not worth the churn today.

---

## [2026-04-19 21:25] - Remove bogus `vexctl validate` step from build-vex job

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml` (`build-vex` job): Deleted the `Install vexctl` step (VEXCTL_VERSION 0.3.0) and the `Run vexctl validate vex.openvex.json` step. Updated the inline comment on the adjacent `Install Grype` step to drop the now-dangling "mirrors the vexctl install step above" reference.

### Why
`vexctl` has no `validate` subcommand — it never has — and runs of `build-vex` on push-to-main were failing with `unknown command "validate" for "vexctl"`. The step was added on the assumption that vexctl mirrored the OpenVEX reference implementation's schema validator; it does not. vexctl's 0.3.0 subcommand surface is `attest / create / generate / merge / verify`. Since vexctl was not used anywhere else in the job (Cosign does the attestation, not vexctl), the install step was removed with it rather than left as dead weight. Input-side schema/enum/uniqueness validation is still performed up front by `tools/validate-vex.sh` (which runs `validate_vex.py` against `.vex/*.toml`) — that's the real defensive gate. No behaviour change on pull_request events: the whole `build-vex` job is gated by `if: github.event_name != 'pull_request'` and only runs on push-to-main + release.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI workflow)
- [ ] Documentation only

### Follow-up (optional, not in this commit)
If a second-opinion schema check of the *assembled* `vex.openvex.json` is wanted, add a step that validates against the OpenVEX JSON Schema with a tool that actually ships a validator (e.g. `python -m jsonschema -i vex.openvex.json openvex-schema.json`). Record the dependency in `~/dev/roadmaps/5spot-vex-generation-and-signing.md § Dependencies`.

---

## [2026-04-19 21:10] - Add child-cluster MutatingAdmissionPolicy to label Nodes for kata-deploy

**Author:** Erick Bourgeois

### Changed
- `deploy/admission/child-cluster-kata-runtime-mutatingpolicy.yaml` (new): `MutatingAdmissionPolicy` (`admissionregistration.k8s.io/v1alpha1`) that matches `nodes` on `CREATE` and stamps `katacontainers.io/kata-runtime=true` onto `metadata.labels` via an `ApplyConfiguration` patch. `failurePolicy: Ignore` so a policy evaluation error can never block kubelet from registering a Node.
- `deploy/admission/child-cluster-kata-runtime-mutatingpolicybinding.yaml` (new): `MutatingAdmissionPolicyBinding` activating the policy cluster-wide on the child cluster. `matchResources: {}` covers every Node; an `objectSelector` can be added to scope to a specific pool.

### Why
The child (workload) cluster needs Nodes labelled for the upstream `kata-deploy` DaemonSet's default `nodeSelector`. Labelling at Node `CREATE` is the cleanest admission shape: kubelet updates the `Ready` condition via the `nodes/status` subresource, which the API server does not allow to mutate `metadata.labels`, so a true "on Ready" mutation is not possible. `kata-deploy` is a DaemonSet, and DaemonSet pod lifecycle already gates installation on Node readiness — matching on `CREATE` and letting the DaemonSet handle Ready yields the behaviour the user asked for without introducing a controller.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (child-cluster manifests; not wired into the 5-spot controller)
- [ ] Documentation only

### Follow-up (optional, not in this commit)
- Promote to `admissionregistration.k8s.io/v1beta1` once the child-cluster floor is Kubernetes >= 1.34.
- If a subset of Nodes should opt out of kata, add an `objectSelector` to the binding rather than conditions in the policy expression.

---

## [2026-04-19 20:45] - Fix vexctl installer 404 in build.yaml

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml` (Install vexctl step): Changed the download URL from the non-existent `vexctl_${VEXCTL_VERSION}_linux_amd64.tar.gz` tarball to the raw binary `vexctl-linux-amd64`. Removed the `tar -xzf` step accordingly. Kept the version pin at `0.3.0` and the inline comment, expanded to describe the actual asset layout (raw binaries + detached Cosign `.sig`/`.pem` per platform).

### Why
The OpenVEX project does not publish versioned tarballs for vexctl. Their release assets are raw platform binaries named `vexctl-<os>-<arch>` (hyphens, no version, no `.tar.gz`). The workflow was copy-pasted from the Grype install step — which *does* ship tarballs in the format `grype_<version>_<os>_<arch>.tar.gz` — so the pattern looked reasonable but 404ed on the first CI run. Verified the correct asset name against `api.github.com/repos/openvex/vexctl/releases/tags/v0.3.0`. Grype's own installer was audited and is correct.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI workflow)
- [ ] Documentation only

### Follow-up (optional, not in this commit)
vexctl ships detached Cosign signatures (`vexctl-linux-amd64.sig` + `.pem`). A follow-up could verify the binary with `cosign verify-blob` before installing, closing a small supply-chain gap on the installer itself.

---

## [2026-04-19 20:30] - Add Community section to README (FINOS Slack #5-spot)

**Author:** Erick Bourgeois

### Changed
- `README.md`: New `## Community` section between `## Contributing` and `## Security`, pointing to `#5-spot` on the FINOS Slack workspace (<https://finos-lf.slack.com/channels/5-spot>, join at <https://finos.org/slack>), plus GitHub Issues and GitHub Discussions as the other two canonical contact surfaces. Security-sensitive reports are explicitly redirected back to the Security section so they do not land in public Issues.

### Why
New contributors and users landing on the repo had no documented way to reach the project maintainers for usage questions short of opening an Issue. Publishing the FINOS Slack channel — now that the project lives under the FINOS org — gives a low-friction conversation surface and matches what every other FINOS project does.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-19 20:15] - Rename Service to `controller` (RFC 1035 compliance)

**Author:** Erick Bourgeois

### Changed
- `deploy/deployment/service.yaml`: Renamed `metadata.name` from `5spot-controller` to `controller`. Kubernetes Services require **RFC 1035** DNS labels (`[a-z]([-a-z0-9]*[a-z0-9])?`) — stricter than the RFC 1123 rule used by Namespace/Deployment/ConfigMap/RBAC — and labels must start with a letter, not a digit. The cluster API server rejects `5spot-controller` with `metadata.name: Invalid value: "5spot-controller": a DNS-1035 label…`. Added a header comment explaining the constraint so nobody renames it back.
- `docs/src/installation/controller.md`: Updated the port-forward example from `svc/5spot-controller` to `svc/controller`.

### Why
`kubectl apply --dry-run=client` did not catch this — client-side dry-run skips the Service-specific admission validator. Only `--dry-run=server` (or an actual apply) surfaces the RFC 1035 rule. Renaming to `controller` is safe here because the Service is already scoped by the `5spot-system` namespace; cluster DNS resolves it as `controller.5spot-system.svc.cluster.local`. The ServiceMonitor selector matches on label `app: 5spot-controller` (not on Service name), so monitoring continues to work without change.

### Audit: every other kube resource name

Server-side validated against a live cluster — all resources other than the Service were already compliant. Kubernetes applies different name rules per kind; this matrix confirms each one:

| Kind | Name | Rule | Result |
|---|---|---|---|
| Namespace | `5spot-system` | RFC 1123 label (digit-start OK) | ✅ |
| ConfigMap | `5spot-controller-config` | RFC 1123 subdomain | ✅ |
| Deployment | `5spot-controller` | RFC 1123 subdomain | ✅ |
| Service | `controller` | **RFC 1035 label** (letter-start required) | ✅ (renamed) |
| NetworkPolicy | `5spot-controller` | RFC 1123 subdomain | ✅ |
| PodDisruptionBudget | `5spot-controller` | RFC 1123 subdomain | ✅ |
| ClusterRole | `5spot-controller` | RFC 1123 subdomain | ✅ |
| ClusterRoleBinding | `5spot-controller` | RFC 1123 subdomain | ✅ |
| ServiceAccount | `5spot-controller` | RFC 1123 subdomain | ✅ |
| ServiceMonitor | `5spot-controller` | CR default = RFC 1123 subdomain | ✅ |
| CustomResourceDefinition | `scheduledmachines.5spot.finos.org` | `<plural>.<group>`, each label RFC 1123 (digit-start OK) | ✅ |
| ValidatingAdmissionPolicy | `scheduledmachine-validation` | RFC 1123 subdomain | ✅ |
| ValidatingAdmissionPolicyBinding | `scheduledmachine-validation-binding` | RFC 1123 subdomain | ✅ |
| ScheduledMachine | `business-hours-worker`, `weekend-worker` | RFC 1123 subdomain | ✅ |

### Unrelated issue surfaced during validation
`examples/scheduledmachine-weekend.yaml` fails server-side admission with `unknown field "spec.schedule.cron"` — the example is out of date against the current CRD schema (which uses day/hour ranges, not cron expressions). Tracked separately; not fixed in this commit.

### Impact
- [x] Breaking change (anyone referencing `5spot-controller.5spot-system.svc.cluster.local` — e.g. in ServiceMonitor `endpoints[*].port` lookups by Service name, Ingress backends, or custom dashboards — must use `controller.5spot-system.svc.cluster.local`)
- [x] Requires cluster rollout (old Service object must be `kubectl delete`d; the new one will be created on re-apply)
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-19 19:45] - Resolve Trivy IaC findings in GitHub Code Scanning

**Author:** Erick Bourgeois

### Changed
- `deploy/deployment/deployment.yaml`:
  - Replaced `image: ghcr.io/RBC/5-spot:latest` with `image: ghcr.io/finos/5-spot:v0.1.0` (Trivy **KSV-0013** — image `:latest` tag; also removes a stale RBC-internal reference that violated the CLAUDE.md internal-references rule and pointed at a non-existent org).
  - Added `seccompProfile: { type: RuntimeDefault }` at both the pod `spec.securityContext` and the container `securityContext` (Trivy **KSV-0104** — seccomp policies disabled, **KSV-0030** — runtime/default seccomp profile not set).
  - Added `runAsGroup: 65534` to the container `securityContext` (Trivy **KSV-0021** — runs with low GID; 65534 is the `nogroup`/`nobody` GID on distroless and Chainguard images).
- `Dockerfile`:
  - Removed "RBC Capital Markets" from the copyright header (CLAUDE.md internal-references rule).
  - Pinned the distroless base image by digest: `gcr.io/distroless/cc-debian13:nonroot@sha256:8f960b7fc6a5d6e28bb07f982655925d6206678bd9a6cde2ad00ddb5e2077d78`. Dependabot (docker ecosystem) will open a re-pin PR when Google publishes a patched image.
- `Dockerfile.chainguard`:
  - Removed "RBC Capital Markets" from the copyright header.
  - Pinned the Chainguard glibc-dynamic base image by digest: `cgr.dev/chainguard/glibc-dynamic:latest@sha256:fa0d07a6a352921b778c4da11d889b41d9ef8e99c69bc2ec1f8c9ec46b2462e9` (Trivy **DS-0001** — `:latest` tag used). Chainguard rebuilds this tag daily with security patches and Dependabot picks up the new digest on each rebuild.
- `.trivyignore` (new): Six architecturally-justified suppressions, each with a written rationale so an auditor can answer "why is this ignored" without reading code:
  - `AVD-KSV-0046` — RBAC wildcard on `bootstrap.cluster.x-k8s.io` and `infrastructure.cluster.x-k8s.io` API groups is required by the provider-agnostic design (controller must support any CAPI provider installed in the cluster).
  - `AVD-KSV-0048` — Pod/eviction permissions are required for node drain during machine shutdown.
  - `AVD-KSV-0041` — Secret access is **read-only** (get/list/watch), required to resolve SSH keys and bootstrap-data references.
  - `AVD-KSV-0125` — Registry allow-listing is a cluster-level admission policy (Kyverno / OPA / VAP), not a workload field.
  - `AVD-KSV-01010` — ConfigMap "sensitive content" finding is a false positive; the ConfigMap holds only log-level strings and port numbers.
  - `AVD-DS-0026` — Kubernetes uses `livenessProbe`/`readinessProbe` on `:8081` for health; Dockerfile `HEALTHCHECK` would be dead code, and distroless/Chainguard have no shell to run one.
- `.github/workflows/build.yaml`: `Run Trivy config scan` step now explicitly passes `trivyignores: ./.trivyignore` so the suppressions apply deterministically in CI (Trivy auto-discovers the file by default, but the explicit path documents intent and survives future action-version bumps).

### Why
GitHub Code Scanning was showing 10 open Trivy IaC findings against 5-Spot's deploy manifests and Dockerfiles, including two **error-severity KSV-0046** alerts. Most were real hardening gaps (seccomp, low GID, `:latest` tags, stale RBC image path); a handful (CAPI wildcards, pod eviction, read-only secret access) are load-bearing architectural choices that belong in an explicit suppression list with written justification rather than being fixed away. A local `trivy config` run after the changes now reports **0 misconfigurations** across all 17 scanned files. Separately, this fixes two CLAUDE.md policy violations — the `ghcr.io/RBC/5-spot` image reference and the "RBC Capital Markets" copyright headers — that had been quietly sitting in the tree.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (deployment.yaml securityContext + image tag change)
- [x] Config change only (Dockerfile digest pins, `.trivyignore`, CI workflow input)
- [ ] Documentation only

### Operational note
The deployment image is now pinned to `v0.1.0` — the current latest GitHub release. When a new release is cut, either edit `deploy/deployment/deployment.yaml` or override the tag via your kustomize/Helm overlay. Do **not** revert to `:latest`.

---

## [2026-04-19 18:00] - Add OpenVEX generation, validation, and signing pipeline

**Author:** Erick Bourgeois

### Changed
- `.vex/README.md` (new), `.vex/.gitkeep` (new): repository convention for hand-authored per-CVE triage. One TOML file per CVE with `status`, `justification` (enum), `products`, `author`, and `timestamp`. Documents the workflow, the allowed enum values, and what is required per status (`not_affected` → justification; `affected`/`under_investigation` → `action_statement`).
- `tools/validate-vex.sh` (new), `tools/validate_vex.py` (new): shell + Python (`tomllib`, stdlib) validator. Checks per-file schema, enum membership (status, justification), non-empty `products`, RFC-3339 UTC timestamp, and CVE uniqueness across files. Deterministic error output.
- `tools/assemble_openvex.py` (new): assembler that reads `.vex/*.toml` and emits a single OpenVEX v0.2.0 JSON document with a canonical `@id`. Runs `validate_dir` first so malformed input never yields a document. Normalizes TOML datetimes to RFC-3339 UTC `Z`-suffix strings, sorts keys for diffable output.
- `tools/tests/validate-vex-tests.sh` + 18 fixture dirs: 19 cases covering every positive/negative/exception branch (empty dir, missing dir, missing each required field, malformed TOML, bad CVE format, invalid status/justification, empty products, bad timestamp, missing-justification-when-required, missing-action-statement-when-required, duplicate CVE across files, valid-single / valid-multiple / valid-affected).
- `tools/tests/assemble-openvex-tests.sh`: 6 cases covering happy path (file + stdout), JSON validity, validator-gate negative path, CLI argument errors, default-timestamp RFC-3339 shape.
- `.github/workflows/build.yaml`:
  - Added `.vex/**` and `tools/**` to the PR `paths:` filter so changes to either directory trigger the workflow.
  - New PR-only `validate-vex` job runs the validator unit tests and then validates the live `.vex/` directory. Gates PRs touching `.vex/**` or `tools/**`.
  - New release-only `build-vex` job (`needs: [docker, extract-version]`): defensively re-runs the validator, calls `assemble_openvex.py` with a canonical `@id = https://github.com/<repo>/releases/tag/<tag>/vex`, installs pinned `vexctl` and runs `vexctl validate` as a second-opinion check, Cosign-attests the document to both image digests (`cosign attest --type openvex` for Chainguard + Distroless), runs `actions/attest-build-provenance` on the document, and uploads `vex.openvex.json` + `.bundle` as a workflow artifact.
  - `upload-release-assets` now `needs: build-vex`, downloads the `openvex` artifact, copies the document to `release/` and the attestation bundle to `signatures/`, includes both in `checksums.sha256`, and publishes the document as a release asset.
- `docs/src/security/vex.md` (new): user-facing page covering what VEX is, what gets published, how to verify (Cosign + `gh attestation verify`), how to consume (`grype --vex`, `trivy --vex`), and how maintainers add statements. Linked from `docs/src/security/index.md` and `docs/mkdocs.yml` nav.
- `README.md`: Security section now lists OpenVEX under the publication surface with links to `docs/src/security/vex.md` and `.vex/README.md`.
- `.claude/SKILL.md`: `pre-commit-checklist` skill gains a "If preparing a release" block requiring every open Trivy finding to have a statement in `.vex/`, plus explicit commands for the validator and the two test scripts.

### Why
Without VEX, every Trivy CVE surfaced on 5-Spot images re-triages itself at every downstream consumer. Publishing a signed OpenVEX document once — per release, bound to image digests via Cosign, attached to the GitHub Release — pushes the triage decision to exactly one place (`.vex/<cve>.toml`, PR-reviewed) and lets scanners (Grype, Trivy, Harbor) suppress already-triaged findings without repeating the analysis. Keeps 5-Spot honest: only human-authored statements ship, and CI won't emit a document from a malformed source tree.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow + new repo convention)
- [ ] Documentation only

### Operational note
- **vexctl pin:** `build-vex` installs vexctl by downloading a tagged release tarball (`VEXCTL_VERSION: '0.3.0'`). Per the roadmap's "Dependencies to pin" section this should be replaced with SHA-pinned asset verification (`cosign verify-blob`) before the first release actually flows through this workflow; re-pin quarterly with the rest of the signing toolchain.
- **First release after merge:** add at least one hand-authored statement in `.vex/` (even `under_investigation` on a known low-severity CVE) so the signing chain is exercised end-to-end and the release contains a non-empty `vex.openvex.json`.
- **Phase 4 of the roadmap (CycloneDX-VEX co-emission) is intentionally deferred** — kept gated behind an explicit consumer ask per the roadmap's decision gate. Phases 1/2/3/5 are now in place.

---

## [2026-04-19 17:15] - Add Dependabot config for Actions, Cargo, and Docker

**Author:** Erick Bourgeois

### Changed
- `.github/dependabot.yml` (new): SPDX-headered Dependabot v2 config with three ecosystems:
  - **`github-actions`** — weekly Monday 09:00 `America/Toronto`, limit 10 PRs. Grouped update `actions-routine` bundles low-risk first-party + tooling bumps (`actions/*`, `docker/*`, `github/codeql-action`, `softprops/action-gh-release`, `anchore/sbom-action`, `aquasecurity/trivy-action`) into one PR. Security-sensitive actions (`sigstore/*`, `EmbarkStudios/cargo-deny-action`, `ossf/*`) are left ungrouped so each one opens as an individual PR for per-change review. `dtolnay/rust-toolchain` explicitly ignored (branch-tracking ref, no tags — re-pinned manually on quarterly cadence).
  - **`cargo`** — weekly Monday, limit 5 PRs. Groups patch + minor bumps; majors open individually. cargo-audit + cargo-deny CI gates block regressions.
  - **`docker`** — weekly Monday, limit 3 PRs. Picks up `FROM ...@sha256:...` digest bumps once Dockerfiles adopt digest pinning (currently no-op; future-proof).
- Commit-message prefix: `ci` for Actions, `chore` for Cargo and Docker. Labels applied to every PR for easy filtering.

### Why
The previous commit SHA-pinned all external GitHub Actions across the workflows, which is correct for supply-chain hygiene but creates stagnation risk — pins do not auto-update, so known-vulnerable action versions can sit in CI indefinitely. Dependabot solves the stagnation side of the trade-off: it opens a PR per new release with the full changelog diff, and the same CI gates (Semgrep, Trivy, cargo-deny, Scorecard) that guard any other PR guard the bump itself. This closes the loop between "pin everything" and "keep pins current" without inviting humans to cowboy-update SHAs.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (Dependabot)
- [ ] Documentation only

### Operational note
- **PR volume:** expect 1–3 Actions PRs and 1–2 Cargo PRs per week initially, tapering as the tree stabilizes. The `actions-routine` group keeps the routine noise in one PR.
- **Review rule of thumb:** security-sensitive bumps (sigstore, cargo-deny, ossf) → read the full release notes before merging. Routine Actions group → verify Scorecard Pinned-Dependencies still passes, spot-check the diff for any behaviour change flags, merge if CI is green.
- **Future-proof Docker:** `Dockerfile` and `Dockerfile.chainguard` use tag-based `FROM` lines today. If we move to digest pinning (`FROM image@sha256:...`), Dependabot starts opening PRs for base-image digest bumps automatically — no further config change needed.

---

## [2026-04-19 17:00] - Pin all GitHub Actions to full commit SHAs

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`, `calm.yaml`, `calm-test.yaml`, `docs.yaml` and `.github/actions/prepare-docker-binaries/action.yaml`: replaced every `@<tag>` reference with `@<40-char-sha> # <tag>` for all external actions. 77 replacements across 5 files covering `actions/*`, `docker/*`, `sigstore/cosign-installer`, `anchore/sbom-action`, `aquasecurity/trivy-action`, `EmbarkStudios/cargo-deny-action`, `softprops/action-gh-release`, `github/codeql-action/upload-sarif`, `dtolnay/rust-toolchain`, and all `firestoned/github-actions/*` sub-actions. `scorecard.yaml` was already SHA-pinned and is unchanged.
- `.github/workflows/build.yaml`: the SLSA reusable workflow at `slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@v2.1.0` stays on a semver tag — pinning to SHA would break SLSA provenance verification because `slsa-verifier` validates against approved released versions. Added an inline multi-line comment explaining this so future reviewers don't "fix" it by SHA-pinning.
- `aquasecurity/trivy-action` was previously pinned to the nonexistent tag `0.28.0` (missing `v` prefix — the tag is `v0.28.0`). Bumped to the current latest `v0.35.0` at SHA `57a97c7e7821a5776cebc9bb87c984fa69cba8f1`.
- `dtolnay/rust-toolchain@stable` resolved to SHA `29eef336d9b2848a0b548edc03f92a220660cdb8` with an inline comment noting it is a moving ref that should be re-pinned quarterly to keep Rust current.

### Why
Closes the OpenSSF Scorecard `Pinned-Dependencies` finding across the entire CI surface. Unpinned action tags are a supply-chain risk: a compromised or silently-force-pushed tag would silently execute attacker code in our workflows with access to `GITHUB_TOKEN`, `SNYK_TOKEN`-equivalent secrets, and GHCR push permissions. SHA pinning freezes behaviour to a specific reviewed commit; Dependabot can later send PRs to bump pins with a full diff.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow)
- [ ] Documentation only

### Operational note
- **Dependabot for Actions is strongly recommended** as a follow-up so pins don't rot. Add `.github/dependabot.yml` with a `github-actions` ecosystem entry; Dependabot will open one PR per outdated action SHA with a changelog link.
- **Re-pin cadence:** security-sensitive actions (`sigstore/*`, `ossf/*`, `EmbarkStudios/cargo-deny-action`) should be bumped within a week of a new release. Build tooling (`docker/*`, `actions/*`) is less time-sensitive. `dtolnay/rust-toolchain` should be bumped quarterly to keep the Rust toolchain current.
- All pin comments include the resolved semver tag (e.g. `# v4.3.1`) so a human reader can see at a glance what version is in use without clicking through to the SHA.

---

## [2026-04-19 17:30] - Phase 5 docs finish: regenerate stale api.md (mdBook source)

**Author:** Erick Bourgeois

### Changed
- `docs/src/reference/api.md`: regenerated via `make crddoc`. Now includes the Phase 1 status-schema additions — `providerID`, extended `nodeRef` (with `uid`, `apiVersion`, `kind` alongside `name`), `machineRef`, `bootstrapRef`, `infrastructureRef`, and refreshed condition/observedGeneration docstrings. Previously this file was stale; Phase 1's regen wrote only to `docs/reference/api.md` (a legacy path referenced by `.claude/SKILL.md`), not to the mdBook source path that actually renders on the doc site (`docs/src/reference/api.md`, per the `Makefile` `crddoc` target).

### Why
The Phase 5 checklist in the roadmap required "`docs/src/reference/api.md` — regenerated by `regen-api-docs` skill." Spot-checking after the earlier Phase 5 commit revealed that the canonical Makefile target output path and the SKILL.md instruction disagreed, and the mdBook source copy was still on the pre-Phase-1 schema. Regenerating closes that gap so the doc site reflects what consumers actually get on `kubectl get scheduledmachine -o yaml`.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

### Operational note
- **SKILL.md drift:** `.claude/SKILL.md` currently instructs `cargo run --bin crddoc > docs/reference/api.md`, which writes to the wrong path. The `Makefile` `crddoc` target is correct. A small follow-up should reconcile the skill instructions with the Makefile so future regens aren't silently written to the wrong path.

---

## [2026-04-19 16:30] - Phase 5 docs: watch-topology diagram in architecture.md

**Author:** Erick Bourgeois

### Changed
- `docs/src/concepts/architecture.md`: New top-level section "Watch Topology" with a Mermaid diagram showing the primary `ScheduledMachine` watch plus the two new secondary watches on CAPI `Machine` (label-filtered) and core `Node` (cluster-wide) feeding into pure reverse-mapper functions (`machine_to_scheduled_machine`, `node_to_scheduled_machines`) that enqueue the owning `ScheduledMachine`. Updated the "Controller" component-detail bullet list to enumerate the three watches.
- `docs/roadmaps/5spot-event-driven-watches-and-status-enrichment.md` (copy at `~/dev/roadmaps/`): status header updated to `Phase 5 ✅` — roadmap closed.

### Why
Closes Phase 5 of the event-driven-watches roadmap. Until this commit, the concepts doc described a controller that only watched its own CR; readers had no way to understand why a Node cordon now triggers an immediate reconcile instead of waiting for the next requeue. The watch-topology diagram makes the event-driven claim concrete and auditable.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-19 16:00] - Replace misleading Snyk claim with real OSS security tooling (Semgrep + Trivy config + cargo-deny)

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`: Three new PR + push-to-main jobs (release events skip these — the release pipeline relies on scans that already ran on main):
  - `semgrep-sast` — runs `semgrep scan` in the `returntocorp/semgrep` container with the community `p/rust`, `p/security-audit`, `p/secrets`, and `p/owasp-top-ten` rulesets; `--metrics=off`; uploads SARIF to Code Scanning under the `semgrep` category. No token required.
  - `iac-scan` — `aquasecurity/trivy-action@0.28.0` with `scan-type: config` against the repo root (picks up `deploy/**/*.yaml` + `Dockerfile*` + `Dockerfile.chainguard`); uploads SARIF under the `trivy-iac` category.
  - `cargo-deny` — `EmbarkStudios/cargo-deny-action@v2` running `check --all-features` against the new `deny.toml`.
- `deny.toml` (new, repo root): SPDX-headered cargo-deny config. License allow-list covers Apache-2.0, MIT, BSD-2/3-Clause, ISC, Unicode-DFS-2016, Unicode-3.0, Zlib, CC0-1.0, MPL-2.0, OpenSSL. Advisories: `yanked = "deny"`. Bans: `multiple-versions = "warn"` (to avoid breaking CI on transitive pulls we do not control), `wildcards = "deny"`. Sources: only official crates.io registry, no unknown git URLs.
- `README.md`: Removed the misleading `Snyk` SAST badge (we never actually ran Snyk). Added badges for **Semgrep**, **Trivy** (Container + IaC), **cargo-deny**, **cargo-audit**, **Cosign**, and **SLSA** — each reflecting tooling that actually runs in the pipeline.
- `docs/architecture/calm/architecture.json`: The `supply-chain-scanning` control description now reads: "Repository is scanned by Semgrep OSS (SAST), Trivy (container image + IaC config), cargo-audit + cargo-deny (RustSec advisories, license allow-list, source restrictions), and Gitleaks (secrets); OpenSSF Scorecard publishes supply-chain posture; SPDX license identifiers on source files." Old wording referenced Snyk and Aqua which were never actually wired up.

### Why
The README's Security & Compliance section and the CALM architecture JSON both claimed "Snyk (SAST)" but `rg -i snyk` returned zero hits outside those two strings — the repo has never had Snyk configured. That is a compliance posture lie for a project in a regulated banking context. This commit closes the three real gaps with free OSS tooling: SAST (Semgrep), IaC misconfig scanning (Trivy config), and dependency license/advisory enforcement (cargo-deny). All three are token-free, run on every PR, and write SARIF to the existing Code Scanning dashboard — same surface consumers already use for Scorecard and Trivy container results.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow + cargo-deny config)
- [ ] Documentation only

### Operational note
- **First run of `cargo-deny` may fail** if an existing transitive dep carries a license not in the allow-list. Treat the first failure as signal: either add the license to `deny.toml` (with a justification comment) or file an issue to swap the dep. Do not weaken `unknown-registry = "deny"` — that is a supply-chain boundary, not a nuisance.
- **Semgrep findings initially expected**: the first run will surface Rust and OWASP-style findings that were never triaged. Review in Code Scanning → filter by tool `semgrep` → triage-or-suppress with justification (Semgrep supports `// nosemgrep: rule-id — reason` inline comments).
- **Trivy IaC findings**: scope is the `deploy/` manifests plus both Dockerfiles. Findings you cannot fix (e.g., a base-image constraint) can be suppressed via a `.trivyignore` file at repo root with a comment.
- All three jobs run as `needs: [verify-commits]` so they parallelize with `extract-version`/`build` and do not serialize the critical path.

---

## [2026-04-19 15:45] - Add OpenSSF Scorecard badge to README

**Author:** Erick Bourgeois

### Changed
- `README.md`: Added `[![OpenSSF Scorecard]...]` badge as the first entry in the "Security & Compliance" section, linking to `https://scorecard.dev/viewer/?uri=github.com/finos/5-spot`.

### Why
The `scorecard.yaml` workflow already publishes results to the OpenSSF REST API (`api.securityscorecards.dev`) and to GitHub Code Scanning, but without a visible badge the score is invisible to anyone reading the README. The badge is the canonical consumer-facing signal that the project runs Scorecard and auto-updates with each `scorecard.yaml` run.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2026-04-19 15:30] - Sign binaries and generate SLSA provenance on push-to-main (not only on release)

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/build.yaml`:
  - `sign-artifacts` job: `if` flipped from `github.event_name == 'release'` to `!= 'pull_request'`. Added `attestations: write` and `contents: read` permissions. New step runs `actions/attest-build-provenance@v2` on each signed tarball (amd64 + arm64) — GitHub-native attestation in addition to the Cosign signature.
  - `generate-provenance-subjects` job: `if` flipped to `!= 'pull_request'` so SLSA subject hashes are computed on push-to-main too.
  - `slsa-provenance` job: `if` flipped to `!= 'pull_request'`; the reusable SLSA generator now runs on every push-to-main. `upload-assets` parameterised to `${{ github.event_name == 'release' }}` so the `.intoto.jsonl` only attaches to the GitHub Release on release events; on push-to-main it lands as a workflow artifact (verifiable via `slsa-verifier` or `gh attestation verify`).
- `upload-release-assets` unchanged — stays `if: github.event_name == 'release'`; its `needs:` on `sign-artifacts`/`slsa-provenance` resolves cleanly whether those jobs ran (push, release) or were skipped (PR).

### Why
Every merge to `main` produces a buildable, distributable artifact — it should carry the same supply-chain assurances as a release. With these changes, a `main` build now has: Cosign keyless signature on the container image (already in place), GitHub Artifact Attestation on the container image (already in place), Cosign signature on each binary tarball (new on `main`), GitHub Artifact Attestation on each binary tarball (new), and SLSA Level 3 provenance for the binaries (new on `main`). Consumers pulling a `main-YYYY-MM-DD` build can now cryptographically verify it the same way they would a release.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow)
- [ ] Documentation only

### Operational note
- Push-to-main runs now take longer (extra ~3–5 min for the SLSA generator + attestations). Expected trade-off.
- `sign-artifacts` signed-tarball artifact retention is the repo default (90 days) — long enough for verification and audit, well under the release-asset lifetime.
- Binary SLSA generator dependency pinned at `@v2.1.0` (unchanged) — verify this is the latest patched version periodically.

---

## [2026-04-19 15:00] - Harden OpenSSF Scorecard workflow: SPDX header + pin remaining action

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/scorecard.yaml`: Prepended the project's standard two-line SPDX/Copyright header (`Copyright (c) 2025 Erick Bourgeois, firestoned` + `SPDX-License-Identifier: Apache-2.0`) to match every other workflow in `.github/workflows/`. Pinned the previously unpinned `github/codeql-action/upload-sarif@v3` to the full commit SHA for v3.35.2 (`ce64ddcb0d8d890d2df4a9d1c04ff297367dea2a`) with an inline version comment — brings the last step into line with the other three actions in the file which were already SHA-pinned.

### Why
OpenSSF Scorecard's own `Pinned-Dependencies` check flags float-tagged actions (`@v3`) as a supply-chain risk, so the scorecard workflow itself should score well on that check. The SPDX header is the repo-wide convention; every other workflow has it. Both changes are compliance housekeeping — no behavior change.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

---

## [2026-04-19 14:00] - Phases 3 + 4: Event-driven `.watches()` on CAPI Machine and Node

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Added two pure mapper functions used as secondary-watch reverse-maps on the Controller builder:
  - `machine_to_scheduled_machine(&DynamicObject) -> Vec<ObjectRef<ScheduledMachine>>` — reads the `5spot.eribourg.dev/scheduled-machine` label (constant `LABEL_SCHEDULED_MACHINE`) and emits at most one `ObjectRef`. Guards against missing/empty/whitespace label values, missing namespace, and imposter labels with similar prefixes.
  - `node_to_scheduled_machines<'a, I: IntoIterator<Item = &'a ScheduledMachine>>(&Node, I) -> Vec<ObjectRef<ScheduledMachine>>` — returns all SMs whose `status.nodeRef.name` matches `node.metadata.name`. Returns multiple refs on conflict so the reconciler can surface the issue. O(N) per Node event; documented as acceptable for current scale.
- `src/reconcilers/mod.rs`: Re-exports both mappers alongside the existing `error_policy`, `evaluate_schedule`, `should_process_resource`.
- `src/main.rs`: Extended the `Controller` builder with `.watches_with(...)` on CAPI `Machine` (`ApiResource` from GVK, label-filtered `watcher::Config`) and `.watches(...)` on `Node`. The Node mapper closure captures a clone of `controller.store()` (`reflector::Store<ScheduledMachine>`) and calls `state()` on each event to avoid out-of-band API lists.
- `src/reconcilers/helpers_tests.rs`: Added 14 new unit tests — 7 for `machine_to_scheduled_machine` (positive match, missing label, no labels, empty value, whitespace value, missing namespace, wrong-prefix imposter) and 7 for `node_to_scheduled_machines` (single match, multi-match conflict, no match, empty list, SM-without-status, Node-without-name, empty `nodeRef.name`). Covers positive, negative, and exception paths per the project's 100%-coverage rule.

### Why
Closes roadmap Phases 3 + 4 of `event-driven-watches-and-status-enrichment.md`. The `ScheduledMachine` controller previously only watched its own CR — all downstream state was polled during reconciles, which CLAUDE.md explicitly forbids ("ALWAYS use event-driven programming… as opposed to polling"). With these two watches, CAPI setting `status.nodeRef` or a Node being drained now enqueues an immediate reconcile via kube-rs's watch stream (<1s debounce) instead of waiting for the next periodic requeue. The label-based reverse map on Machines uses the label we already stamp on every child — zero new ownership semantics, no `controller: true` owner references introduced, no contention with CAPI's own controllers.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — controller now opens additional watches (CAPI `Machine` with label selector; `Node` cluster-wide)
- [ ] Config change only
- [ ] Documentation only

### Operational note
- RBAC: the controller already has `get`/`list`/`watch` on CAPI `Machine` (used for drain lookup) and needs `list`/`watch` on core `Node`. Verify `get,list,watch` on `nodes` is present in the ClusterRole before rollout; if not, add it in the same rollout.
- Observability: the existing `reconcile_queue_depth` metric will show enqueues driven by Machine and Node events in addition to SM events.

---

## [2026-04-19 10:00] - Phase 2: Close test-coverage gap for async CAPI helpers

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers_tests.rs`: Added 14 `tower-test`-backed unit tests covering positive, negative, and exception paths for the three async helpers introduced in the previous Phase 2 entry:
  - `fetch_capi_machine` — 200 returns `Some`, 404 returns `Ok(None)`, 500 and 403 map to `CapiError`.
  - `patch_machine_refs_status` — both-fields patch asserts full body shape, single-field patches assert the other key is **omitted** (not null) so merge-patch never clears existing values, both-`None` case asserts **zero HTTP traffic**, 500 and 404 map to `KubeError`.
  - `get_node_from_machine` — success returns the node name, machine-404 and nodeRef-missing both return `Ok(None)`, 500 propagates as `CapiError`.

### Why
Per durable user guidance: every function (public **and** private) must have unit tests covering the happy path, negative cases, and exception/error paths. The original Phase 2 change landed with tests only for the pure `extract_machine_refs` function — the three async helpers, which actually touch the Kubernetes API surface, were undertested. This entry closes that gap so the coverage floor now matches the project rule (not just CLAUDE.md's "public function" minimum).

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only (test-only change — no runtime behaviour modified)

---

## [2026-04-18 12:00] - Phase 2: Populate providerID and nodeRef from CAPI Machine

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/helpers.rs`: Added three new helpers — `extract_machine_refs` (pure function pulling `providerID` + full `NodeRef` out of a CAPI Machine `DynamicObject`), `fetch_capi_machine` (typed 404 → `Ok(None)` wrapper around the dynamic GET), and `patch_machine_refs_status` (merge-patches both fields onto `ScheduledMachine.status`). Refactored `get_node_from_machine` to delegate to the first two helpers for the drain path.
- `src/reconcilers/scheduled_machine.rs`: `handle_active_phase` happy path now fetches the CAPI Machine, extracts refs, and patches them onto the SM status. Failures are logged and ignored — status enrichment must never block reconciliation.
- `src/reconcilers/helpers_tests.rs`: Added 6 TDD cases for `extract_machine_refs` covering fully populated, empty, providerID-only, nodeRef-without-uid, incomplete-nodeRef-returns-None, and malformed-providerID-ignored.

### Why
Phase 2 of the event-driven watches + status enrichment roadmap. The schema landed in Phase 1; this change actually populates the new fields every time the reconciler visits an active machine, so `kubectl get sm -o jsonpath='{.status.providerID}{"\t"}{.status.nodeRef.name}'` returns meaningful values.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — controller image carries the new reconcile logic
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-16 17:00] - Phase 1: Enrich ScheduledMachine status with providerID and full nodeRef

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Added `provider_id: Option<String>` (serialized as `providerID`) to `ScheduledMachineStatus`. Replaced the thin `LocalObjectReference { name }` node reference with a new `NodeRef { apiVersion, kind, name, uid }` struct mirroring CAPI's `Machine.status.nodeRef`. Removed the now-unused `LocalObjectReference` type.
- `src/crd_tests.rs`: Added 6 TDD cases covering providerID round-trip, full `nodeRef` deserialization, optional `uid`, serialization omission, old-shape rejection, and `NodeRef` round-trip.
- `src/bin/crddoc.rs`: Documented new `providerID` and `nodeRef` status fields.
- `deploy/crds/scheduledmachine.yaml`: Regenerated from the updated Rust types.
- `docs/reference/api.md`: Regenerated to reflect new status schema.

### Why
Phase 1 of the event-driven watches + status enrichment roadmap. Surfacing `providerID` and a full Node reference (with UID) on `ScheduledMachine.status` lets operators correlate a scheduled machine to a specific VM and Node from `kubectl get sm -o jsonpath=...`, without manual lookups across CAPI Machines and the Node API. This is the schema foundation that Phase 2 (reconciler populates the fields) and Phases 3–4 (event-driven watches on CAPI Machine and Node) build upon.

### Impact
- [x] Breaking change — `status.nodeRef` shape changed from `{ name }` to `{ apiVersion, kind, name, uid }`. Existing CRs with the old shape must clear `status.nodeRef` before rollout, or the controller will report deserialization errors on that field.
- [x] Requires cluster rollout — CRD must be re-applied alongside the new controller image.
- [ ] Config change only
- [ ] Documentation only

---

## [2026-04-17] - Bump base image to cc-debian13 and fix GLIBC_2.39 crash (issue #17)

**Author:** Daniel Guns

### Changed
- `Dockerfile`: Base image bumped from `gcr.io/distroless/cc-debian12:nonroot` (glibc 2.36) to `gcr.io/distroless/cc-debian13:nonroot` (glibc 2.41)
- `.github/workflows/build.yaml`: Pinned Linux x86_64 runner from `ubuntu-latest` to `ubuntu-24.04` for CI stability
- `Cargo.lock`: Updated transitive dependency `rustls-webpki` from `0.103.11` to `0.103.12`

### Why
`ubuntu-latest` now resolves to Ubuntu 24.04 (glibc 2.39), producing binaries that require `GLIBC_2.39` at runtime. The previous `cc-debian12` base only provides glibc 2.36, causing a hard crash at container startup. Bumping to `cc-debian13` (glibc 2.41) resolves the mismatch. Runners are explicitly pinned to `ubuntu-24.04` so CI doesn't break silently when `ubuntu-latest` moves to 26.04. Additionally, two CVEs in `rustls-webpki 0.103.11` (RUSTSEC-2026-0098, RUSTSEC-2026-0099) were patched by bumping to `0.103.12`. Fixes issue #17.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout — new base image
- [ ] Config change only
- [ ] Documentation only

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
