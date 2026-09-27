<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 00 — Release diff: `v0.2.2` → `main`

> 📄 Reference doc — a point-in-time record of what shipped in this range, not a
> phase with a completion state. Generated 2026-07-22; not re-audited against the
> tree since. Later work is recorded in `docs/adr/` and `.claude/CHANGELOG.md`.

Generated 2026-07-22. Scope: 20 commits, 134 files changed, +14,814/−1,305 lines.
Note: `v0.2.3` and `v0.3.0` are both tagged commits *within* this range (`v0.2.3` =
`020d39f`, second half of kata agent; `v0.3.0` = `e530266`, provider unification) —
so this range actually spans two intervening point releases, not one uninterrupted
branch of work.

---

## Executive Summary

This range ships two major feature arcs plus a security/quality hardening pass.
First, **per-node Kata containerd config delivery** (`spec.kata`): a read-only
controller resolution step (ADR 0002) paired with a new privileged node-side
DaemonSet, `5spot-kata-config-agent` (ADR 0003), that writes the drop-in to the
host filesystem and restarts k0s via `nsenter`; a security review then removed
the user-configurable destination path entirely to close a node-root-via-CRD
gate (ADR 0005) and added a compensating `ValidatingAdmissionPolicy` for the two
privileged agents (ADR 0004). Second, **a pluggable spot-schedule provider
architecture** (ADR 0006/0007/0009): activation is no longer an inline
`daysOfWeek`/`hoursOfDay` window evaluated in-process, but a required reference
(`spec.schedule`) to an external provider CRD in a new
`spotschedules.5spot.finos.org` API group, resolved via a duck-typed
`status.active` contract and driven by an event-driven dynamic watch; the
former inline scheduler is reified as a first-party `TimeBasedSpotSchedule`
provider alongside a new `CapitalMarketsSchedule` provider. Both arcs are
backed by 8 new ADRs and CALM model updates. Alongside these, `ScheduledMachine`
moved from `v1alpha1` to `v1beta1` (breaking, pre-release), CAPI Machine API
version handling became fully dynamic (discovery-based, no hardcoded version),
and a security/quality sweep added named HTTP-status constants, an idempotency
pass (409/finalizer handling), an `error_chain()` diagnostic helper, and
cleared a couple of RUSTSEC advisories. Cargo.toml's version also jumped from a
stale `0.1.1` to `0.3.0`, catching the crate version up to the git tag lineage.

---

## New Architecture Decisions

| ADR | Title | Status | Decision |
|---|---|---|---|
| 0002 | Kata config delivery via `spec.kata` | Accepted | Controller only *resolves* a pre-existing ConfigMap/Secret and opts the Node in via label+annotation; writes nothing itself. |
| 0003 | In-pod host restart via `nsenter` | Accepted | Privileged `5spot-kata-config-agent` DaemonSet (`hostPID`, `privileged: true`) does the host file write + `nsenter -t 1 ... systemctl restart`. |
| 0004 | Agent pod-security exception boundary | Accepted | Deny-by-default VAP (`5spot-agent-pod-security`) allowlisting exactly the two agent ServiceAccounts' documented posture. |
| 0005 | Remove `spec.kata.destPath` | Accepted | Destination is now the compile-time constant `/etc/k0s/containerd.d/kata.toml` — closes an arbitrary-host-write vector. |
| 0006 | Pluggable spot-schedule provider contract | Accepted (amended by 0009) | Activation moves to a duck-typed `spotschedules.5spot.finos.org` contract, event-driven watch, hold-last-known-state on failure. |
| 0007 | CRD multi-version support | Accepted (amended by 0009) | Multiple served versions with `conversion.strategy: None`, additive-only fields; true breaking change triggers a future conversion-webhook ADR. |
| 0008 | Auto-VEX presubmission gate | Accepted | Committed VEX snapshots (`.vex/snapshot/`, `.vex/auto/`) + byte-exact CI diff gate forcing human sign-off on machine-authored CVE suppressions. |
| 0009 | Unify `spec.schedule` as provider reference | Accepted | `spec.schedule` becomes required `SpotScheduleRef`; inline window removed; `spec.spotSchedule` folded in/removed; new top-level `spec.enabled` replaces `schedule.enabled`; single served version `v1beta1`. |

