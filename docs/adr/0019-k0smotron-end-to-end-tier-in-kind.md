<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0019: A k0smotron end-to-end tier in kind, with strict example validation

- **Status:** Accepted
- **Date:** 2026-10-13
- **Deciders:** Erick Bourgeois
- **Supersedes:** (none)
- **Related:** ADR [0017](./0017-machine-reference-shape-follows-capi-contract.md) (the defect this tier would have caught), roadmap [06](../../.github/community/06-k0smotron-e2e-and-worker-confinement.md), `rules/testing.md`, `.github/scripts/k0smotron-e2e.bats`, `.github/scripts/validate-examples.py`

## Context

5-Spot's tiers (`rules/testing.md`) are unit tests over pure functions,
integration tests against a mocked kube API, and kind targets that prove the
CRDs install, RBAC suffices and the reconciler runs. None of them runs the path
5-Spot exists for: a `ScheduledMachine` producing a real bootstrap object, a real
infrastructure object and a real CAPI `Machine`, which a real Cluster API turns
into a Node that joins a real child cluster, and then taking it away again.

The hand-run in roadmap 06 did exactly that once, on a production-shaped
management cluster with a VM worker, and found two defects that no lower tier
could reach:

- the `Machine`'s references used the `v1beta1` shape on a `v1beta2` contract
  (ADR 0017). The mocked API accepted it; Cluster API v1.14.3 rejected it.
- the controller's secondary watches selected on a label in a retired API
  group, so they matched nothing (bug-007). The mock had no watches to miss.

Both shipped. Rule 1 of `rules/testing.md` names the cause: a fake more
permissive than the real thing hides bugs. Nothing stops the next one.

A second gap surfaced while moving the examples to `v1beta2`. The only
validation the examples get is a manual `kubectl apply --dry-run`, and even a
server-side dry-run only checks the `ScheduledMachine` schema: `bootstrapSpec`
and `infrastructureSpec` are deliberately unstructured, so a field the provider
renamed between versions (k0smotron renamed `preStartCommands` to
`preK0sCommands` in `v1beta2`) passes validation and is silently dropped at
apply time.

Options weighed:

- **Mock Cluster API more faithfully.** Rejected: the defects lived precisely
  in the behaviour of the real CAPI webhooks and conversion, which a mock
  re-implements by hand and therefore gets wrong in the same places we do.
- **Run against a real VM or cloud host.** Rejected for CI: it needs
  credentials, infrastructure identifiers this public repository cannot carry
  (`rules/no-real-infrastructure.md`), and money. The manual run on real
  hardware stays the reference for the cgroup confinement work, which needs a
  real kernel and is out of scope here.
- **Use the CAPD (Docker) provider for the worker.** Rejected for this tier:
  5-Spot's primary path is k0smotron's SSH-provisioned `RemoteMachine`, and a
  kubeadm/CAPD run would not exercise the provider the two defects were found
  against.
- **Validate examples with an offline schema tool** (kubeconform with generated
  JSON schemas). Rejected: it needs a third-party schema catalogue for the
  provider CRDs, and it cannot run the CAPI and k0smotron conversion and
  validation webhooks, which are part of what "valid" means.

## Decision

We add a **kind-based k0smotron end-to-end tier**, run in CI and locally by the
same Makefile targets.

1. **Management cluster.** A kind cluster with cert-manager, Cluster API and
   k0smotron installed by `clusterctl` at **pinned versions** (Cluster API
   v1.14.3, k0smotron v2.2.0), and 5-Spot deployed from `deploy/`. kind
   (v0.33.0) and `clusterctl` are installed by `make e2e-tools`, checksum-
   verified, and the node image is pinned by digest (Kubernetes v1.37.0):
   Cluster API v1.14 needs a management cluster of v1.33 or later, which the
   repository's other kind targets (v1.31) do not provide.
2. **Child cluster.** A `Cluster` with a hosted `K0smotronControlPlane` (the
   control plane runs as pods in the management cluster) behind a `NodePort`,
   with `externalAddress` set to the kind node's address so the generated
   kubeconfig and the serving certificate agree with the address the worker and
   5-Spot use (roadmap 06: neither can be fixed after creation).
