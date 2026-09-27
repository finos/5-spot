<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 01 — Locked design decisions

> Every decision in this file is **locked**. Do not re-litigate during
> implementation. If a decision needs to change, write a superseding ADR in
> `docs/adr/NNNN-title.md` (Status / Context / Decision / Consequences), get
> it merged, then update the entry here to point at it.
>
> **`docs/adr/` is the canonical decision log** — unlike its banlieue
> counterpart, this file postdates the ADR sequence and is a quick-reference
> digest of it. Every entry cites the ADR or binding rule it summarizes; if
> this file and an ADR ever disagree, the ADR wins and this file is the bug.

## D-001 — Language and toolchain

- **Rust**, edition **2021**, MSRV **1.90** (root `Cargo.toml`).
- One crate, `five_spot`, with explicit `[[bin]]` targets — controller
  (`5spot`), node agents (`5spot-kata-config-agent`, `5spot-reclaim-agent`),
  first-party providers (`spot-schedule-time-based`,
  `spot-schedule-capital-markets`), generators (`crdgen`, `crddoc`) and the
  auto-VEX tools. Provider binary names are hyphenated to match their deploy
  manifests' `command:`.
- `rustfmt` defaults; `clippy` clean with `-D warnings` (`cargo-quality`
  skill is the gate).

## D-002 — Operator framework

- **`kube-rs`** (`kube` 4.x: `derive`, `client`, `runtime`, `ws`, plus
  `unstable-runtime` for `Controller::reconcile_on` — the canonical way to
  feed child-cluster Node-watch events into the controller loop) with
  `k8s-openapi` 0.28 and `tokio`.
- Lease handling via `kube-lease-manager`.
- No higher-level operator frameworks.

## D-003 — Methodology: Architecture Driven Development

- `ADR → CALM → TDD → implement → docs`, in that order, for anything
  architecturally significant
  ([ADR 0001](../../docs/adr/0001-adopt-architecture-driven-development.md),
  `.claude/rules/architecture-driven-development.md`).
- ADRs and CALM diagrams are first-class deliverables, equal to code and
  tests. Process-only decisions get an ADR that states "no CALM impact".

## D-004 — API groups

- `5spot.finos.org/v1beta1` — `ScheduledMachine`, the user-facing CR.
- `spotschedules.5spot.finos.org/v1alpha1` — the spot-schedule provider
  contract group (`TimeBasedSpotSchedule`, `CapitalMarketsSchedule`, and any
  third-party provider CRD).

## D-005 — Activation is a provider reference, not an inline window

- `spec.schedule` is a **required** `SpotScheduleRef` to a provider CR;
  the inline `daysOfWeek`/`hoursOfDay` window is gone, and `spec.enabled`
  is a top-level field
  ([ADR 0006](../../docs/adr/0006-pluggable-spot-schedule-provider-contract.md),
  [ADR 0009](../../docs/adr/0009-unify-schedule-as-provider-reference.md)).
- The contract is **duck-typed `status.active`**, resolved through an
  event-driven dynamic watch; on provider failure the controller
  **holds the last known state** rather than guessing.
- The first-party providers ship in the main 5-Spot image (D-001).

## D-006 — CAPI interaction

- The CAPI `Machine` API version is **discovered at runtime** — never
  hardcoded (roadmap 03, "fully dynamic" arc).
- All Kubernetes API calls carry timeout and retry logic; reconciliation is
  idempotent (409/finalizer handling was an explicit hardening pass).

## D-007 — Kata config delivery: resolve in the controller, write in the agent

- The controller only *resolves* a pre-existing ConfigMap/Secret and opts the
  Node in via label + annotation; it writes nothing to hosts
  ([ADR 0002](../../docs/adr/0002-kata-config-delivery-via-spec-kata.md)).
- The privileged `5spot-kata-config-agent` DaemonSet (`hostPID`,
  `privileged: true`) performs the host write and restarts k0s via
  `nsenter -t 1`
  ([ADR 0003](../../docs/adr/0003-in-pod-host-service-restart-via-nsenter.md)).
- The destination is the **compile-time constant**
  `/etc/k0s/containerd.d/kata.toml`; `spec.kata.destPath` was removed to
  close an arbitrary-host-write vector
  ([ADR 0005](../../docs/adr/0005-remove-kata-destpath-fixed-host-path.md)).
  Reopening any user-configurable host path requires a superseding ADR.