---

## Major Feature Streams

### 1. Kata config agent / per-node Kata delivery (`020d39f`, `50f14d6`)

- `src/crd.rs`: `spec.kata: Option<KataConfig>` (`ConfigMap`|`Secret` ref, key,
  restart_service, namespace) — no `destPath` (removed pre-release by ADR 0005).
  Destination fixed via `KATA_CONFIG_DEST_PATH` in `src/constants.rs`.
- Controller (`src/reconcilers/helpers.rs`): resolves the named object
  read-only on the workload cluster, patches the bound Node with label
  `5spot.finos.org/kata-config=enabled` + annotation
  `5spot.finos.org/kata-config-ref`; clears opt-in and sets
  `SourceNotFound`/`TargetNamespaceMissing` conditions on absence. No writes to
  ConfigMaps/Secrets, no namespace creation.
- Agent: `src/kata_config_agent.rs` (+ `_tests.rs`, 581 lines) and
  `src/bin/kata_config_agent.rs`. Atomic temp-file+rename writes, SHA-256
  drift detection (`sha256_hex`/`file_sha256`), `confine_dest_path` defense-in-
  depth path containment, `RestartExecutor` trait wrapping the `nsenter`
  command construction for testability without executing it.
- Teardown handshake: controller clears only the ref annotation; agent sees it
  gone, unlinks the host file, *then* removes its own opt-in label.
- New metrics: `kata_config_{writes,deletes,drift_corrected,restarts,sync_errors}_total`,
  `kata_config_last_sync_timestamp_seconds`.
- Deploy: `deploy/kata-config-agent/{daemonset,kustomization,rbac}.yaml` (new).
- Docs: `docs/src/concepts/kata-config-delivery.md` (230 lines, new),
  `docs/src/guides/kata-config.md` (160 lines, new).
- Tests: `tests/integration_kata_config.rs` (320 lines), `examples/scheduledmachine-kata.yaml`.

### 2. Pluggable SpotSchedule provider architecture (`6733d81`→`a34abfa`)

- New API group `spotschedules.5spot.finos.org`. `SpotScheduleRef {apiVersion,
  kind, name}` in `src/crd.rs`.
- `src/reconcilers/spot_schedule.rs`: `resolve_spot_schedule()` /
  `SpotScheduleVerdict` implementing "unresolved never flaps, hold-last-state"
  (ADR 0006 §4). Status condition `CONDITION_TYPE_SPOT_SCHEDULE_RESOLVED` with
  reasons `ProviderCRDNotInstalled`/`ProviderNotFound`/`StatusActiveMissing`/`ProviderNotReady`.
- `src/reconcilers/spot_schedule_watch.rs`: `ReverseIndex` + `SpotScheduleWatchManager`
  lazily starts one `kube::runtime::watcher` per distinct provider GVK
  (discovery-based `Api<DynamicObject>`), stops it when unreferenced, rebuilds
  from cluster state on restart — no state outside Kubernetes. Wired into
  `src/main.rs` via a second `reconcile_on` stream (mpsc channel,
  `SPOT_SCHEDULE_EVENT_CHANNEL_CAP = 1024`).
- Two first-party providers, each its own CRD/binary/controller/deploy manifest set:
  - `TimeBasedSpotSchedule` — `src/providers/time_based.rs` +
    `src/bin/spot_schedule_time_based.rs`; `deploy/providers/time-based/*`.
  - `CapitalMarketsSchedule` — `src/providers/capital_markets.rs` +
    `src/bin/spot_schedule_capital_markets.rs`; `deploy/providers/capital-markets/*`.
- ADR 0009 unification: `spec.spotSchedule` removed, `spec.schedule` becomes
  required; new top-level `spec.enabled` (default `true`) replaces
  `schedule.enabled`; emergency-reclaim loop-breaker repointed to patch
  `spec.enabled`. `ScheduledMachine`'s `v1alpha1` module dropped.
