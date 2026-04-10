# 5-Spot Machine Scheduler - Project Roadmap

**Date:** 2026-03-21  
**Last Updated:** 2026-04-10  
**Status:** Active — Phase 1 & 2 complete, Phase 3 hardening next  
**Author:** Erick Bourgeois

---

## Executive Summary

5-Spot is a Kubernetes controller for time-based machine scheduling with Cluster API (CAPI) integration. This roadmap outlines the development plan to bring the project to production readiness and add advanced features.

---

## Current State Assessment

### ✅ Implemented Features

| Feature | Status | Notes |
|---------|--------|-------|
| Core CRD (`ScheduledMachine`) | ✅ Complete | CAPI-based scheduling |
| Time-based scheduling | ✅ Complete | Days/hours + timezone support |
| Reconciliation controller | ✅ Complete | Event-driven with kube-rs |
| Priority-based distribution | ✅ Complete | Multi-instance support |
| Kill switch | ✅ Complete | Emergency immediate removal |
| Graceful shutdown | ✅ Complete | Configurable timeout |
| Node draining | ✅ Complete | Per-pod timeout, PDB-aware (429 → error) |
| CRD code generation | ✅ Complete | `crdgen` binary |
| API documentation generation | ✅ Complete | `crddoc` binary |
| Health/readiness endpoints | ✅ Complete | `/healthz`, `/readyz` — `src/health.rs` |
| Prometheus metrics | ✅ Complete | 8 metrics — `src/metrics.rs` |
| Structured JSON logging | ✅ Complete | `tracing` + `RUST_LOG_FORMAT=json` default |
| Reconciliation correlation IDs | ✅ Complete | `reconcile_id` span field on every log event |
| Kubernetes Event recording | ✅ Complete | All phase transitions emit Events |
| Exponential back-off | ✅ Complete | 30→60→120→240→300 s cap, resets on success |
| Leader election | ✅ Complete | `kube-lease-manager`, `ENABLE_LEADER_ELECTION` CLI flag |
| Admission validation | ✅ Complete | `ValidatingAdmissionPolicy` — `deploy/admission/` |
| NetworkPolicy | ✅ Complete | Egress: DNS + k8s API; ingress: monitoring namespace only |
| Deployment manifests | ✅ Complete | `deploy/deployment/` — RBAC, ServiceAccount, NetworkPolicy |
| CI/CD pipeline | ✅ Complete | Single `build.yaml` — native cargo, amd64 + arm64, all triggers (PR/main/release) — consolidated 2026-04-10 |
| SBOM generation in CI | ✅ Complete | `cargo-cyclonedx` in every build job |
| Container image signing | ✅ Complete | Cosign keyless on main + release; GitHub artifact attestation (`gh attestation verify`) on every build — 2026-04-10 |
| Documentation site | ✅ Complete | MkDocs + GitHub Pages, auto-deployed on every main push (`docs.yaml`) — added 2026-04-10 |
| SPDX license headers | ✅ Complete | Apache-2.0 on all `src/**/*.rs` files |
| Unit tests | ✅ Complete | 161 tests across 6 test files |

### 🚧 In Progress / Open

| Feature | Status | Notes |
|---------|--------|-------|
| Integration tests | 🚧 Not started | Requires `kind` cluster — P3-7 |
| `observed_generation` | 🚧 Not started | CRD field defined, reconciler does not populate — P3-2 |
| `cargo deny` | 🚧 Not started | No `deny.toml` — P3-5 |
| PrometheusRule alerts | 🚧 Not started | No `deploy/monitoring/alerts.yaml` — P3-6 |
| Secret reference resolution | 🚧 Not started | Bootstrap specs cannot safely embed credentials — P3-1 |

### ❌ Not Yet Planned

| Feature | Priority | Notes |
|---------|----------|-------|
| Helm chart | Medium | Manual manifest deployment works; Helm adds packaging |
| Cron expression scheduling | Medium | Spec field exists but returns error when used |
| Multi-cluster support | Low | Cross-cluster `ClusterRef` in spec |
| Notification integration | Low | Slack/Teams/webhook callbacks |
| State machine enforcement | Medium | Invalid phase transitions not blocked — P3-3 |
| Fuzzing | Low | `cargo-fuzz` for parsers — P3-8 |
| Multi-tenancy | Low | Namespace-scoped controller mode |

### 🔴 Current Limitations

| Limitation | Impact | Priority | Notes |
|------------|--------|----------|-------|
| Cron expressions not implemented | Medium | Medium | Spec field exists but returns error when used |
| `observed_generation` not set | Low | Medium | Status field defined but reconciler never populates it |
| No startup validation | Low | Low | Port ranges, instance count ≥ 1 not validated at boot |
| Logs ephemeral (NIST AU-9) | High | Medium | No persistent log sink — requires Fluentd/Vector → immutable store |

---

## Phase 1: Production Readiness ✅ COMPLETE (2026-04-08)

**Goal:** Make 5-Spot deployable to production Kubernetes clusters

### 1.1 Health & Readiness Endpoints ✅

