<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 06: k0smotron end-to-end and worker confinement

> The first run of `ScheduledMachine` against a **real** k0smotron hosted
> control plane with a **real** SSH-provisioned worker, on current Cluster API.
> It found two defects that the whole unit suite could not see, and it produced
> the systemd confinement recipe now documented in
> [Worker Resource Confinement](../../docs/src/operations/worker-resource-confinement.md).
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

**The confinement is now exonerated twice over.** Having already shown the
*limits* were never approached, this run tested the remaining variable, the
cgroup *placement*: `cgroupRoot` was removed from the worker profile so
`kubepods.slice` moved back to the root and pods left `k0s.slice` entirely. The
CrashLoop continued unchanged. Both halves of the confinement are therefore ruled
out as the cause, by independent experiment rather than by inference.

Also ruled out: a CNI or sandbox fault (the `hostNetwork` pods, which have no
CNI sandbox, CrashLoop too) and the management cluster's API (healthy
throughout, and its own control-plane VIP is held by exactly one node).

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
- **The worker pod CrashLoop is still unexplained, but tightly bounded.** Every
  pod on the worker, `hostNetwork` or not, starts cleanly (caches synced, DNS
  serving) and is then killed within one to two minutes, repeatedly.
  `kube-proxy` exits 2 with nothing logged after a successful startup and has no
  probes configured. Ruled out so far, each by experiment rather than argument:
  cgroup limits, cgroup placement, control-plane availability and replica count,
  certificate and kubeconfig validity, the CNI sandbox, and the management
  cluster's own health.

  The untested variable is now **the worker substrate itself**. This VM is
  attached with **macvtap** on a VM host whose CPU is passed through, and macvtap
  deliberately breaks host-to-guest traffic. The next experiment is to provision
  a second worker on a plain bridge, change nothing else, and compare: that
  isolates the network attachment and the VM environment in one step. Until that
  runs, nothing here should be attributed to 5-Spot, to k0smotron or to the
  confinement.

- **No automated e2e covers this path.** The run was driven by hand. The
  manifests are recorded well enough here to script against a disposable VM, and
  the v1beta2 shapes above are the ones worth scripting, since `v1beta1` is
  deprecated.
- **Watch-stream reconnects log at ERROR/WARN.** Normal watch timeouts surface
  as `ServiceError: error reading a body from connection` with an `ERROR`-level
  "reflector terminated" line, which reads as a fault during routine operation.
  Worth demoting to `debug`/`info` with a distinct message for genuine failures.