- Metrics: `capital_markets_active`, `capital_markets_transitions_total`,
  `time_based_active`, `time_based_transitions_total`,
  `spot_schedule_resolutions_total`, `spot_schedule_resolution_errors_total`,
  `spot_schedule_transitions_total` — briefly regressed mid-stream (a merge
  dropped 3 implementations while call sites/tests survived) and fixed same day
  per the changelog.
- Shipping fix (`a34abfa`): the two provider binaries were built but never
  copied into the container image / CI artifact upload (hyphenated `command:`
  vs underscore file stems) — fixed via explicit `[[bin]]` entries in
  `Cargo.toml`, `Dockerfile`/`Dockerfile.chainguard` `COPY` lines, `Makefile`,
  and `.github/workflows/build.yaml` (91 lines changed) /
  `.github/actions/prepare-docker-binaries/action.yaml` (22 lines changed).
- Docs: `docs/src/concepts/spot-schedule.md` (93 lines, new),
  `docs/src/guides/{time-based-schedule,capital-markets-schedule,create-your-own-provider}.md`
  (new, 121/105/258 lines), `docs/src/reference/spot-schedule-contract.md`
  (169 lines, new — the duck-typed provider contract spec).
- Tests: `src/reconcilers/spot_schedule_tests.rs` (286 lines),
  `spot_schedule_watch_tests.rs` (266 lines), `providers/{capital_markets,time_based}_tests.rs`,
  `tests/integration_spot_schedule.rs` (233 lines).

### 3. CAPI bootstrap/infrastructure multi-version passthrough (`fff03cb`)

- Hardcoded `CAPI_MACHINE_API_VERSION`/`_FULL` constants removed.
- `resolve_capi_machine_version()` in `src/main.rs`: env override →
  `kube::discovery::group()` lookup (6 retries × 5s backoff) → hard error
  (controller restarts). Cached in a `OnceLock`, read via
  `capi_machine_api_version()` (panics pre-resolution — enforced startup
  ordering). Supports CAPI serving `v1beta1` (≤ v1.10) or `v1beta2` (v1.11+)
  with the same binary.
- `machine_api_version_for` in `helpers.rs` derives the version to *create*
  from the user's `bootstrapSpec.apiVersion`, falling back to the discovered
  served version — per-`ScheduledMachine` version pinning independent of
  cluster default.

### 4. Security / dependency fixes

- `10efb01` — rustls switched to the `aws-lc-rs` crypto provider feature.
- `9d42851` — **reverted** the mTLS/resumption change from the prior commit;
  the real root causes (fixed directly, per changelog) were (a) a missing
  `apiVersion`/`kind` type-meta on an SSA taint patch (silently returning 400,
  taint never applied) and (b) `deploy/deployment/networkpolicy.yaml` egress
  only permitting ports 53/6443 while k0smotron's hosted control plane listens
  on NodePort 30443 by default. The TLS angle was a misdiagnosis.
- `49d87bb` — anyhow 1.0.102 → 1.0.103, clears RUSTSEC-2026-0190.
- RUSTSEC-2026-0173 (`proc-macro-error2`, unreachable via jiff's unused
  `defmt` feature) suppressed in `.cargo`/`deny.toml` with written justification.
- `.trivyignore` (83 lines, new) — `AVD-KSV-0106` (privileged makes cap-drop
  moot) and `AVD-KSV-0113` (Secret `get`-only, no list/watch) for the new
  privileged kata agent.
- ADR 0008 Auto-VEX gate: `.vex/snapshot/` (frozen grype/SBOM inputs) +
  `.vex/auto/` (generated VEX docs) committed; `make vex-auto` /
  `vex-auto-check` Makefile targets; `docs/security/vex.md` (57 lines, new).

### 5. Code quality / idempotency / constants sweep (`c720dde`, `dedef25`)

- `error_chain(&dyn Error)` helper flattens `source()` chains; wired into six
  child-cluster failure log sites.
- `create_dynamic_resource` now treats 409 `AlreadyExists` as success
  (idempotent create); `add_finalizer` no longer appends duplicates.
- Named constants `HTTP_ALREADY_EXISTS`/`HTTP_NOT_FOUND`/`HTTP_TOO_MANY_REQUESTS`
  in `src/constants.rs`, derived from `http::StatusCode` (promoted `http` from
  dev-dependency to a real dependency); replaced 9 raw numeric match guards.
