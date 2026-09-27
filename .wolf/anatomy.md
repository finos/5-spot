# anatomy.md

> Auto-maintained by OpenWolf. Last scanned: 2026-09-27T03:48:27.348Z
> Files: 299 tracked | Anatomy hits: 0 | Misses: 0

> Project structure index. Auto-maintained by OpenWolf hooks and daemon.
> Run `openwolf scan` to generate, or wait for the first Claude Code session.
> Status: Pending initial scan

## ./

- `.gitignore` — Git ignore rules (~73 tok)
- `.trivyignore` — Trivy ignore list — 5-Spot Machine Scheduler (~4384 tok)
- `AGENTS.md` — AGENTS.md - AI Coding Agent Instructions (~1886 tok)
- `Cargo.toml` — Rust package manifest (~1239 tok)
- `CHANGELOG.md` — Change log (~3980 tok)
- `CLAUDE.md` — OpenWolf (~99 tok)
- `CONTRIBUTING.md` — 5 Spot Machine Scheduler Contribution and Governance Policies (~1641 tok)
- `Cross.toml` — Cross-compilation configuration (~71 tok)
- `deny.toml` — SPDX-License-Identifier: Apache-2.0 (~816 tok)
- `Dockerfile` — Docker container definition (~794 tok)
- `Dockerfile.chainguard` — SPDX-License-Identifier: Apache-2.0 (~989 tok)
- `LICENSE` — Project license (~3022 tok)
- `LICENSE.spdx` (~50 tok)
- `MAINTAINERS.md` — Maintainers (~138 tok)
- `Makefile` — Make build targets (~11335 tok)
- `NOTICE` (~65 tok)
- `osv-scanner.toml` — osv-scanner.toml — OSV-Scanner ignore list. (~518 tok)
- `README.md` — Project documentation (~4284 tok)
- `ROADMAPS.md` — Roadmaps (~920 tok)
- `SECURITY.md` — Security Policy (~825 tok)

## .claude/

- `CHANGELOG.md` — Changelog (~86344 tok)
- `CLAUDE.md` — Project Instructions for Claude Code (~5648 tok)
- `settings.json` (~88 tok)
- `settings.local.json` (~2279 tok)

## .claude/rules/

- `documentation.md` — Documentation Rules (~909 tok)
- `no-pii.md` — Never Commit Home Directories or PII (~1588 tok)

## .github/