3. **Worker.** A container on the kind network running systemd and sshd, from
   `quay.io/k0sproject/bootloose-ubuntu22.04` **pinned by digest** (the image
   k0s uses for its own tests). k0smotron provisions it over SSH as a
   `RemoteMachine`, exactly as it would a physical host. The SSH key pair is
   generated per run and lives only in the throwaway cluster. The worker needs
   a **rootful** container engine (under a rootless one k0s cannot load its eBPF
   device filter or find the `cpuset` controller), a volume at `/var/lib/k0s`
   (containerd's overlay snapshots cannot sit on the container's own overlay
   root), and under podman no per-container pids cap (podman's default of 2048
   kills the hosted control plane); the harness sets the last two itself.
4. **The suite** (`.github/scripts/k0smotron-e2e.bats`) drives one
   `ScheduledMachine` through activate, deactivate and reactivate, and asserts
   the two contracts that escaped before: the `Machine` is created at
   `v1beta2` with `apiGroup` references (ADR 0017), proved from the Machine's
   `managedFields` because Cluster API's conversion would mask a read-back, and
   it carries the controller-owned `5spot.finos.org/scheduled-machine` label
   that the secondary watches select on (bug-007). It fails, never skips, when a prerequisite is
   missing.
5. **Strict example validation** (`.github/scripts/validate-examples.py`)
   runs on the same cluster: every `examples/**/*.yaml` document, every complete
   object in a fenced YAML block under `docs/src/`, **and every embedded
   `bootstrapSpec` / `infrastructureSpec` extracted as a standalone object**,
   goes through `kubectl apply --dry-run=server --validate=strict`, so an
   unknown or renamed provider field fails. An object marked
   `validate-examples: expect-reject` must be **rejected**, or the check fails;
   one marked `validate-examples: fragment` is listed with its reason. Anything
   else not validated is listed with a reason in the script, never skipped
   silently. The kubeadm and Docker providers are installed alongside
   k0smotron for this purpose only, so the workshop manifests are validated
   too.
6. **The image under test** is built from the checkout by `make kind-load` in
   CI. Locally, `E2E_IMAGE` may name a published image by digest instead, so the
   harness can run on a host that does not build images.
7. **When it runs:** on pull requests that touch `src/`, `deploy/`,
   `examples/`, the harness or its workflow; on pushes to `main`; and on
   `workflow_dispatch` / `workflow_call`.

## Consequences

**Easier.** The contract between 5-Spot and a real Cluster API is now tested
on every relevant change rather than discovered in a cluster. A provider schema
change that breaks an example fails CI instead of failing a reader. Adding a
provider version bump becomes a one-line pin change that the tier verifies.

**Harder.** The tier is slow (a fresh management cluster, providers, a child
control plane and a worker join: on the order of 15 minutes) and depends on
network fetches at pinned versions: `clusterctl` release manifests, provider
images, the bootloose image, and k0s binaries downloaded by the worker. A
GitHub, quay.io or registry outage now fails CI. Pins are verified by digest
where the source publishes one (the `clusterctl` binary, the worker image).

**Not covered, deliberately.** The cgroup confinement recipe
(`docs/src/guides/confining-a-worker-with-cgroups.md`) needs a real kernel and
host cgroup tree; a container worker cannot prove a CPU or memory ceiling, so
that stays verified by hand on real hardware. Multi-node child control planes,
LoadBalancer endpoints and drain under real workloads are also out of scope.

**Security.** Everything the tier creates is ephemeral: a throwaway cluster, a
per-run SSH key, and a privileged worker container that exists only for the
job. It adds no production component and no credential. It does add supply-chain
inputs to CI (above), recorded in the threat model's §3.

**Verification.** Run end to end on 2026-10-13 under rootful podman, with the
controller built from this change: all seven tests pass (the Machine exists
within a second of the `ScheduledMachine`; the worker joins in 69s; closing the
window removes the Node and all three derived objects in 163s; reopening brings
the same host back in 79s; deletion cleans up in 21s), and
`validate-examples.py` reports 77 passed, 0 failed, 7 not validated with
reasons. The first run against the published image found bug-008 (threat model
D6), which is why the controller had to come from this change.

**Extended 2026-10-14 (ADR 0020, ADR 0021).** Once every component shared one
image, the tier also deploys the capacity controller (and runs
`.github/scripts/capacity-e2e.bats`, which until then had never run in CI and
had drifted from the `TimeBasedSpotSchedule` schema) and both node agents on the
child cluster, with the image imported into the worker's containerd. Building
that found three defects no other tier could: the reclaim agent crash-looped on
Linux (ADR 0021), the agent pod-security policy rejected the shipped kata agent,
and a duplicate `args` key in the reclaim DaemonSet.

**CALM impact:** none. CALM models the running system; this is a test tier
that deploys the existing system unchanged.