- [x] Implement `/healthz` endpoint (`src/health.rs`)
- [x] Implement `/readyz` endpoint with dependency checks
- [x] Add Kubernetes liveness/readiness probes to deployment manifests

### 1.2 Prometheus Metrics ✅

- [x] Add `prometheus` crate with process metrics
- [x] Implement reconciliation metrics:
  - `fivespot_reconciliations_total{phase,result}`
  - `fivespot_reconciliation_duration_seconds`
  - `fivespot_machines_active`
  - `fivespot_schedule_evaluations_total`
  - `fivespot_kill_switch_activations_total`
  - `fivespot_node_drains_total`
  - `fivespot_pod_evictions_total`
  - `fivespot_reconciliation_errors_total`
- [x] Implement `/metrics` endpoint (port 8080)
- [ ] Add Grafana dashboard JSON (deferred — low priority)

### 1.3 Deployment Manifests ✅

- [x] Complete `deploy/deployment/` manifests (Deployment, ServiceAccount, RBAC, Namespace, Service)
- [x] Add NetworkPolicy (egress: DNS + k8s API; ingress: monitoring only)
- [ ] Add PodDisruptionBudget (deferred — not critical for single-replica mode)

### 1.4 Helm Chart ⏳ Deferred

- [ ] Create `charts/5spot/` Helm chart — deferred; manual manifests work for current scale

### 1.5 CI/CD Pipeline ✅

- [x] GitHub Actions — consolidated into single `build.yaml` (2026-04-10; replaces pr.yaml + main.yaml + release.yaml):
  - fmt + clippy + test on every PR
  - Native `cargo build --release` — amd64 on `ubuntu-latest`, arm64 on `ubuntu-24.04-arm`; `CARGO_TARGET_*_LINKER=cc` prevents `.cargo/config.toml` cross-compiler override on Linux CI
  - Docker multi-arch build (amd64/arm64)
  - Security scanning (`cargo audit` + Trivy)
  - SBOM generation (`cargo-cyclonedx` in every build job)
  - Apache-2.0 license header check
  - GitHub artifact attestation (`actions/attest-build-provenance@v2`) on every docker build — `gh attestation verify` queryable (2026-04-10)
- [x] Release automation: Cosign signing extended to main branch (staging images now signed); SLSA L3 provenance; release asset upload (2026-04-10)
- [x] Documentation site: MkDocs built and published to GitHub Pages on every main push via `docs.yaml` (2026-04-10)

---

## Phase 2: Compliance Hardening ✅ COMPLETE (2026-04-09)

**Goal:** SOX/Basel III/NIST compliance for pre-GA regulated environment deployment

### 2.1 Audit Logging ✅

- [x] Kubernetes Event recording on all phase transitions (TDD — 6 mock API tests)
- [x] Before/after state logging on `update_phase()` (TDD — 10 pure fn tests)
- [x] `reconcile_id` correlation ID on every reconciliation span (TDD — 5 tests)
- [x] JSON structured logging default in production (`RUST_LOG_FORMAT=json`)

### 2.2 Error Handling & Resilience ✅

- [x] Eliminate all `unwrap()` / `expect()` panics from production code
- [x] Apply `K8S_API_TIMEOUT_SECS` to kube client
- [x] Fix 429 PDB-blocked eviction to propagate error (TDD — 5 mock API tests)
- [x] Bounded exponential back-off: 30→60→120→240→300 s cap (TDD — 5 tests)
- [x] Per-resource retry counter, resets on success

### 2.3 Data Integrity ✅

- [x] Leader election via `kube-lease-manager` — `ENABLE_LEADER_ELECTION` CLI flag; non-leaders return `await_change()` (TDD — 2 tests)
- [x] `ValidatingAdmissionPolicy` — duration format, cron/XOR days-hours, provider allowlist, kind non-empty
- [x] CRD `Condition.status` enum constraint (`True`/`False`/`Unknown`) (TDD — 5 tests)

### 2.4 Quality Gates ✅

- [x] `cargo fmt` + `cargo clippy -D warnings` enforced in CI
- [x] SPDX Apache-2.0 headers on all source files; CI license-check aligned
- [x] Signed commits verified on every PR and push
- [ ] `cargo deny` — not yet configured (P3-5)
- [ ] Code coverage measurement (no tarpaulin/llvm-cov)

## Phase 3: Hardening (Priority: Medium) — 🔄 Not started

**Goal:** Fill remaining compliance gaps; production HA polish

### 3.1 Secret Reference Resolution (P3-1) — CRITICAL

- [ ] Implement `validate_secret_ref()` — check secret exists and is readable at admission
- [ ] Implement secret injection into bootstrap specs (reference, not inline value)
- [ ] Document hard constraint: inline credentials in bootstrap spec are rejected

### 3.2 `observed_generation` Tracking (P3-2)

- [ ] Reconciler sets `status.observedGeneration` to `metadata.generation` on each reconcile
- [ ] Status patch includes `observedGeneration` — enables `kubectl wait --for=jsonpath` patterns

### 3.3 State Machine Enforcement (P3-3)

- [ ] Define allowed phase transition table (Pending→Active, Active→ShuttingDown, etc.)
- [ ] Reconciler rejects invalid transitions with a typed error + Event

