<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 06: k0smotron end-to-end and worker confinement

> The first run of `ScheduledMachine` against a **real** k0smotron hosted
> control plane with a **real** SSH-provisioned worker, on current Cluster API.
> It found two defects that the whole unit suite could not see, and it produced
> the systemd confinement recipe now documented in
> [Worker Resource Confinement](../../docs/src/guides/confining-a-worker-with-cgroups.md).
> Decision recorded in [ADR 0017](../../docs/adr/0017-machine-reference-shape-follows-capi-contract.md).

## Why this exists

Every prior verification of `ScheduledMachine` was a unit test, a mocked
integration test, or a `kind` run against stub CRDs. None of them involved
Cluster API's own controllers, a k0smotron control plane that really serves an
API, or a host that k0s is really installed onto over SSH. Roadmap
[05](05-schedule-gated-capacity.md) had already shown what that gap costs: a
live run of the capacity controller found a hot reconcile loop no unit test
could see.

The second motivation was a question the CRD does not answer. A
`ScheduledMachine` decides *when* a host belongs to the cluster. It says
nothing about *how much* of that host k0s may consume while it is there. On a
shared host both questions matter, and the answer has to survive the machine
being destroyed and recreated on every schedule transition.

## Topology of the run

```text
management cluster (3-node k0s, Cluster API v1.14.3, k0smotron v2.2.0)
  ├── 5spot-controller            (local build, pinned by digest)
  ├── spot-schedule-time-based    (the provider that computes status.active)
  ├── cert-manager v1.21.2
  └── namespace spot-demo
        ├── Cluster / RemoteCluster / K0smotronControlPlane   (hosted CP, NodePort)
        ├── TimeBasedSpotSchedule                             (the calendar)
        └── ScheduledMachine  ->  K0sWorkerConfig + RemoteMachine + Machine
                                        |
                                        | SSH
                                        v
                        worker VM (8 vCPU / 8 GiB, Debian 13, cgroup v2)
                          k0s.slice   CPUQuota=200%  MemoryMax=4G
                            ├── k0sworker.service
                            └── k0s-kubepods.slice
```

The worker was deliberately given **more** CPU and memory than the cap so that
any observed throttling could only come from the cgroup, never from the host
running out.

## What it found

### F-1: the Machine reference shape did not follow the discovered contract

**Closed** by [ADR 0017](../../docs/adr/0017-machine-reference-shape-follows-capi-contract.md).

5-Spot resolved the served Machine version correctly (`v1beta2` on Cluster API
v1.14.3) and then built the Machine body in the `v1beta1` shape, with
`apiVersion` and `namespace` inside `bootstrap.configRef` and
`infrastructureRef`. `v1beta2` replaced both with a
`ContractVersionedObjectReference` (`apiGroup` + `kind` + `name`) and decodes
strictly, so the create failed outright:

```
strict decoding error: unknown field "spec.bootstrap.configRef.apiVersion", …
```

The reason it had not been noticed: `machine_api_version_for` passes the user's
`bootstrapSpec` version through, so a `ScheduledMachine` declaring `v1beta1`
produced a self-consistent `v1beta1` Machine that the conversion webhook
accepted. Only a `v1beta2` spec, or the discovery fallback, hit the bug. A unit
suite whose fixtures all assume `v1beta1` cannot reach it, and there was
already a passing test asserting that `v1beta2` passes through.

### F-2: the watched label had drifted off the API group

**Closed** in the same change; recorded in `.wolf/buglog.json`.

`src/labels.rs` still carried the retired `5spot.eribourg.dev` domain after the
group was renamed to `5spot.finos.org`. The reconciler *stamps*
`constants::SCHEDULED_MACHINE_LABEL` (`5spot.finos.org/…`) on every Machine it
creates, while `main` *selected* on `labels::LABEL_SCHEDULED_MACHINE`
(`5spot.eribourg.dev/…`). Consequences:

- both secondary watches matched nothing, so the Machine and Node mappings
  never fired and anything depending on Machine or Node state was timer-driven
  rather than event-driven, against the project's own standing rule;