- `src/labels.rs`'s inline `mod tests` moved to `src/labels_tests.rs` per convention.
- `crdgen` reverted to stdout-only + `clap` arg parsing (Semgrep finding on raw `std::env::args`).

---

## Breaking / Contract Changes

- `ScheduledMachine`: `v1alpha1` → `v1beta1` (module dropped, not kept
  alongside — acceptable pre-release, no conversion webhook needed).
- `spec.schedule` changed twice: inline window → inline-or-reference (ADR
  0006) → **required reference only**, inline window and `spec.spotSchedule`
  both gone (ADR 0009). Old inline manifests no longer apply.
- New required `spec.enabled` (default `true`) replaces `spec.schedule.enabled`.
- `spec.kata.destPath` removed — rejected at admission if set
  (`deny_unknown_fields`).
- CAPI Machine API version is no longer a compile-time default — a cluster
  where CAPI isn't discoverable now hard-fails controller startup.
- New RBAC: `get;list;watch` on `spotschedules.5spot.finos.org/*`,
  `namespaces: get`; `deploy/deployment/rbac/clusterrole.yaml` (48 lines
  changed); two new node/provider-side ServiceAccounts + ClusterRoles per
  provider deployment.
- A minimal `ScheduledMachine` now needs a companion `TimeBasedSpotSchedule`
  object — no built-in default scheduler.
- `deploy/crds/{capitalmarketsschedule,timebasedspotschedule}.yaml` (new, 184/152
  lines); `deploy/crds/scheduledmachine.yaml` (204 lines changed).
- `deploy/admission/agent-pod-security-{binding,policy}.yaml` (new, ADR 0004);
  `deploy/admission/validatingadmissionpolicy.yaml` (62 lines changed).

---

## Infrastructure, Build & CI (missed in first pass — added on review)

- **Cargo.toml**: version `0.1.1` → `0.3.0` (crate version had drifted behind
  the git tag lineage; now caught up). New deps: `http = "1"` (promoted from
  dev-only), `sha2 = "0.11"` (kata content/drift hashing). New `[[bin]]`
  entries: `5spot-kata-config-agent`, `spot-schedule-time-based`,
  `spot-schedule-capital-markets` — declared explicitly so binary names are
  hyphenated to match `command:` in deploy manifests rather than Cargo's
  auto-discovered underscore names. `Cargo.lock`: 546 lines churned
  (transitive updates from the above + routine bumps).
- **Makefile** (116 lines changed): new targets `set-image-version` (pin
  deploy/ image tags to `VERSION`), `vex-auto` / `vex-auto-check` (ADR 0008 gate).
- **Dockerfile / Dockerfile.chainguard**: `COPY` lines added for the two new
  provider binaries.
- **.github/workflows/build.yaml** (91 lines): binary-copy fix for the new
  provider binaries feeding into the image; smaller tweaks to
  `calm-test.yaml`, `calm.yaml`, `codeql.yaml`, `docs.yaml`, `fuzz.yaml`,
  `sast.yaml`, `scorecard.yaml` (version bumps / config, not deep-dived — low
  risk). No new workflow file was added for the Auto-VEX gate; it rides
  existing CI plumbing via the new Makefile targets.
- **Docs consolidation**: top-level `docs/reference/api.md` (195 lines)
  **deleted** — superseded by the maintained mdbook copy at
  `docs/src/reference/api.md` (104 lines changed), removing a stale duplicate.
  `docs/mkdocs.yml` nav updated (10 lines) for the new pages.
- **Full mdbook doc sweep**: beyond the new concept/guide pages already listed
  under the feature streams above, existing pages were updated for the new
  contracts — `docs/src/concepts/{emergency-reclaim,machine-lifecycle,
  scheduled-machine,schedules,child-cluster-kubeconfig}.md`,
  `docs/src/operations/{configuration,monitoring,troubleshooting}.md`,
  `docs/src/installation/{controller,crds,quickstart}.md`,
  `docs/src/security/{admission-validation,crd-attack-surface,index,threat-model}.md`,
  `docs/src/advanced/capi-integration.md`, `docs/src/index.md`.