### 3.4 `cargo deny` Configuration (P3-5)

- [ ] `deny.toml` with license allowlist (Apache-2.0, MIT, BSD-3, ISC)
- [ ] Advisory deny list (RUSTSEC) integrated into CI

### 3.5 PrometheusRule Alerts (P3-6)

- [ ] `deploy/monitoring/alerts.yaml` — error rate, drain failures, phase stuck >1h

### 3.6 Integration Tests (P3-7)

- [ ] `kind`-based integration test framework in `tests/integration/`
- [ ] Machine lifecycle: Creation → Active → Inactive → Deletion
- [ ] Kill switch, graceful shutdown, multi-instance failover

### 3.7 Pod Anti-Affinity (P3-9)

- [ ] Change `preferredDuringSchedulingIgnoredDuringExecution` → `requiredDuringSchedulingIgnoredDuringExecution`

---

## Phase 4: Advanced Features (Priority: Low)

**Goal:** Enterprise-ready extensions beyond compliance baseline

### 4.1 Advanced Scheduling

- [ ] Cron-style schedule expressions (spec field exists but returns error when used)
- [ ] Blackout windows (holidays, maintenance)
- [ ] Schedule inheritance/templates
- [ ] Conditional schedules (based on external signals)

### 4.2 Multi-Cluster Support

- [ ] Add `ClusterRef` to `ScheduledMachineSpec`
- [ ] Support for remote kubeconfig secrets
- [ ] Cross-cluster machine management

### 4.3 Notification Integration

- [ ] Slack/Teams notifications for state changes
- [ ] Webhook callbacks for external systems

### 4.4 Helm Chart

- [ ] Create `charts/5spot/` Helm chart (ServiceMonitor, multi-replica values, RBAC toggles)

---

## Phase 5: Enterprise / Long-Term (Priority: Low)

### 5.1 Multi-Tenancy

- [ ] Namespace-scoped controller mode
- [ ] Resource quotas per namespace
- [ ] Tenant isolation

### 5.2 Persistent Audit Trail (NIST AU-9)

- [ ] Fluentd/Vector log pipeline → immutable object storage
- [ ] Kubernetes API server audit log forwarding → SIEM
- [ ] Actor attribution on ScheduledMachine mutations (SOX §302)

### 5.3 Cost Management

- [ ] Cost estimation integration
- [ ] Usage reporting / budget alerts

---

## Technical Debt & Ongoing Quality

### Code Quality

- [x] All magic numbers declared as named constants (`src/constants.rs`)
- [x] All public APIs have rustdoc comments
- [x] `async-trait` used for testable trait boundaries
- [ ] Code coverage measurement (`cargo-llvm-cov` or `cargo-tarpaulin`)
- [ ] Fuzzing for schedule/duration/timezone parsers (`cargo-fuzz`)

### Documentation

- [x] User guide (`docs/src/`)
- [x] Operations guide (configuration, monitoring, troubleshooting)
- [x] Security and admission validation docs
- [ ] Architecture diagrams (Mermaid)
- [ ] Contribution guide

### Performance

- [ ] Profile reconciliation loop under load
- [ ] Request batching for bulk machine operations

---

## Success Criteria

### Phase 1 ✅ Complete (2026-04-08)
- [x] Health checks pass (`/healthz`, `/readyz`)
- [x] Metrics visible in Prometheus (`/metrics` port 8080)
- [x] CI pipeline green (native cargo builds, fmt, clippy, test, security scan)
- [ ] Operator deploys successfully via Helm (deferred)

### Phase 2 ✅ Complete (2026-04-09)
- [x] All SOX/Basel III/NIST Phase 2 items resolved — 10/10 done
- [x] 161 unit tests with TDD coverage of all new code
- [x] No clippy warnings; Apache-2.0 SPDX headers everywhere
- [x] Leader election operational

### Phase 3 Complete When:
- [ ] Secret reference resolution implemented (P3-1)
- [ ] `observed_generation` populated by reconciler (P3-2)
- [ ] `cargo deny` passes in CI (P3-5)
- [ ] PrometheusRule alerts deployed (P3-6)
- [ ] Integration tests green in CI (P3-7)

---

## Next Steps (Phase 3 Recommended Order)

1. **P3-9** — Pod anti-affinity `required` (30 min, Basel III availability — quick win)
2. **P3-5** — `cargo deny` config (2h, NIST SA-4 — unblocks supply chain compliance)
3. **P3-2** — `observed_generation` (2h, NIST SI-7)
4. **P3-6** — PrometheusRule alerts (3h, Basel III monitoring)
5. **P3-3** — State machine enforcement (4h, NIST SI-7)
6. **P3-1** — Secret reference resolution (8h, SOX/NIST SC-28 — most complex)
7. **P3-7** — Integration tests with `kind` (12h — longest effort)

---

## Resources & References

- [kube-rs documentation](https://kube.rs/)
- [Cluster API documentation](https://cluster-api.sigs.k8s.io/)
- [Kubernetes Operator Patterns](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- [Prometheus Rust client](https://docs.rs/prometheus/)
