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

### F-3: `.cargo/config.toml` makes the tree unbuildable on native Linux

**Open.** The checked-in cargo config hardcodes macOS cross-toolchain linkers:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "x86_64-unknown-linux-gnu-gcc"
```

On a native `x86_64-unknown-linux-gnu` host that linker does not exist, so every
build script fails. This contradicts the project's own instruction to build on
a Linux build host, and the run had to work around it with
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc`. The fix is to move the
cross-linker pins somewhere that only applies when actually cross-compiling
(a `Cross.toml`, a `[target.…]` block selected by an explicit profile, or a
documented env override), rather than to the default config every build reads.

### F-4: the node advertises the host, not the cap

**Open, by nature.** kubelet reads CPU and memory from `/proc`, so with the
slice capped at 2 cores / 4 GiB the Node still advertised `cpu: 8` and
`memory: 8030652Ki`. The scheduler will admit pods the worker can never serve.
There is no cgroup-only fix: the cap is the *enforcement* and
`systemReserved` is the *advertisement*, and both have to be set. Documented
with the correction in the operations page.

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

## Open threads

- **F-3** and **F-4** above.
- **A pod on the confined worker was killed with SIGKILL about 30s after
  start**, repeatably, with no OOM (`oom_kill 0`), no node pressure, no taint,
  no cordon and nothing in the 5-Spot controller log. systemd created and then
  removed only that pod's slice while the system pods' slices stayed active, so
  kubelet is ending it. Not yet root-caused; the CPU and memory measurements
  above were taken with the load placed directly into `k0s.slice` via
  `systemd-run`, which is the unit under test and is unaffected. This needs its
  own investigation before the confinement recipe is recommended for anything
  carrying real workloads.
- **No automated e2e covers this path.** The run was driven by hand. A
  `kind`-based harness cannot replace it, because the defects came from real
  CAPI controllers and a real SSH install, but the manifests are recorded well
  enough here to script against a disposable VM.
- **Watch-stream reconnects log at ERROR/WARN.** Normal watch timeouts surface
  as `ServiceError: error reading a body from connection` with an `ERROR`-level
  "reflector terminated" line, which reads as a fault during routine operation
  and made the first minutes of this run much harder to interpret. Worth
  demoting to `debug`/`info` with a distinct message for genuine failures.