- the retired prefix was **not** in `RESERVED_LABEL_PREFIXES`, so a user could
  set `5spot.eribourg.dev/scheduled-machine` through `machineTemplate.labels`,
  which is the exact key the reverse mapping trusted.

An existing test had enshrined the bug: it asserted that
`5spot.finos.org/scheduled-machine`, the canonical key, was an "imposter". That
test is the clearest evidence the divergence was an incomplete rename rather
than a decision.

`LABEL_SCHEDULED_MACHINE` is now an alias of the constant the reconciler
stamps, so the write path and the read path cannot drift again, and three
drift-guard tests pin it.

### F-3: `.cargo/config.toml` made the tree unbuildable on native Linux

**Closed 2026-10-11.** The checked-in cargo config hardcoded macOS
cross-toolchain linkers:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "x86_64-unknown-linux-gnu-gcc"
```

A `[target.<triple>]` pin applies whenever that triple is the *target*,
including when it is also the *host*. On a native `x86_64-unknown-linux-gnu`
machine that binary does not exist, so every build script failed at once with
`linker 'x86_64-unknown-linux-gnu-gcc' not found`, and the project could not be
built on the Linux build host the contributor guide tells you to use. The run
had to work around it with an env override.

A cross-linker is a property of the host/target **pair**, which cargo's config
cannot express, so the choice moved to the Makefile. `build-linux-amd64` and
`build-linux-arm64` now try `cross`, then a native build when the host already
targets that architecture, then a cross toolchain on `PATH` with
`CARGO_TARGET_<TRIPLE>_LINKER` exported for it. `kind-load` got the same
treatment: it previously required a cross linker unconditionally and so could
not run on a Linux host at all. Both the `<triple>-gcc` and
`<arch>-linux-gnu-gcc` spellings are now accepted, which also resolves a
pre-existing inconsistency where the config pinned one name and the Makefile
probed the other. `.cargo/config.toml` keeps a comment explaining why the pins
are absent, so they do not come back.

Verified in both directions, because removing the pin alone would have broken
macOS cross builds:

- **native Linux x86_64**, env override explicitly unset: `make
  build-linux-amd64` completes (release profile, 4m02s);
- **cross from macOS**: without the linker env a target build fails at
  `linking with 'cc' failed`; with the env var the Makefile now exports it links
  and produces a valid `ELF 64-bit LSB pie executable, x86-64`.

### F-4: the node advertises the host, not the cap

**Closed 2026-10-12** (the behaviour is inherent; the *correction* is now
verified rather than merely prescribed). kubelet reads CPU and memory from
`/proc`, so with the slice capped at 2 cores / 4 GiB the Node still advertised
`cpu: 8`, `memory: 8030648Ki`, and the scheduler would admit pods the worker can
never serve. There is no cgroup-only fix: the cap is the *enforcement* and
`systemReserved` is the *advertisement*, and both have to be set.

Applying `systemReserved: {cpu: "6", memory: 4Gi}` to the worker profile and
restarting `k0sworker` brought the two views into line:

| | cpu | memory |
| --- | --- | --- |
| capacity | 8 | 8133048Ki |
| allocatable, before | 8 | 8030648Ki |
| allocatable, after | **2** | **3836344Ki** |

`cpu=2` matches `CPUQuota=200%` exactly. Memory settles slightly *under*
`MemoryMax` because kubelet also withholds its hard-eviction threshold on top of
`systemReserved`, which is worth knowing before anyone tries to make the two
numbers identical.

## What was verified

| Behaviour | Result |
| --- | --- |
| CAPI version discovery | `Discovered CAPI Machine API version: v1beta2` |
| Machine stored shape | `apiGroup`-form refs at `cluster.x-k8s.io/v1beta2` |
| Owned-label stamping | `5spot.finos.org/scheduled-machine: spot-worker-01` |
| Activation | provider `active=true` -> SM `Active` -> bootstrap + infra + Machine created |
| Provisioning | k0s v1.34.11+k0s.1 installed over SSH; Machine `Running`; Node `Ready` |
| Confinement placement | `k0sworker.service` **and** `k0s-kubepods.slice` both inside `k0s.slice` |
| CPU cap | 8 cores demanded, 2.01 used, ~100% of periods throttled, 90.4s denied per 15s |
| Memory cap | peak 3,983,450,112 B against a 4 GiB max; 164,008 `high` events; 0 OOM kills |
| Deactivation | window closed -> `ShuttingDown` -> grace period -> `Inactive`, all three objects removed |
| Reactivation | window reopened -> `Active` within 20s, objects recreated with correct refs |
| Host cleanup | k0smotron stopped `k0sworker` on Machine deletion |
| Controller delegation across reboot | the `hugetlb` delegation unit ran to `success` before kubelet; **0** missing-controller failures that boot against **17** on the boot before it existed |
| Native Linux build (F-3 fix) | `make build-linux-amd64` with no env override, release profile, 4m02s |

The CPU figure is self-checking: 8 demanded minus 2 granted is 6 cores' worth
of denied runnable time, and 6 x 15s = 90s, which is what `throttled_usec`
reported.

## The lesson worth keeping

**Throttling from a parent cgroup is invisible at the child.** At the same
instant the slice was refusing six cores' worth of work, the pod doing the
asking reported `nproc 8`, `cpu.max max 100000` and `nr_throttled 0`. A
workload cannot observe the limit that binds it, so a container's
`nr_throttled 0` is not evidence that it is unconstrained. Diagnose at the
slice.

## The 2026-10-12 rebuild: what it settled

The environment was torn down and recreated from scratch to test the leading
hypothesis for the pod-kill thread, with three changes: a **stable LoadBalancer
endpoint** instead of a NodePort, `externalAddress` pinned, and **three control
plane replicas** instead of one. Everything was written at the **v1beta2**
storage versions so no field could be lost in conversion.

**The hypothesis was wrong.** Pods on the worker still CrashLoop on a brand new
cluster with three control plane replicas, a stable VIP, correct certificate
SANs and fully verified TLS. The single-endpoint single-replica control plane
was not the cause.

**Root cause, found 2026-10-12: a cgroup driver mismatch introduced by this
project's own worker profile.** k0s's containerd is configured with
`SystemdCgroup = false`, i.e. the **cgroupfs** driver. The confinement recipe
set `cgroupDriver: systemd` on kubelet, so kubelet created and expected systemd
slices while containerd placed containers in cgroupfs paths. kubelet could not
reconcile the two and tore down and recreated pod sandboxes indefinitely.

Proven by two controls:

| configuration | result |
| --- | --- |
| **stock** k0smotron worker, no `files`, no `preK0sCommands`, no `--profile`, no worker profile | all 5 system pods `1/1 Running`, **0 restarts**, 7 minutes |
| **corrected** confinement: slice + limits kept, `cgroupDriver` override removed, `cgroupRoot: /k0s.slice` | all 5 pods `1/1 Running`, **0 restarts**, 7 minutes, **and** the cap enforces: 8 cores demanded, 2.01 used, 301 of 301 periods throttled, pods still healthy under full saturation |

### Durability, verified 2026-10-13

The corrected confinement was then checked against the two things a scheduled
machine actually does: outlive a host reboot, and be rebuilt by the schedule.

| check | result |
| --- | --- |
| **Duration** | 20h21m uptime with **0 total pod restarts** across all five system pods |
| **Host reboot** | confinement reassembles unattended: `hugetlb` unit `active (success)` and the controller delegated, `k0sworker` back in `/k0s.slice/k0sworker.service`, `cpu.max` 2 cores, `memory.max` 4 GiB, pods back under `k0s.slice/kubepods`, **0** kubelet controller failures |
| **Deactivation** | window closed, `active=false` within 20s, `ShuttingDown`, 5m grace honoured, `Inactive` with all three CAPI objects removed |
| **Reactivation** | window reopened, `active=true` within 20s, Machine `Running` in 60s |
| **Reapplication from `bootstrapSpec`** | all three host files were **deliberately deleted** while the machine was down, and the fresh activation recreated every one: slice, `Slice=` drop-in and `hugetlb` unit, with the limits and pod placement identical and 0 controller failures |

The reapplication check is the one worth noting. k0smotron's host reset does
**not** remove files written by `K0sWorkerConfig.files`, so a leftover slice
would have made a naive test pass for the wrong reason. Wiping them first is what
proves the confinement genuinely travels with the machine definition rather than
persisting as host state.

### Why the earlier "exoneration" was wrong

The three elimination experiments were real measurements but **all confounded by
the same uncontrolled variable**: `cgroupDriver: systemd` stayed in the worker
profile throughout. Removing the limits, then the `cgroupRoot`, then the slice
itself each changed nothing *because the mismatch was never removed*. The
conclusion drawn from them, that cgroups were innocent and the substrate was at
fault, was wrong, and the macvtap hypothesis it produced was a dead end.

The lesson is about method, not cgroups: when several knobs are changed together
and then removed one at a time, "removing X changed nothing" only exonerates X
if everything else is genuinely back to default. The honest control was the
**stock install**, and it should have been the first experiment rather than the
tenth.

### Why it was so hard to see

The failure presents as nothing resembling a cgroup problem:

- every pod, `hostNetwork` or not, starts perfectly (caches synced, DNS
  answering), idles, and is SIGKILLed 60 to 90 seconds later;
- containerd reports `exit_status:137` while the container's own cgroup is
  unlimited and reports `oom_kill 0`;
- `kube-proxy` dies with no error logged and has no probes that could explain it;
- the Node stays `Ready` throughout, because kubelet's own health is fine;
- no limit is approached: 76 throttled periods of 684,221, 779 MB of 4 GiB,
  `pids` 85 of 8192, PSI 0.00;
- AppArmor, `systemd-oomd`, the kernel OOM killer, certificates, control-plane
  availability and the management cluster were all checked and were all clean.

A `Ready` node whose every pod silently restarts is the signature worth
remembering.

### What the rebuild did prove

| Behaviour | Result |
| --- | --- |
| ADR 0017 on the **forward** path | a `v1beta2` `bootstrapSpec` produced a `cluster.x-k8s.io/v1beta2` Machine with `apiGroup`-form refs, phase `Running`. This path was previously both broken and unexercised |
| LoadBalancer control plane | VIP assigned on creation, both ports reachable, API answers |
| `externalAddress` before creation | lands in the serving certificate SANs **and** the generated kubeconfig |
| Full TLS verification over the VIP | `/version` returns `v1.34.11+k0s`, no `--insecure-skip-tls-verify` |
| Three CP + three etcd replicas | all `1/1` within ~2 minutes |
| 5-Spot finalizer on teardown | removed the Machine, bootstrap and infrastructure objects in 15s, against a *degraded* child cluster |
| Worker join | Node `Ready`, confinement reapplied from `bootstrapSpec` |

### Two things cannot be retrofitted onto an existing k0smotron cluster

Both are fixed at creation time and k0smotron reconciles neither, so changing
the service type on a live cluster is not sufficient:

- the **generated kubeconfig** keeps the original server address. Deleting the
  secret does not help: it was not recreated at all;
- the **serving certificate SANs** keep the original address, so a client using
  the new VIP fails with `x509: certificate is valid for ... not <vip>`.

Set `externalAddress` **before** the cluster is created.

### k0smotron v1beta1 to v1beta2 conversion is lossy

Worth knowing independently of 5-Spot, and the same class of defect as
ADR 0017: fields present in the served `v1beta1` but absent from the `v1beta2`
**storage** version are silently discarded on write.

| Type | v1beta1 | v1beta2 (storage) |
| --- | --- | --- |
| `K0smotronControlPlane.spec.service` | `annotations`, `labels`, `externalTrafficPolicy`, `loadBalancerClass`, … | **none of those**; only `apiPort`, `konnectivityPort`, `type` |
| `K0sWorkerConfig.spec` | `preStartCommands`, `postStartCommands` | renamed to **`preK0sCommands`**, **`postK0sCommands`** |
| `RemoteMachine.spec` | `customCleanUpCommands` | renamed to **`cleanUpCommands`** |

The practical consequence here: a MetalLB pool **cannot** be selected with
`service.annotations`, because that field never reaches storage. The pool is
instead scoped with MetalLB's own
`serviceAllocation.namespaces`, which needs no annotation on the Service and
does not make the pool a cluster-wide default.

## Open threads

- **F-4** is closed; **F-3** is closed. See above.
- **The worker pod CrashLoop is resolved.** Cause and fix above. The guide
  (`docs/src/guides/confining-a-worker-with-cgroups.md`) has been corrected: the
  `cgroupDriver` override is removed, `cgroupRoot` is `/k0s.slice` for the
  cgroupfs driver, section 6.1 now leads with the mismatch and its symptom
  signature, and the `Delegate=` advice is split by driver. The macvtap
  hypothesis is withdrawn: the substrate was never implicated.

- **No automated e2e covers this path.** Closed by ADR 0019: see
  "Closed 2026-10-13 (second round)" below.
- **`ScheduledMachine.status.machineRef`, `bootstrapRef` and
  `infrastructureRef` are never written.** Found by the e2e tier. The CRD
  declares all three and `docs/src/concepts/machine-lifecycle.md` shows them
  populated, but no code path sets them; only `providerID` and `nodeRef` are
  patched (`patch_machine_refs_status` in `src/reconcilers/helpers.rs`). The
  suite asserts on the Machine found by its owned label instead. **Open, and a
  decision rather than a typo:** either the controller populates them on create
  and clears them on removal, or they leave the CRD and the docs. Either way it
  is an API change that wants its own ADR.
- **Deletion-time reconcile noise.** Minor, found by the same run: in the 20
  seconds after a `ScheduledMachine` is deleted, Machine and Node events still
  map to it and its reconciles log ten `WARN`/`ERROR` lines ("not found in local
  store", a 404 on the status patch) with exponential back-off. Harmless, but it
  is the same kind of false alarm that hid bug-008. **Open.**

### Closed 2026-10-13

- **Watch-stream reconnects logged at ERROR/WARN.** Closed the first time on a
  wrong premise, then corrected the same day; see bug-008 in the second round
  below. The first fix added `is_routine_watch_restart` to `src/stream.rs` and
  classed `WatchFailed` as routine, because it appeared several times a minute
  on a healthy cluster. That cadence was the defect, not noise: a 30-second
  client read timeout was killing every quiet watch. `WatchFailed` is now
  **not** routine and stays at `warn`; only HTTP 410 Gone is demoted to
  `debug`. The four call sites still branch on the classifier.

- **The repo's examples were all on the deprecated `v1beta1` contract**, so the
  forward path ADR 0017 added was documented nowhere a reader would copy from.
  Closed. The seven `examples/scheduledmachine-*.yaml` files now declare
  `v1beta2` for both `bootstrapSpec` and `infrastructureSpec`, which matches
  reality on this cluster: k0smotron v2.2.0 serves `v1beta1` and `v1beta2` for
  all four of its CRDs and has **`v1beta2` as the storage version**, so
  `v1beta1` is the one being converted away from. `examples/scheduledmachine-basic.yaml`
  carries the reasoning inline, including which version to declare on older
  Cluster API, since 5-Spot passes the version through rather than choosing it.

  `docs/src/advanced/capi-integration.md` now shows the `Machine` 5-Spot creates
  in the v1beta2 shape (`apiGroup` + `kind` + `name`) with the v1beta1
  `ObjectReference` beside it, which is the ADR 0017 behaviour and was not
  previously written down anywhere in the published docs.

  **The workshop stayed on `v1beta1` in the first round**, because the kubeadm
  and CAPD v1beta2 shapes could not be verified without a CAPI v1.11+ cluster
  with those providers. That is superseded: see the second round below.

  What *was* wrong in the workshop is now fixed: both `README.md` and
  `workload-cluster.yaml` told the reader to pin `clusterctl` to the v1.8 or
  v1.9 line "because 5-Spot emits v1beta1 Machines". That claim was false after
  ADR 0017 and it steered people onto a two-year-old CAPI for no reason.

  `examples/scheduledmachine-bad-taint.yaml` keeps its deliberate defect, and
  `docs/src/security/admission-validation.md` is unchanged: its rejection
  examples exercise the admission rules (duration format, the API-group
  allowlist), use the allowlisted `k0smotron.io` group for the bootstrap side,
  and no rule they demonstrate depends on the contract version.

  **Still open from this item in the first round:** nothing in CI validated
  `examples/` against a schema. Closed in the second round below.

### Closed 2026-10-13 (second round)

- **An automated end-to-end tier now covers this path (ADR 0019).** A kind
  management cluster with Cluster API v1.14.3 and k0smotron v2.2.0, a hosted
  `K0smotronControlPlane`, and a worker container (systemd + sshd, pinned by
  digest) provisioned over SSH as a `RemoteMachine`. `.github/scripts/k0smotron-e2e.bats`
  drives one `ScheduledMachine` through activate, deactivate, reactivate and
  delete, and asserts the two contracts that escaped before: the Machine written
  at `v1beta2` (proved from its `managedFields`, since conversion would mask a
  read-back) with `apiGroup` references (ADR 0017), and the controller-owned
  label the watches select on (bug-007). Runs in `.github/workflows/e2e.yaml`
  through `make e2e-k0smotron-up`, `make validate-examples`, `make e2e-k0smotron`.
  The worker needs a rootful container engine: under rootless podman, k0s cannot
  load its eBPF device filter or find the `cpuset` controller. Two more
  environment traps are handled by the harness: containerd's overlay snapshots
  need `/var/lib/k0s` on a volume, and podman's default 2048-pid cap kills the
  hosted control plane. **Result, 2026-10-13, controller built from this
  change: 7 of 7 pass** (join 69s, removal 163s, return 79s, deletion 21s).

- **bug-008, found on the tier's first run.** A `ScheduledMachine` created at
  00:41:41 was first reconciled at 00:44:22. Every watch in the controller failed
  on a fixed cadence (the `ScheduledMachine` reflector every 30 seconds exactly):
  `src/main.rs` and `src/reconcilers/child_client.rs` set a 30-second
  `read_timeout` on the client, an idle timeout that applies to watches too,
  while every watch used kube-runtime's 290-second server-side default. kube-rs
  documents that the watch timeout must be below the client's read timeout.
  Present since the first commit, and it is the same "error reading a body from
  connection" this roadmap had recorded as routine noise. Fixed with
  `stream::watch_config()` (server-side timeout 20s) on all twelve watch sites,
  compile-time guards in `src/constants.rs`, and the classifier correction
  above. With the fixed binary, the Machine appeared in the same second as the
  `ScheduledMachine`, and the controller logged no watch failures at all where
  the old one logged one every 30 seconds. Security-relevant, since the kill
  switch and emergency reclaim are reactive: threat model 1.9, D6.

- **Examples and docs are validated in CI against the real schemas.**
  `.github/scripts/validate-examples.py` dry-runs every `examples/` document and
  every complete object in `docs/src` with `--validate=strict` against the e2e
  cluster, and validates each embedded `bootstrapSpec` / `infrastructureSpec` as
  the standalone provider object 5-Spot creates. Its first run found seven real
  problems in the docs: an invalid `nodeRegistration: {}` that the `v1beta2`
  sweep had produced, a `Lease` with the wrong time format, a `Machine` example
  that did not match what `src/reconcilers/helpers.rs` creates, a README example
  still on `5spot.finos.org/v1alpha1`, and four deliberately partial snippets,
  now marked `validate-examples: fragment` so they are listed, not skipped.
  Result: 77 passed, 0 failed, 7 not validated, each with its reason.

- **The workshop moved to `v1beta2`, verified rather than guessed.** The shapes
  were taken from Cluster API's own conversion of the old manifests on a live
  v1.14.3 cluster, then validated with the providers' webhooks. It also moved
  off the `Docker*` kinds, which CAPI v1.14 deprecates, to the `Dev*` kinds with
  the docker backend. The prerequisites now say why kind v0.28 is the minimum:
  Cluster API v1.14 needs a management cluster of Kubernetes v1.33 or later.
  **Not verified:** an actual CAPD walkthrough run; CAPD needs a Docker daemon.