- **Examples**: all pre-existing `examples/scheduledmachine-*.yaml` updated for
  the new required `spec.schedule` reference + `spec.enabled` shape; new
  `examples/{capitalmarketsschedule,timebasedspotschedule,
  scheduledmachine-spot-schedule}.yaml`; `examples/workshop/` updated to match.

---

## Routine Maintenance

Five dependency/CI-bump commits with no architectural content: `ebf6e38` /
`afca11d` (actions-routine group GitHub Actions bumps), `c5b9ecc`
(codeql-action bump), `97ab81d` (cargo-deny-action 2.0.19→2.0.20), `132da7c`
(cargo patch/minor group, 3 updates), `96d1bec` (sha2 0.10.9→0.11.0 — later
consumed for real by the kata agent, see above).

---

## Full Commit List

```
49d87bb RUSTSEC-2026-0190 is cleared by the anyhow 1.0.102 → 1.0.103 bump (#100)
9d42851 Remove the mTLS check and resumption change, as this was a red herring (#99)
10efb01 Use aws-lc-rs feature in rusttls (#94)
c720dde The magic 409 I'd added (and the pre-existing 404/429 guards, which violated the same rule) are now named constants in constants.rs, sourced from the canonical HTTP definitions so no numeral is hardcoded anywhere in 5-Spot source
dedef25 New error_chain() helper flattens the error source() chain. Idempotency sweep across the code base to make sure 409 tend to OK. (#93)
a34abfa Make sure binary for the spotSchedule controller is added to 5-spot image (#92)
ebf6e38 ci(deps): bump the actions-routine group with 2 updates (#91)
fff03cb Add support for different versions of CAPI bootstrap and infrastructure specs, passthrough (#88)
e530266 Move to using providers instead of inline time based schdules, this will solidify the provider contract as a first class API (#87)  [tag: v0.3.0]
2add878 SpotSchedule: Add new 'capital markets' provider and remaining work for docs (#86)
96d1bec chore(deps): bump sha2 from 0.10.9 to 0.11.0 (#84)
9e882e1 Phase 2-4: true event-driven watch, a ScheduledMachine.spec.spotSchedule reference now actually drives the machine's active/inactive decision (pull-on-reconcile) (#83)
883db3b Phase 1: add spotSchedule to CRD and move to v1beta1 (#82)
6733d81 Phase 0 of the new SpotSchedule feature: ADR and CALM diagram update. (#81)
020d39f Second half of the kata config agent changes (#79)  [tag: v0.2.3]
50f14d6 Initial phase 1-3 of a per-node Kata config delivery (#78)
c5b9ecc ci(deps): bump github/codeql-action in the actions-routine group (#76)
132da7c chore(deps): bump the cargo-patch-and-minor group with 3 updates (#73)
afca11d ci(deps): bump the actions-routine group with 2 updates (#74)
97ab81d ci(deps): bump EmbarkStudios/cargo-deny-action from 2.0.19 to 2.0.20 (#75)
```

---

## Open questions / things worth double-checking directly (not fully verified here)

- **Cargo.lock churn (546 lines)** was not itemized dependency-by-dependency —
  worth a `cargo tree` diff if a supply-chain review is needed.
- **CI workflow tweaks** in `calm-test.yaml`, `calm.yaml`, `codeql.yaml`,
  `docs.yaml`, `fuzz.yaml`, `sast.yaml`, `scorecard.yaml` were noted by line
  count only, not read line-by-line.
- **`deploy/crds/scheduledmachine.yaml` (204 lines) and `capitalmarketsschedule
  /timebasedspotschedule.yaml`** should be diffed against `src/crd.rs` output
  via the `verify-crd-sync` skill to confirm they're current (project CLAUDE.md
  mandates this before investigating any k8s-related issue).
- No CHANGELOG/ADR evidence of a **conversion webhook** — per ADR 0007/0009,
  multi-version support today relies on `conversion.strategy: None` (additive
  fields only); a genuinely breaking field change post-1.0 would need a new ADR.