- `CODE_OF_CONDUCT.md` — Code of Conduct for 5 Spot Machine Scheduler (~33 tok)
- `dco.yml` — SPDX-License-Identifier: Apache-2.0 (~70 tok)
- `dependabot.yml` — /*.yaml — those pin our OWN ghcr.io/finos/5-spot* images to (~1671 tok)
- `PULL_REQUEST_TEMPLATE.md` (~85 tok)

## .github/ISSUE_TEMPLATE/

- `bug_report.yml` — SPDX-License-Identifier: Apache-2.0 (~513 tok)
- `feature_request.yml` — SPDX-License-Identifier: Apache-2.0 (~400 tok)
- `meeting_minutes.yml` — SPDX-License-Identifier: Apache-2.0 (~513 tok)
- `support_question.yml` — SPDX-License-Identifier: Apache-2.0 (~178 tok)

## .github/actions/extract-version/

- `action.yml` — SPDX-License-Identifier: Apache-2.0 (~1163 tok)

## .github/actions/prepare-docker-binaries/

- `action.yaml` — SPDX-License-Identifier: Apache-2.0 (~850 tok)

## .github/codeql/

- `codeql-config.yml` — # MkDocs build output. (~186 tok)

## .github/community/

- `00-overview.md` — 00 — Roadmap overview (~1887 tok)
- `00-release-diff-v0-2-2-to-main.md` — 00 — Release diff: `v0.2.2` → `main` (~4629 tok)
- `01-decisions.md` — 01 — Locked design decisions (~1995 tok)
- `01-dependency-internalization-matrix.md` — 01 — Dependency internalization matrix (~2677 tok)
- `02-conventions.md` — 02 — Coding conventions (~1386 tok)
- `03-release-diff-v0-2-2-to-main.md` — 03 — Release diff: `v0.2.2` → `main` (~4629 tok)
- `04-dependency-internalization-matrix.md` — 04 — Dependency internalization matrix (~2677 tok)
- `README.md` — Project documentation (~745 tok)

## .github/scripts/

- `calm-args.bats` — SPDX-License-Identifier: Apache-2.0 (~1775 tok)
- `calm-args.sh` — SPDX-License-Identifier: Apache-2.0 (~788 tok)

## .github/workflows/

- `build.yaml` — SPDX-License-Identifier: Apache-2.0 (~17798 tok)
- `calm-test.yaml` — SPDX-License-Identifier: Apache-2.0 (~1533 tok)
- `CALM.md` — Reusable CALM workflow (~2589 tok)
- `calm.yaml` — SPDX-License-Identifier: Apache-2.0 (~2024 tok)
- `codeql.yaml` — /*.rs (beta in CodeQL; upstream (~866 tok)
- `dependabot-auto-merge.yaml` — SPDX-License-Identifier: Apache-2.0 (~3017 tok)
- `docs.yaml` — SPDX-License-Identifier: Apache-2.0 (~2126 tok)
- `fuzz.yaml` — /*.rs' (~920 tok)
- `sast.yaml` — SPDX-License-Identifier: Apache-2.0 (~589 tok)
- `scorecard.yaml` — SPDX-License-Identifier: Apache-2.0 (~1019 tok)

## .vex/

- `.affected-functions.json` (~681 tok)
- `.gitkeep` (~0 tok)
- `CVE-2019-1010022.json` (~212 tok)
- `CVE-2019-1010023.json` (~200 tok)
- `CVE-2019-1010024.json` (~205 tok)
- `CVE-2019-1010025.json` — Declares that (~208 tok)
- `CVE-2026-4437.json` (~210 tok)
- `CVE-2026-4438.json` (~203 tok)
- `GHSA-cq8v-f236-94qc.json` (~276 tok)
- `README.md` — Project documentation (~1964 tok)

## .vex/auto/

- `vex.auto-presence.json` (~64 tok)
- `vex.auto-reachability.json` (~66 tok)

## .vex/snapshot/

- `grype.json` (~6 tok)
- `README.md` — Project documentation (~792 tok)
- `sbom-bootstrap.json` (~7 tok)
- `symbols.txt` (~0 tok)
- `timestamp.txt` (~6 tok)

## deploy/admission/

- `agent-pod-security-binding.yaml` — ValidatingAdmissionPolicyBinding — activates 5spot-agent-pod-security, (~430 tok)
- `agent-pod-security-policy.yaml` — ValidatingAdmissionPolicy — pod-security exception boundary for the 5-Spot (~3432 tok)
- `child-cluster-kata-runtime-mutatingpolicy.yaml` — MutatingAdmissionPolicy for child-cluster Node registration. (~804 tok)
- `child-cluster-kata-runtime-mutatingpolicybinding.yaml` — MutatingAdmissionPolicyBinding — activates child-cluster-kata-runtime-label (~417 tok)
- `controller-deployment-binding.yaml` — ValidatingAdmissionPolicyBinding — activates the (~428 tok)
- `controller-deployment-policy.yaml` — ValidatingAdmissionPolicy for the 5-Spot controller Deployment. (~1113 tok)
- `validatingadmissionpolicy.yaml` — ValidatingAdmissionPolicy for ScheduledMachine resources. (~5109 tok)
- `validatingadmissionpolicybinding.yaml` — ValidatingAdmissionPolicyBinding — activates the scheduledmachine-validation (~369 tok)

## deploy/crds/

- `capitalmarketsschedule.yaml` — K8s CustomResourceDefinition: capitalmarketsschedules.spotschedules.5spot.finos.org (~2138 tok)
- `scheduledmachine.yaml` — K8s CustomResourceDefinition: scheduledmachines.5spot.finos.org (~9269 tok)
- `timebasedspotschedule.yaml` — K8s CustomResourceDefinition: timebasedspotschedules.spotschedules.5spot.finos.org (~1730 tok)

## deploy/deployment/

- `configmap.yaml` — 5-Spot Controller ConfigMap (~135 tok)
- `deployment.yaml` — 5-Spot Controller Deployment (~1506 tok)
- `namespace.yaml` — 5-Spot Controller Namespace (~52 tok)
- `networkpolicy.yaml` — SPDX-License-Identifier: Apache-2.0 (~672 tok)
- `pdb.yaml` — 5-Spot Controller PodDisruptionBudget (~120 tok)
- `service.yaml` — 5-Spot Controller Service (~228 tok)

## deploy/deployment/rbac/

- `clusterrole.yaml` — 5-Spot Controller ClusterRole (~1904 tok)
- `clusterrolebinding.yaml` — 5-Spot Controller ClusterRoleBinding (~118 tok)
- `serviceaccount.yaml` — 5-Spot Controller Service Account (~64 tok)

## deploy/kata-config-agent/

- `daemonset.yaml` — 5-Spot Kata Config Agent DaemonSet (workload cluster) (~1495 tok)
- `kustomization.yaml` — 5-Spot Kata Config Agent — Kustomization (~174 tok)
- `rbac.yaml` — 5-Spot Kata Config Agent — ServiceAccount + RBAC (workload cluster) (~946 tok)

## deploy/monitoring/

- `servicemonitor.yaml` — 5-Spot Controller ServiceMonitor (~160 tok)

## deploy/node-agent/

- `daemonset.yaml` — 5-Spot Reclaim Agent DaemonSet (~2688 tok)
- `kustomization.yaml` — 5-Spot Reclaim Agent — Kustomization (~155 tok)
- `rbac.yaml` — 5-Spot Reclaim Agent RBAC (~922 tok)
- `reclaim.toml.example` — 5-Spot Reclaim Agent — Example Configuration (~378 tok)

## deploy/spot-schedule-providers/capital-markets/

- `clusterrole.yaml` — Least-privilege ClusterRole for the CapitalMarketsSchedule provider. (~334 tok)
- `clusterrolebinding.yaml` — K8s ClusterRoleBinding: spot-schedule-capital-markets (~121 tok)
- `deployment.yaml` — CapitalMarketsSchedule reference spot-schedule provider (ADR 0006, Phase 5). (~784 tok)
- `kustomization.yaml` — CapitalMarketsSchedule reference spot-schedule provider (ADR 0006, Phase 5). (~162 tok)
- `serviceaccount.yaml` — ServiceAccount for the CapitalMarketsSchedule reference spot-schedule provider (~101 tok)

## deploy/spot-schedule-providers/time-based/

- `clusterrole.yaml` — Least-privilege ClusterRole for the TimeBasedSpotSchedule provider. (~330 tok)
- `clusterrolebinding.yaml` — K8s ClusterRoleBinding: spot-schedule-time-based (~117 tok)
- `deployment.yaml` — TimeBasedSpotSchedule core spot-schedule provider (ADR 0009). (~773 tok)
- `kustomization.yaml` — TimeBasedSpotSchedule core spot-schedule provider (ADR 0009). (~158 tok)
- `serviceaccount.yaml` — ServiceAccount for the TimeBasedSpotSchedule core spot-schedule provider (~95 tok)

## docs/

- `.gitignore` — Git ignore rules (~95 tok)
- `.python-version` (~2 tok)
- `mkdocs.yml` — SPDX-License-Identifier: Apache-2.0 (~2164 tok)
- `pyproject.toml` — Python project configuration (~258 tok)
- `README.md` — Project documentation (~457 tok)

## docs/adr/

- `0001-adopt-architecture-driven-development.md` — 0001 — Adopt Architecture Driven Development (ADD) (~1134 tok)
- `0002-kata-config-delivery-via-spec-kata.md` — 0002 — Kata config delivery via `spec.kata`: workload-cluster resolution, fail-fast on absence (~2698 tok)
- `0003-in-pod-host-service-restart-via-nsenter.md` — 0003 — In-pod host k0s-service restart via `nsenter` (privileged Kata-config agent) (~2566 tok)
- `0004-agent-pod-security-exception-boundary-vap.md` — 0004 — Agent pod-security exception boundary via deny-by-default ValidatingAdmissionPolicy (~1970 tok)
- `0005-remove-kata-destpath-fixed-host-path.md` — 0005 — Remove `spec.kata.destPath`; fix the host path to `/etc/k0s/containerd.d/kata.toml` (~1318 tok)
- `0006-pluggable-spot-schedule-provider-contract.md` — 0006 — Pluggable spot-schedule provider contract via `spec.spotSchedule` and the `spotschedules.5spot.finos.org` API group (~2516 tok)
- `0007-crd-multi-version-and-conversion.md` — 0007 — CRD multi-version support with `None` conversion and additive-only evolution (~1712 tok)
- `0008-autovex-presubmission-gate.md` — 0008 — Auto-VEX is generated and signed off before submission, enforced by a byte-exact CI gate (~1527 tok)
- `0009-unify-schedule-as-provider-reference.md` — 0009 — Unify activation under `spec.schedule` as a provider reference; ship `TimeBasedSpotSchedule` as the core provider (~2051 tok)
- `0010-base-image-pins-on-from-line.md` — 0010 — Base-image digests are pinned on the `FROM` line so Dependabot re-pins them (~1681 tok)
- `README.md` — Project documentation (~809 tok)
- `template.md` — NNNN — <short decision title> (~251 tok)

## docs/architecture/calm/

- `architecture.json` (~16044 tok)
- `README.md` — Project documentation (~701 tok)

## docs/architecture/calm/templates/mermaid/

- `flows.md.hbs` — Architecture Flows (~259 tok)
- `system.md.hbs` — System Architecture (~424 tok)

## docs/site/

- `404.html` — 5-Spot - Kubernetes Machine Scheduler Operator (~11206 tok)
- `index.html` — 5-Spot - Kubernetes Machine Scheduler Operator (~17319 tok)
- `sitemap.xml` (~1559 tok)

## docs/site/advanced/capi-integration/

- `index.html` — Integration with CAPI - 5-Spot - Kubernetes Machine Scheduler Operator (~19665 tok)

## docs/site/advanced/ha/

- `index.html` — High Availability - 5-Spot - Kubernetes Machine Scheduler Operator (~21194 tok)

## docs/site/advanced/resource-distribution/

- `index.html` — Resource Distribution - 5-Spot - Kubernetes Machine Scheduler Operator (~16390 tok)

## docs/site/architecture/flows/

- `index.html` — Architecture Flows - 5-Spot - Kubernetes Machine Scheduler Operator (~15647 tok)

## docs/site/architecture/system/

- `index.html` — System Diagram - 5-Spot - Kubernetes Machine Scheduler Operator (~13117 tok)

## docs/site/assets/javascripts/lunr/

- `tinyseg.js` — export the module via AMD, CommonJS or as a browser global (~5698 tok)
- `wordcut.js` — e: s (~110353 tok)

## docs/site/changelog/

- `index.html` — Changelog - 5-Spot - Kubernetes Machine Scheduler Operator (~13435 tok)

## docs/site/concepts/

- `index.html` — Overview - 5-Spot - Kubernetes Machine Scheduler Operator (~13373 tok)

## docs/site/concepts/architecture/

- `index.html` — Architecture - 5-Spot - Kubernetes Machine Scheduler Operator (~17571 tok)

## docs/site/concepts/child-cluster-kubeconfig/

- `index.html` — Child-cluster kubeconfig support - 5-Spot - Kubernetes Machine Scheduler Operator (~17527 tok)

## docs/site/concepts/emergency-reclaim/

- `index.html` — Emergency Reclaim (Kill Switch) - 5-Spot - Kubernetes Machine Scheduler Operator (~28963 tok)

## docs/site/concepts/kata-config-delivery/

- `index.html` — Kata Config Delivery - 5-Spot - Kubernetes Machine Scheduler Operator (~19811 tok)

## docs/site/concepts/machine-lifecycle/

- `index.html` — Machine Lifecycle - 5-Spot - Kubernetes Machine Scheduler Operator (~21591 tok)

## docs/site/concepts/scheduled-machine/

- `index.html` — ScheduledMachine - 5-Spot - Kubernetes Machine Scheduler Operator (~22753 tok)

## docs/site/concepts/schedules/

- `index.html` — Schedule Configuration - 5-Spot - Kubernetes Machine Scheduler Operator (~21228 tok)

## docs/site/concepts/spot-schedule/

- `index.html` — Spot Schedules (Providers) - 5-Spot - Kubernetes Machine Scheduler Operator (~14655 tok)

## docs/site/css/

- `timeago.css` — Styles: 2 rules, 1 media queries (~112 tok)

## docs/site/development/building/

- `index.html` — Building - 5-Spot - Kubernetes Machine Scheduler Operator (~20034 tok)

## docs/site/development/contributing/

- `index.html` — Contributing - 5-Spot - Kubernetes Machine Scheduler Operator (~20582 tok)

## docs/site/development/setup/

- `index.html` — Development Setup - 5-Spot - Kubernetes Machine Scheduler Operator (~21267 tok)

## docs/site/development/testing/

- `index.html` — Testing - 5-Spot - Kubernetes Machine Scheduler Operator (~22492 tok)

## docs/site/guides/capital-markets-schedule/

- `index.html` — CapitalMarketsSchedule - 5-Spot - Kubernetes Machine Scheduler Operator (~15504 tok)

## docs/site/guides/create-your-own-provider/

- `index.html` — Create Your Own Provider - 5-Spot - Kubernetes Machine Scheduler Operator (~23921 tok)

## docs/site/guides/kata-config/

- `index.html` — Kata Config Drop-In - 5-Spot - Kubernetes Machine Scheduler Operator (~17432 tok)

## docs/site/installation/controller/

- `index.html` — Deploying Operator - 5-Spot - Kubernetes Machine Scheduler Operator (~15856 tok)

## docs/site/installation/crds/

- `index.html` — Installing CRDs - 5-Spot - Kubernetes Machine Scheduler Operator (~14701 tok)

## docs/site/installation/prerequisites/

- `index.html` — Prerequisites - 5-Spot - Kubernetes Machine Scheduler Operator (~15531 tok)

## docs/site/installation/quickstart/

- `index.html` — Quick Start - 5-Spot - Kubernetes Machine Scheduler Operator (~16435 tok)

## docs/site/javascripts/

- `mermaid-init.js` — SPDX-License-Identifier: Apache-2.0 (~787 tok)

## docs/site/js/

- `timeago_mkdocs_material.js` — Script to ensure timeago keeps working when (~255 tok)

## docs/site/license/

- `index.html` — License - 5-Spot - Kubernetes Machine Scheduler Operator (~13630 tok)

## docs/site/operations/configuration/

- `index.html` — Configuration - 5-Spot - Kubernetes Machine Scheduler Operator (~25173 tok)

## docs/site/operations/monitoring/

- `index.html` — Monitoring - 5-Spot - Kubernetes Machine Scheduler Operator (~34954 tok)

## docs/site/operations/multi-instance/

- `index.html` — Multi-Instance - 5-Spot - Kubernetes Machine Scheduler Operator (~21410 tok)

## docs/site/operations/troubleshooting/

- `index.html` — Troubleshooting - 5-Spot - Kubernetes Machine Scheduler Operator (~34470 tok)

## docs/site/reference/api/

- `index.html` — API Reference - 5-Spot - Kubernetes Machine Scheduler Operator (~21214 tok)

## docs/site/reference/cli/

- `index.html` — CLI Reference - 5-Spot - Kubernetes Machine Scheduler Operator (~18232 tok)

## docs/site/reference/spot-schedule-contract/

- `index.html` — Spot Schedule Provider Contract - 5-Spot - Kubernetes Machine Scheduler Operator (~17511 tok)

## docs/site/search/

- `search_index.json` (~115778 tok)

## docs/site/security/

- `index.html` — Overview - 5-Spot - Kubernetes Machine Scheduler Operator (~13795 tok)

## docs/site/security/admission-validation/

- `index.html` — Admission Validation - 5-Spot - Kubernetes Machine Scheduler Operator (~33057 tok)

## docs/site/security/crd-attack-surface/

- `index.html` — CRD Attack Surface - 5-Spot - Kubernetes Machine Scheduler Operator (~15293 tok)

## docs/site/security/threat-model/

- `index.html` — Threat Model - 5-Spot - Kubernetes Machine Scheduler Operator (~26789 tok)

## docs/site/security/vex/

- `index.html` — VEX (Vulnerability Exploitability eXchange) - 5-Spot - Kubernetes Machine Scheduler Operator (~21209 tok)

## docs/site/stylesheets/

- `extra.css` — 5Spot Documentation - Custom Styles for MkDocs Material (~2132 tok)

## docs/src/

- `changelog.md` — Changelog (~294 tok)
- `index.md` — <img src="images/5-spot-icon.svg" alt="5-Spot Logo" width="60"  style="vertical-align: middle; margin-right: 5px;" /> Introduction (~1650 tok)
- `license.md` — License (~340 tok)

## docs/src/advanced/

- `capi-integration.md` — CAPI Integration (~1382 tok)
- `ha.md` — High Availability (~1224 tok)
- `resource-distribution.md` — Resource Distribution (~734 tok)

## docs/src/concepts/

- `architecture.md` — Architecture (~2342 tok)
- `child-cluster-kubeconfig.md` — Child-cluster kubeconfig support (~1778 tok)
- `emergency-reclaim.md` — Emergency Reclaim (Process-Match Kill Switch) (~6605 tok)
- `index.md` — Concepts Overview (~598 tok)
- `kata-config-delivery.md` — Kata Config Delivery (Per-Node containerd Drop-In) (~3644 tok)
- `machine-lifecycle.md` — Machine Lifecycle (~2032 tok)
- `scheduled-machine.md` — ScheduledMachine (~3627 tok)
- `schedules.md` — Schedule Configuration (~1627 tok)
- `spot-schedule.md` — Spot Schedules (pluggable providers) (~1148 tok)

## docs/src/development/

- `building.md` — Building (~946 tok)
- `contributing.md` — Contributing (~1109 tok)
- `setup.md` — Development Setup (~996 tok)
- `testing.md` — Testing (~976 tok)

## docs/src/guides/

- `capital-markets-schedule.md` — Guide: CapitalMarketsSchedule provider (~1035 tok)
- `create-your-own-provider.md` — Guide: Create your own spot-schedule provider (~2255 tok)
- `kata-config.md` — Delivering a Kata containerd Drop-In (~1731 tok)
- `time-based-schedule.md` — Guide: TimeBasedSpotSchedule provider (~1168 tok)

## docs/src/installation/

- `controller.md` — Deploying the Controller (~1055 tok)
- `crds.md` — Installing CRDs (~675 tok)
- `prerequisites.md` — Prerequisites (~675 tok)
- `quickstart.md` — Quick Start (~1139 tok)

## docs/src/javascripts/

- `mermaid-init.js` — SPDX-License-Identifier: Apache-2.0 (~787 tok)

## docs/src/operations/

- `configuration.md` — Configuration (~2887 tok)
- `monitoring.md` — Monitoring (~4350 tok)
- `multi-instance.md` — Multi-Instance Deployment (~1292 tok)
- `troubleshooting.md` — Troubleshooting (~6291 tok)

## docs/src/reference/

- `api.md` — 5Spot API Reference (~3376 tok)
- `cli.md` — CLI Reference (~1228 tok)
- `spot-schedule-contract.md` — Spot Schedule Provider Contract (~2056 tok)

## docs/src/security/

- `admission-validation.md` — Admission Validation (~6195 tok)
- `crd-attack-surface.md` — CRD Attack Surface (~1800 tok)
- `index.md` — Security (~844 tok)
- `threat-model.md` — Threat Model: 5-Spot ScheduledMachine Controller (~9650 tok)
- `vex.md` — VEX (Vulnerability Exploitability eXchange) (~3544 tok)

## docs/src/stylesheets/

- `extra.css` — 5Spot Documentation - Custom Styles for MkDocs Material (~2132 tok)

## examples/

- `capitalmarketsschedule.yaml` — Example CapitalMarketsSchedule — the reference spot-schedule provider (~500 tok)
- `scheduledmachine-bad-taint.yaml` — Example ScheduledMachine with INVALID nodeTaints — intentionally rejected (~548 tok)
- `scheduledmachine-basic.yaml` — Example ScheduledMachine resource (~617 tok)
- `scheduledmachine-child-cluster.yaml` — Example ScheduledMachine targeting a CAPI / k0smotron child cluster. (~680 tok)
- `scheduledmachine-kata.yaml` — Example ScheduledMachine with Kata config delivery (spec.kata) (~568 tok)
- `scheduledmachine-spot-schedule.yaml` — Example ScheduledMachine driven by the CapitalMarketsSchedule spot-schedule (~482 tok)
- `scheduledmachine-tainted.yaml` — Example ScheduledMachine with user-defined Node taints. (~549 tok)
- `scheduledmachine-weekend.yaml` — Example ScheduledMachine with a weekend schedule. (~400 tok)
- `timebasedspotschedule.yaml` — Example TimeBasedSpotSchedule objects — the core, first-party spot-schedule (~621 tok)

## examples/workshop/

- `kind-management.yaml` — Kind configuration for the 5-Spot WORKSHOP management cluster. (~215 tok)
- `README.md` — Project documentation (~3100 tok)
- `scheduledmachine-business-hours.yaml` — 5-Spot ScheduledMachine — a time-scheduled WORKER for the CAPD dev-cluster. (~1056 tok)
- `teardown.sh` — Tear down the 5-Spot workshop environment. (~348 tok)
- `workload-cluster.yaml` — Workshop WORKLOAD cluster — plain Cluster API with the Docker provider (CAPD). (~940 tok)

## fuzz/

- `.gitignore` — Git ignore rules (~46 tok)
- `Cargo.toml` — Rust package manifest (~248 tok)

## fuzz/corpus/parse_duration/

- `arabic_indic_zero_regression` (~1 tok)

## fuzz/fuzz_targets/

- `parse_day_ranges.rs` — SPDX-License-Identifier: Apache-2.0 (~184 tok)
- `parse_duration.rs` — SPDX-License-Identifier: Apache-2.0 (~122 tok)
- `parse_hour_ranges.rs` — SPDX-License-Identifier: Apache-2.0 (~148 tok)

## src/

- `auto_vex_presence_tests.rs` — Unit tests for the `auto_vex_presence` module. (~5243 tok)
- `auto_vex_presence.rs` — Presence-based auto-VEX generation (roadmap Phase 2). (~2298 tok)
- `auto_vex_reachability_tests.rs` — Unit tests for the `auto_vex_reachability` module. (~3528 tok)
- `auto_vex_reachability.rs` — Symbol-import-based auto-VEX (roadmap Phase 3). (~2302 tok)
- `constants.rs` — # Global constants (~11189 tok)
- `crd_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~20848 tok)
- `crd.rs` — # CRD type definitions (~18606 tok)
- `health_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~486 tok)
- `health.rs` — Health state shared across the application (~1465 tok)
- `kata_config_agent_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~6224 tok)
- `kata_config_agent.rs` — # Kata config agent — host-filesystem sync engine (~5467 tok)
- `labels_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~390 tok)
- `labels.rs` — # Standard Kubernetes labels (~1458 tok)
- `lib.rs` — # `five_spot` — library crate (~511 tok)
- `loop_protection_tests.rs` — Tests for the rapid-re-reclaim loop-protection helpers. (~2173 tok)
- `loop_protection.rs` — # Rapid-re-reclaim loop protection (~988 tok)
- `main_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~68 tok)
- `main.rs` — # 5-Spot Machine Scheduler — Entry Point (~6265 tok)
- `metrics_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~2368 tok)
- `metrics.rs` — # Prometheus metrics (~11109 tok)
- `netlink_proc_tests.rs` — Byte-level tests for the netlink proc connector parsers. (~3058 tok)
- `netlink_proc.rs` — # Netlink proc connector — rung 2 of the reclaim-agent detection ladder (~9728 tok)
- `reclaim_agent_tests.rs` — Write a fake `/proc/<pid>/comm` + `/proc/<pid>/cmdline` pair. (~7327 tok)
- `reclaim_agent.rs` — # Reclaim agent — process-match detector (~4722 tok)

## src/bin/

- `auto_vex_presence.rs` — # Presence-based auto-VEX generator (CI tool, roadmap Phase 2) (~1348 tok)
- `auto_vex_reachability.rs` — # Symbol-import reachability auto-VEX (CI tool, roadmap Phase 3) (~1478 tok)
- `crddoc.rs` — # CRD API documentation generator (~5826 tok)
- `crdgen.rs` — # CRD YAML generator (~934 tok)
- `kata_config_agent.rs` — # `5spot-kata-config-agent` (~3684 tok)
- `reclaim_agent.rs` — # 5spot-reclaim-agent — node-side emergency reclaim trigger (~6727 tok)
- `spot_schedule_capital_markets.rs` — # `spot-schedule-capital-markets` (~645 tok)
- `spot_schedule_time_based.rs` — # `spot-schedule-time-based` (~607 tok)

## src/providers/

- `capital_markets_tests.rs` — NYSE-like regular session: Mon–Fri, 09:00–15:00 local (hour 15 is the (~2193 tok)
- `capital_markets.rs` — # CapitalMarketsSchedule provider controller (ADR 0006, Phase 5) (~3448 tok)
- `mod.rs` — # Spot-schedule providers (~237 tok)
- `time_based_tests.rs` — Weekdays 09:00–17:00 local (hour 17 is the last active hour). (~2375 tok)
- `time_based.rs` — # TimeBasedSpotSchedule provider controller (ADR 0009) (~3112 tok)

## src/reconcilers/

- `child_client_tests.rs` — Tests for the [`ChildClientCache`] resolver. Lock the contract for: (~8597 tok)
- `child_client.rs` — # Child-cluster client resolver (~6551 tok)
- `child_watch_tests.rs` — Tests for [`super::ChildNodeWatchManager`]. The actual `kube::runtime` (~3388 tok)
- `child_watch.rs` — # Per-child-cluster Node watcher manager (~2567 tok)
- `helpers_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~57946 tok)
- `helpers.rs` — # Reconciliation helper functions (~41295 tok)
- `mod.rs` — # Reconcilers (~536 tok)
- `scheduled_machine_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~15332 tok)
- `scheduled_machine.rs` — # `ScheduledMachine` reconciler (~17862 tok)
- `spot_schedule_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~2861 tok)
- `spot_schedule_watch_tests.rs` — SPDX-License-Identifier: Apache-2.0 (~2950 tok)
- `spot_schedule_watch.rs` — # Dynamic spot-schedule provider watch manager (ADR 0006, Phase 3) (~3775 tok)
- `spot_schedule.rs` — # Spot-schedule provider resolution (ADR 0006) (~2515 tok)

## tests/

- `integration_child_kubeconfig.rs` — # Integration test: child-cluster kubeconfig wiring (~3948 tok)
- `integration_emergency_reclaim.rs` — # Integration test: agent → controller annotation contract on a real cluster (~2447 tok)
- `integration_kata_config.rs` — # Integration test: kata-config delivery contract + host-file lifecycle (~3563 tok)
- `integration_netlink_proc.rs` — Linux runtime test for the netlink proc connector subscriber. (~1404 tok)
- `integration_node_taints.rs` — # Integration test: end-to-end node taint reconcile on a real cluster (~2993 tok)
- `integration_spot_schedule.rs` — # Integration test: spot-schedule provider contract (ADR 0006) (~2490 tok)