## D-008 — Admission posture

- The pod-security exception boundary for the two privileged agents is a
  **deny-by-default `ValidatingAdmissionPolicy`** allowlisting exactly their
  documented posture
  ([ADR 0004](../../docs/adr/0004-agent-pod-security-exception-boundary-vap.md)).
- No conversion webhooks today: CRD versions are served side by side with
  `conversion.strategy: None`
  ([ADR 0007](../../docs/adr/0007-crd-multi-version-and-conversion.md)); see
  D-009.

## D-009 — CRD evolution

- **Multi-version, additive-only**: multiple served versions, fields only
  added, never repurposed
  ([ADR 0007](../../docs/adr/0007-crd-multi-version-and-conversion.md), as
  amended by ADR 0009).
- The first genuinely breaking change triggers a conversion-webhook ADR
  *before* it lands — that path is anticipated (`v1alpha1`/`v1beta1`/`v1`
  serving is a standing design concern for both API groups), not improvised.
- `src/crd.rs` is the source of truth; `deploy/crds/*.yaml` is generated
  (`regen-crds` skill), examples updated, `crddoc` regenerated **last**.

## D-010 — Supply chain

- **Auto-VEX presubmission gate**: machine-authored CVE suppressions live as
  committed snapshots (`.vex/`), and a byte-exact CI diff forces human
  sign-off on every change
  ([ADR 0008](../../docs/adr/0008-autovex-presubmission-gate.md)).
- **Base-image digests are pinned on literal `FROM` lines** so Dependabot's
  Docker parser can re-pin them
  ([ADR 0010](../../docs/adr/0010-base-image-pins-on-from-line.md));
  `Dockerfile.chainguard` is the alternate base.
- Every commit is signed off (DCO) and GPG-signed.

## D-011 — Error handling

- Typed errors via `thiserror` in library code; `anyhow` only in binaries.
- **Never `unwrap()` in production code paths**; never ignore errors in
  finalizers — that blocks resource deletion.

## D-012 — Logging and observability

- `tracing` for all logs and spans (`tracing-subscriber` with env-filter and
  JSON output); never `println!` or `log`.
- Prometheus metrics via the **text exposition format only** — the
  `prometheus` crate's `protobuf` default feature is deliberately disabled.
- Health/readiness and metrics HTTP servers run on `warp`
  (`features = ["server"]` only).

## D-013 — Test strategy

- **TDD is mandatory** — failing test first, then the minimum implementation
  (`tdd-workflow` skill).
- **Unit tests live in separate `_tests.rs` files, never inline** — the
  source file carries only
  `#[cfg(test)] #[path = "foo_tests.rs"] mod tests;`. No exceptions, any
  target.
- Coverage bar: **every function, public and private**, with happy-path,
  negative, and error-path cases.
- Integration tests in `tests/` mock external services; tests needing a real
  docker daemon are left to CI.

## D-014 — Public-repo hygiene

- **No real infrastructure identifiers** — hostnames, IPs, registry paths,
  cluster names (`.claude/rules/no-real-infrastructure.md`; placeholders are
  the RFC 2606 / RFC 5737 families).
- **No home directories or PII, ever** — the scope is open-ended
  (`.claude/rules/no-pii.md`).
- Unremediated security findings go through private vulnerability reporting
  per `SECURITY.md`; the published posture is
  `docs/src/security/threat-model.md`.

## Open decisions

| ID | Topic | Where it stands |
|---|---|---|
| ~~O-001~~ | ~~Remove `hyper` and `tower` from `Cargo.toml`~~ | **Closed 2026-09-27** — both removed (and `http-body-util` moved to dev-dependencies), full suite green; roadmap [04](04-dependency-internalization-matrix.md) closed with it |
| O-002 | Conversion webhook shape | Deferred by [ADR 0007](../../docs/adr/0007-crd-multi-version-and-conversion.md) until the first true breaking CRD change |
| O-003 | Internalizing the "thin" dependency tier | Roadmap [04](04-dependency-internalization-matrix.md) §3 — feasible but mostly bad trades; decide per crate, with an ADR if one is taken |
