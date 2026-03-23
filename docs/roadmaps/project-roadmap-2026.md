# 5-Spot Machine Scheduler - Project Roadmap

**Date:** 2026-03-21
**Status:** Active
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
| CRD code generation | ✅ Complete | `crdgen` binary |
| API documentation generation | ✅ Complete | `crddoc` binary |
| Basic unit tests | ✅ Complete | Separate `*_tests.rs` files |

### 🚧 In Progress / Partial

| Feature | Status | Notes |
|---------|--------|-------|
| Health/metrics endpoints | 🚧 Partial | Port configuration done, needs implementation |
| Integration tests | 🚧 Partial | Test framework present |
| Production deployment | 🚧 Partial | CRDs ready, deployment manifests needed |

### ❌ Not Yet Implemented

| Feature | Priority | Notes |
|---------|----------|-------|
| Prometheus metrics | High | Controller metrics, schedule evaluation |
| Helm chart | High | Production deployment |
| Webhooks | Medium | Validation, mutation |
| Multi-cluster support | Medium | Cross-cluster scheduling |
| CI/CD pipeline | High | GitHub Actions |
| E2E tests | High | Kind-based testing |

---

## Phase 1: Production Readiness (Priority: Critical)

**Timeline:** 2-3 weeks
**Goal:** Make 5-Spot deployable to production Kubernetes clusters

### 1.1 Health & Readiness Endpoints

- [ ] Implement `/healthz` endpoint
- [ ] Implement `/readyz` endpoint with dependency checks
- [ ] Add Kubernetes liveness/readiness probes to deployment manifests

### 1.2 Prometheus Metrics

- [ ] Add metrics crate (`prometheus` or `opentelemetry`)
- [ ] Implement reconciliation metrics:
  - `fivespot_reconciliations_total{phase,result}`
  - `fivespot_reconciliation_duration_seconds`
  - `fivespot_machines_active`
  - `fivespot_machines_scheduled`
  - `fivespot_schedule_evaluations_total`
- [ ] Implement `/metrics` endpoint
- [ ] Add Grafana dashboard JSON

### 1.3 Deployment Manifests

- [ ] Complete `deploy/deployment/` manifests:
  - Deployment with resource limits
  - ServiceAccount with RBAC
  - ClusterRole/ClusterRoleBinding
  - Namespace
  - ConfigMap for configuration
  - Service for metrics scraping
- [ ] Add NetworkPolicy
- [ ] Add PodDisruptionBudget

### 1.4 Helm Chart

- [ ] Create `charts/5spot/` Helm chart
- [ ] Support for:
  - Multi-replica deployments
  - Custom resource limits
  - TLS configuration
  - Metrics ServiceMonitor (Prometheus Operator)
  - RBAC customization

### 1.5 CI/CD Pipeline

- [ ] GitHub Actions workflow:
  - Rust build (fmt, clippy, test)
  - CRD validation (schema sync check)
  - Docker multi-arch build (amd64/arm64)
  - Security scanning (cargo audit, trivy)
  - SBOM generation
- [ ] Release automation (semantic versioning)

---

## Phase 2: Testing & Quality (Priority: High)

**Timeline:** 2-3 weeks
**Goal:** Comprehensive test coverage and quality gates

### 2.1 Unit Test Completion

- [ ] Achieve 80%+ code coverage
- [ ] Test all schedule evaluation edge cases
- [ ] Test finalizer handling
- [ ] Test multi-instance distribution algorithm

### 2.2 Integration Tests

- [ ] Kind-based integration test framework
- [ ] Test complete machine lifecycle:
  - Creation → Active → Inactive → Deletion
  - Kill switch activation
  - Graceful shutdown
- [ ] Test schedule transitions at boundary times
- [ ] Test multi-instance failover

### 2.3 E2E Tests

- [ ] Makefile targets for E2E tests
- [ ] Test with mock CAPI provider
- [ ] Test with actual k0smotron (optional, manual)

### 2.4 Quality Gates

- [ ] Pre-commit hooks (cargo fmt, clippy)
- [ ] Branch protection rules
- [ ] Require passing tests for merge

---

## Phase 3: Advanced Features (Priority: Medium)

**Timeline:** 4-6 weeks
**Goal:** Enterprise-ready features

### 3.1 Admission Webhooks

- [ ] Validating webhook:
  - Schedule syntax validation
  - Timezone validation
  - Reference existence checks
- [ ] Mutating webhook:
  - Default values injection
  - Label standardization

### 3.2 Multi-Cluster Support

- [ ] Add `ClusterRef` to `ScheduledMachineSpec`
- [ ] Support for remote kubeconfig secrets
- [ ] Cross-cluster machine management

### 3.3 Advanced Scheduling

- [ ] Cron-style schedule expressions
- [ ] Blackout windows (holidays, maintenance)
- [ ] Schedule inheritance/templates
- [ ] Conditional schedules (based on external signals)

### 3.4 Notification Integration

- [ ] Slack/Teams notifications for state changes
- [ ] Email alerts for errors
- [ ] Webhook callbacks for external systems

---

## Phase 4: Enterprise Features (Priority: Low)

**Timeline:** 6+ weeks
**Goal:** Large-scale deployment features

### 4.1 Multi-Tenancy

- [ ] Namespace-scoped controller mode
- [ ] Resource quotas per namespace
- [ ] Tenant isolation

### 4.2 Audit Logging

- [ ] Structured audit events
- [ ] Integration with Kubernetes audit logs
- [ ] Compliance reporting

### 4.3 High Availability

- [ ] Leader election for single-writer operations
- [ ] Cross-AZ deployment support
- [ ] Disaster recovery procedures

### 4.4 Cost Management

- [ ] Cost estimation integration
- [ ] Usage reporting
- [ ] Budget alerts

---

## Technical Debt & Improvements

### Code Quality

- [ ] Review and enforce magic number constants
- [ ] Ensure all public APIs have rustdoc
- [ ] Add integration test helpers module
- [ ] Consider `async-trait` for trait async methods

### Documentation

- [ ] User guide with detailed examples
- [ ] Architecture diagrams (Mermaid)
- [ ] Troubleshooting guide
- [ ] FAQ section
- [ ] Contribution guide

### Performance

- [ ] Profile reconciliation loop
- [ ] Optimize Kubernetes API calls (informers)
- [ ] Add request batching for bulk operations

---

## Success Criteria

### Phase 1 Complete When:
- [ ] Operator deploys successfully via Helm
- [ ] Health checks pass
- [ ] Metrics visible in Prometheus
- [ ] CI pipeline green on main branch

### Phase 2 Complete When:
- [ ] 80%+ test coverage
- [ ] Integration tests pass in CI
- [ ] No critical clippy warnings
- [ ] Documentation builds without errors

### Phase 3 Complete When:
- [ ] Webhooks deployed and functional
- [ ] Multi-cluster demo works
- [ ] Advanced scheduling features documented

---

## Next Steps (Immediate Actions)

1. **Start with Phase 1.2** - Add Prometheus metrics (high impact, enables observability)
2. **Then Phase 1.5** - Set up CI/CD (enables quality gates)
3. **Then Phase 1.3** - Complete deployment manifests
4. **Then Phase 1.4** - Create Helm chart

---

## Resources & References

- [kube-rs documentation](https://kube.rs/)
- [Cluster API documentation](https://cluster-api.sigs.k8s.io/)
- [Kubernetes Operator Patterns](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- [Prometheus Rust client](https://docs.rs/prometheus/)
