<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Confining a scheduled worker with systemd and cgroup v2

A `ScheduledMachine` decides **when** a host belongs to a cluster. It says
nothing about **how much** of that host k0s may consume while it is there. On a
host shared with anything else, both questions matter.

This page is the complete guide to answering the second one: how to cap a k0s
worker and every pod it runs at a fixed budget, how to deliver that cap as part
of the machine definition so it survives every schedule transition, which knobs
exist, how to choose values, and how to prove the cap is real.

The worked example throughout caps the worker at **2 cores and 4 GiB** on a host
with **8 vCPUs and 8 GiB**, deliberately, so that any throttling observed can
only come from the cgroup and never from the host running out.

Every command, value and measurement below was taken from a live run; see
[roadmap 06](https://github.com/finos/5-spot/blob/main/.github/community/06-k0smotron-e2e-and-worker-confinement.md).

---

## 1. Prerequisites

| Requirement | Why | Check |
| --- | --- | --- |
| **cgroup v2** (unified hierarchy) | Everything here uses the v2 interface files. v1 has different names and no `MemoryHigh` equivalent | `stat -fc %T /sys/fs/cgroup` returns `cgroup2fs` |
| **systemd** as PID 1 | The limits are expressed as unit properties | `systemctl --version` (257 in the reference run) |
| **kubelet and containerd on the SAME cgroup driver** | A mismatch silently destroys pod lifecycle on the node; k0s uses cgroupfs | `grep SystemdCgroup /run/k0s/containerd-cri.toml`, section 6.1 |
| **The `hugetlb` controller delegated** | kubelet refuses to start without it, and systemd never delegates it on its own | section 6.2 |

A host with no swap, as the reference run had, simplifies memory behaviour:
`MemoryMax` is then a hard wall with no swap escape, and `MemorySwapMax` is
moot. Check with `free -m`.

---

## 2. The two halves

A cap is only worth having if it covers the pods, and that requires two separate
things to line up:

1. **A systemd slice carrying the limits**, holding `k0sworker.service`. This
   bounds k0s, containerd and kubelet.
2. **kubelet creating its pod cgroups inside that slice**, via `cgroupRoot`
   with the systemd cgroup driver. Without this, pods land in a sibling
   `kubepods` tree and the cap does not apply to them at all.

Cap only the service and a runaway pod is unaffected. Both halves together and a
single `CPUQuota` governs the union:

```text
k0s.slice                      CPUQuota=200%  MemoryMax=4G   <-- the limit lives here
├── k0sworker.service          k0s + containerd + kubelet
└── kubepods                   every workload pod
    ├── burstable/pod<uid>
    └── besteffort/pod<uid>
```

The `kubepods` tree uses plain **cgroupfs** names (`kubepods/burstable`), not
systemd ones (`kubepods.slice`), because k0s's containerd uses the cgroupfs
driver and kubelet must match it. Getting that wrong is the single most
destructive mistake available here; see section 6.1.

---

## 3. Quick start

Everything needed, in one `ScheduledMachine`. `K0sWorkerConfig` accepts `files`,
`preStartCommands` and `args`, so the confinement travels with the machine
definition and is reapplied every time the schedule recreates the worker.
Nothing has to be baked into the host image.

```yaml
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: spot-worker-01
  namespace: spot-demo
spec:
  clusterName: spot-demo
  enabled: true
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: business-hours
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: K0sWorkerConfig
    spec:
      version: v1.34.11+k0s.1
      files:
        # (a) the slice that carries the limits
        - path: /etc/systemd/system/k0s.slice
          permissions: "0644"
          content: |
            [Unit]
            Description=k0s worker confinement (2 cores / 4 GiB)
            Before=slices.target

            [Slice]
            CPUAccounting=yes
            MemoryAccounting=yes
            IOAccounting=yes
            TasksAccounting=yes
            CPUQuota=200%
            MemoryMax=4G
            MemoryHigh=3584M
            TasksMax=8192

        # (b) put k0s itself in it. A drop-in for a unit that does not exist
        #     yet is fine: it applies when `k0s install worker` creates it.
        - path: /etc/systemd/system/k0sworker.service.d/10-confine.conf
          permissions: "0644"
          content: |
            [Service]
            Slice=k0s.slice

        # (c) delegate the controller kubelet needs and systemd will not.
        #     Ordered before k0sworker so it wins the race at boot.
        - path: /etc/systemd/system/k0s-cgroup-controllers.service
          permissions: "0644"
          content: |
            [Unit]
            Description=Delegate the cgroup v2 controllers kubelet requires
            Before=k0sworker.service
            DefaultDependencies=no

            [Service]
            Type=oneshot
            RemainAfterExit=yes
            ExecStart=/bin/sh -c 'echo +hugetlb > /sys/fs/cgroup/cgroup.subtree_control'

            [Install]
            WantedBy=multi-user.target
      preStartCommands:
        - systemctl daemon-reload
        - systemctl enable --now k0s-cgroup-controllers.service
      args:
        - --profile=confined
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: RemoteMachine
    spec:
      address: 192.0.2.60
      port: 22
      user: admin
      useSudo: true
      sshKeyRef:
        name: spot-demo-ssh-key
  gracefulShutdownTimeout: 5m
  nodeDrainTimeout: 5m
```

!!! warning "`bootstrapSpec` runs as root on the worker"
    `files` and `preStartCommands` are executed by the bootstrap provider with
    root privilege on the target host. Anyone who may create a
    `ScheduledMachine` in a namespace can therefore write arbitrary files and
    run arbitrary commands on every host its `RemoteMachine` reaches. That is
    k0smotron's model, not something this page introduces, but it is the reason
    `create` on `scheduledmachines` is a privileged grant. See the
    `bootstrapSpec` residual in the
    [threat model](../security/threat-model.md).

!!! note "If you write `bootstrapSpec` at `v1beta2`"
    The example above targets `bootstrap.cluster.x-k8s.io/v1beta1`. `v1beta1`
    is deprecated, and several field names differ in the `v1beta2` **storage**
    version:

    | v1beta1 | v1beta2 |
    | --- | --- |
    | `preStartCommands` | `preK0sCommands` |
    | `postStartCommands` | `postK0sCommands` |
    | `RemoteMachine.customCleanUpCommands` | `cleanUpCommands` |

    `files`, `args` and `version` are unchanged. A field written at `v1beta1`
    that does not exist in `v1beta2` is **silently discarded** on conversion, so
    check the storage version's schema rather than trusting the served one:

    ```sh
    kubectl get crd k0sworkerconfigs.bootstrap.cluster.x-k8s.io -o json \
      | jq -r '.spec.versions[] | select(.storage)
               | .schema.openAPIV3Schema.properties.spec.properties | keys'
    ```

---

## 4. The kubelet half: a worker profile

kubelet's cgroup settings come from a k0s **worker profile**, which lives on the
control plane and is selected by name with `--profile`. Prefer this over
`--kubelet-extra-args`: the profile is structured YAML, so there is no
multi-valued argument string to quote through a shell.

```yaml
apiVersion: controlplane.cluster.x-k8s.io/v1beta1
kind: K0smotronControlPlane
metadata:
  name: spot-demo-control-plane
  namespace: spot-demo
spec:
  replicas: 1
  version: v1.34.11-k0s.1
  service:
    type: NodePort
  k0sConfig:
    apiVersion: k0s.k0sproject.io/v1beta1
    kind: ClusterConfig
    spec:
      workerProfiles:
        - name: confined
          values:
            # Do NOT set cgroupDriver here. k0s's containerd ships
            # SystemdCgroup = false, so overriding kubelet to `systemd`
            # is a driver MISMATCH that breaks the node. See 6.1.
            cgroupRoot: /k0s.slice
            # make the Node's advertised capacity match the cap - see 6.3
            systemReserved:
              cpu: "6"
              memory: 4Gi
```

With k0s's default **cgroupfs** driver, `cgroupRoot` is a literal cgroup path, so
`/k0s.slice` means exactly the directory `/sys/fs/cgroup/k0s.slice` that systemd
created for the slice. kubelet then builds `kubepods` inside it and the slice's
limits apply to the whole subtree.

`values` is a `KubeletConfiguration`, so anything kubelet accepts there is
available, including `evictionHard`, `kubeReserved`, `systemReservedCgroup` and
`cpuManagerPolicy`.

Changing a profile requires the worker to re-read it: patch the control plane,
then `systemctl restart k0sworker` on the host (or let the schedule recreate the
machine).

---

## 5. The control surface

Every directive below is a systemd unit property that writes a cgroup v2
interface file. The mappings in this table were confirmed on the reference
worker by applying each property and reading the file back.

### 5.1 CPU

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `CPUQuota=200%` | `cpu.max` -> `200000 100000` | **Hard cap.** 200 ms of CPU per 100 ms period, so 2 cores. Work beyond it is throttled, not queued elsewhere |
| `CPUQuotaPeriodSec=100ms` | `cpu.max` period field | Enforcement window. Shorter means smoother but more overhead; the quota rescales to keep the same ratio (setting `50ms` turned `200000 100000` into `100000 50000`, still 2 cores) |
| `CPUWeight=100` | `cpu.weight` | **Soft share**, 1 to 10000, default 100. Only matters under contention, and never caps anything |
| `AllowedCPUs=0-3` | `cpuset.cpus` | Pins to specific CPUs. Changes *which* cores, not how many core-seconds |
| `CPUAccounting=yes` | enables `cpu.stat` | Required for the throttling counters to be populated |
| (not a directive) | `cpu.stat` | `usage_usec`, `nr_periods`, `nr_throttled`, `throttled_usec` |
| (not a directive) | `cpu.pressure` | PSI: how long tasks were stalled waiting for CPU |

**Quota or weight?** `CPUQuota` for a guarantee that the worker cannot exceed a
budget, which is the point of a spot worker. `CPUWeight` when you only want the
worker to yield to an incumbent under contention and to use the whole box when
it is idle. They compose: a weight to lose politely, a quota to never exceed.

### 5.2 Memory

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `MemoryMax=4G` | `memory.max` -> `4294967296` | **Hard wall.** Allocation past it triggers reclaim, then the cgroup OOM killer |
| `MemoryHigh=3584M` | `memory.high` -> `3758096384` | **Throttle point.** Over it, the allocating task is slowed and reclaim is forced, but nothing is killed. This is what turns an OOM kill into a slowdown |
| `MemoryLow=0` | `memory.low` | Best-effort protection: reclaim avoids this cgroup while under the value |
| `MemoryMin=256M` | `memory.min` -> `268435456` | Hard protection: this much is never reclaimed |
| `MemorySwapMax=0` | `memory.swap.max` | Caps swap. Moot with no swap configured |
| `MemoryAccounting=yes` | enables the counters | Required for the files below |
| (not a directive) | `memory.current`, `memory.peak` | Live and high-water usage |
| (not a directive) | `memory.events` | `low`, `high`, `max`, `oom`, `oom_kill`, `oom_group_kill` |
| (not a directive) | `memory.stat` | Breakdown: `anon`, `file`, `slab`, `sock`, `pgmajfault` |
| (not a directive) | `memory.pressure` | PSI for memory stalls |
| (not a directive) | `memory.reclaim` | Write a byte count to force reclaim by hand |

**Always set `MemoryHigh` below `MemoryMax`.** This is the single most
consequential choice on this page. With both set, a workload that respects
allocation back-pressure is slowed instead of killed. In the reference run,
asking for 6 GiB inside the 4 GiB slice produced **164,008** `high` events,
**zero** `max` breaches and **zero** OOM kills, with `memory.peak` at
3,983,450,112 bytes against a 4,294,967,296 byte ceiling. A workload that
refuses to back off and holds unreclaimable anonymous pages will still reach
`MemoryMax` and be OOM-killed; `MemoryHigh` buys grace, not immunity.

`memory.oom.group` is worth knowing about: with it set, the kernel kills the
whole cgroup rather than picking a victim, which avoids a half-dead pod. systemd
does not expose it as a slice directive.

### 5.3 IO

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `IOWeight=100` | `io.weight` -> `default 100` | Soft share of disk bandwidth, 1 to 10000 |
| `IOReadBandwidthMax=/dev/vda 50M` | `io.max` | Hard per-device read cap |
| `IOWriteBandwidthMax=/dev/vda 50M` | `io.max` | Hard per-device write cap |
| `IOReadIOPSMax` / `IOWriteIOPSMax` | `io.max` | Hard per-device IOPS caps |
| `IOAccounting=yes` | enables `io.stat` | Required for the counters |
| (not a directive) | `io.stat` | Per device: `rbytes`, `wbytes`, `rios`, `wios` |
| (not a directive) | `io.pressure` | PSI for IO stalls |

The hard IO caps name a **device**, so they are host-specific and awkward to put
in a portable machine definition. `IOWeight` is usually the better choice for a
spot worker: it protects the incumbent under contention without pinning the
config to a device path. Note that image pulls and container writes all pass
through this, so an aggressive read cap makes pod startup slow in a way that is
easy to misdiagnose.

### 5.4 Processes

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `TasksMax=8192` | `pids.max` -> `8192` | Hard cap on tasks, the fork-bomb bound |
| `TasksAccounting=yes` | enables `pids.current` | |
| (not a directive) | `pids.current`, `pids.peak`, `pids.events` | |

Size this generously. Every container contributes several tasks, and hitting
`pids.max` produces confusing `fork: Resource temporarily unavailable` failures
deep inside the runtime rather than a clean error. 8192 was comfortable for a
worker running ~104 tasks at idle.

### 5.5 Delegation

`Delegate=yes` on a unit hands its subtree to the process inside and tells
systemd to stop managing children there. Which setting is right depends entirely
on the cgroup driver, so this is worth stating carefully:

- **With the cgroupfs driver (k0s's default, and what this guide uses)** kubelet
  creates plain cgroup directories under `cgroupRoot` without going through
  systemd. `Delegate=yes` is the honest declaration of that arrangement. The
  verified configuration here carries `Delegate=yes` in the slice unit, though
  note `systemctl show k0s.slice -p Delegate` still reported `no` and the
  configuration worked regardless, so do not treat it as the load-bearing
  setting. What matters is that the drivers match.
- **With the systemd driver** kubelet creates its pod slices *through* systemd,
  so delegation and that driver are contradictory and `Delegate` should be left
  at its default.

Controller **delegation** in the kernel sense is a separate thing and is
required either way: see 6.2.

---

## 6. The three things that will catch you

### 6.1 Do not set `cgroupDriver: systemd` on k0s

**This is the one that will cost you a day.** k0s's containerd is configured
with the **cgroupfs** driver:

```console
$ sudo grep SystemdCgroup /run/k0s/containerd-cri.toml
            SystemdCgroup = false
```

Overriding kubelet to `cgroupDriver: systemd` therefore creates a **cgroup
driver mismatch**: kubelet creates and expects systemd slices
(`kubepods.slice`, `k0s-kubepods-besteffort-pod<uid>.slice`) while containerd
puts containers in cgroupfs paths. kubelet cannot reconcile what it finds with
what it expects, so it tears down and recreates pod sandboxes forever.

The symptom is **nothing like a cgroup problem**, which is what makes it
expensive:

- every pod on the node, `hostNetwork` or not, starts perfectly (caches synced,
  DNS answering), sits idle, and is SIGKILLed 60 to 90 seconds later;
- containerd reports `exit_status:137` while the container's own cgroup is
  unlimited and reports `oom_kill 0`;
- `kube-proxy` dies with **no error logged at all** and has no liveness or
  readiness probe that could explain it;
- the Node stays `Ready` the whole time, because kubelet's own health is fine;
- restart counts climb into the hundreds over a day;
- `SandboxChanged: Pod sandbox changed, it will be killed and re-created`
  appears in events, and kubelet loops on `StopPodSandbox` / `TearDown network`
  / `RemovePodSandbox`.

None of the obvious culprits are involved: no limit is approached, no OOM
occurs, PSI stays at zero, and removing the limits, the `cgroupRoot` or the
slice entirely changes nothing, because the driver mismatch is still there.

**So: leave `cgroupDriver` unset** and let kubelet use k0s's default. Check
which driver containerd actually uses before overriding anything:

```sh
sudo grep -n 'SystemdCgroup' /run/k0s/containerd-cri.toml
```

If you genuinely want the systemd driver, you must switch **both** sides,
giving containerd a drop-in under `/etc/k0s/containerd.d/` that sets
`SystemdCgroup = true`, and only then set `cgroupDriver: systemd`. In that case
`cgroupRoot` becomes a systemd *slice name* rather than a path, and the systemd
driver appends `.slice` to every component, so it must be written `/k0s` (which
resolves to `k0s.slice`); writing `/k0s.slice` asks for `k0s.slice.slice` and
kubelet refuses to start:

```text
failed to run Kubelet: invalid configuration: could not read controllers for
cgroup ["k0s.slice"]: open /sys/fs/cgroup/k0s.slice.slice/cgroup.controllers:
no such file or directory
```

The cgroupfs path in this guide is the simpler of the two and is what has been
verified end to end.

### 6.2 systemd does not delegate `hugetlb`, and kubelet requires it

kubelet checks that its cgroup root carries every controller it needs. systemd
enables controllers for a subtree based on its accounting directives, and it has
no HugeTLB accounting directive, so `hugetlb` is never delegated even though the
kernel offers it:

```console
$ cat /sys/fs/cgroup/cgroup.controllers        # the kernel has it
cpuset cpu io memory hugetlb pids rdma misc
$ cat /sys/fs/cgroup/cgroup.subtree_control    # systemd delegates a subset
cpuset cpu io memory pids
```

Without it, kubelet refuses to start, repeatedly:

```text
failed to run Kubelet: invalid configuration:
cgroup ["k0s"] has some missing controllers: hugetlb
```

Enabling it on the root's `subtree_control` propagates it to `k0s.slice`. Do this
with the ordered unit from section 3(c) rather than by hand, because a manual
`echo` does not survive a reboot. Verified on the reference worker across a real
reboot: the unit ran to `success`, `hugetlb` appeared in both the root's
`subtree_control` and `k0s.slice`'s `cgroup.controllers` with no manual step, and
kubelet logged **0** missing-controller failures that boot against **17** on the
boot before the unit existed.

### 6.3 The node still advertises the whole machine

This is the important one, and no cgroup setting fixes it. kubelet reads CPU and
memory from `/proc`, not from its cgroup, so the Node advertises the whole host
while the slice permits a fraction of it:

```console
$ kubectl get node spot-worker-01 \
    -o jsonpath='{.status.allocatable.cpu} {.status.allocatable.memory}'
8 8030652Ki
```

against `CPUQuota=200%` and `MemoryMax=4G`. The scheduler will admit pods
requesting 6 CPU and 6 GiB onto a worker that can never deliver them. Reconcile
the two views with `systemReserved` in the worker profile (section 4) so
allocatable matches what the slice actually permits.

Verified on the reference worker. Before, with the cap already in force:

```console
capacity    cpu=8  mem=8133048Ki
allocatable cpu=8  mem=8030648Ki     <-- the whole host, on a 2-core/4GiB slice
```

and after adding `systemReserved: {cpu: "6", memory: 4Gi}` and restarting
`k0sworker`:

```console
capacity    cpu=8  mem=8133048Ki
allocatable cpu=2  mem=3836344Ki     <-- now matches what the slice permits
```

`cpu=2` lines up exactly with `CPUQuota=200%`. Memory lands a little *under*
`MemoryMax` rather than on it, because kubelet also holds back its hard-eviction
threshold on top of `systemReserved`. Aim `systemReserved` at the gap between
the host and the cap and let that margin fall where it may; do not try to make
allocatable equal `MemoryMax` to the byte.

**The cgroup is the enforcement; the reservation is the advertisement.** Set one
without the other and you get either an unbounded worker or a lying one.

### 6.4 Smaller traps

- **Clearing `AllowedCPUs` does not work the obvious way.** `AllowedCPUs=` with
  an empty value left `cpuset.cpus` at its previous `0-3`. Set the full range
  explicitly (`AllowedCPUs=0-7`) to undo a pin.
- **`CPUQuotaPeriodSec` rescales the quota.** Changing the period rewrites both
  fields of `cpu.max` to preserve the ratio. Set `CPUQuota` after it, not before,
  if you change both.
- **Drop-ins for units that do not exist yet are fine.** The
  `k0sworker.service.d` drop-in is written before k0s is installed and applies
  when the unit appears. Do not be tempted to install k0s first just to satisfy
  it.
- **`preStartCommands` needs `systemctl daemon-reload`** before enabling a unit
  whose file was just written by `files`.

---

## 7. Verification cookbook

### 7.1 Are the limits in place?

```console
$ systemctl show k0s.slice -p CPUQuotaPerSecUSec -p MemoryMax -p MemoryHigh -p TasksMax
CPUQuotaPerSecUSec=2s
MemoryMax=4294967296
MemoryHigh=3758096384
TasksMax=8192

$ cat /sys/fs/cgroup/k0s.slice/cpu.max
200000 100000            # 200ms per 100ms period = 2 cores
```

### 7.2 Is k0s actually inside the slice?

```console
$ systemctl show k0sworker -p Slice
Slice=k0s.slice
```

### 7.3 Are the pods inside it?

This is the check that catches a `cgroupRoot` which silently did not apply. If
`k0s-kubepods.slice` is missing here, the cap covers k0s but **not** your
workloads:

```console
$ find /sys/fs/cgroup/k0s.slice -maxdepth 1 -type d
/sys/fs/cgroup/k0s.slice
/sys/fs/cgroup/k0s.slice/k0sworker.service
/sys/fs/cgroup/k0s.slice/kubepods
```

`kubepods` with no `.slice` suffix is what you want: it confirms kubelet is on
the cgroupfs driver and therefore matching containerd. If you see
`k0s-kubepods.slice` here, kubelet is on the systemd driver and, unless you also
switched containerd, the node is in the failure mode described in 6.1.

Confirm the two agree:

```console
$ sudo grep -c 'SystemdCgroup = false' /run/k0s/containerd-cri.toml   # containerd: cgroupfs
1
$ find /sys/fs/cgroup/k0s.slice -maxdepth 1 -name 'kubepods*'          # kubelet: cgroupfs
/sys/fs/cgroup/k0s.slice/kubepods
```

### 7.4 Does the slice have the controllers it needs?

```console
$ cat /sys/fs/cgroup/k0s.slice/cgroup.controllers
cpuset cpu io memory hugetlb pids rdma misc
```

### 7.5 Raise the demand past the cap

Put a load generator in the slice and watch `cpu.stat`. `nr_throttled`
approaching `nr_periods` is the cap doing its job:

```console
$ systemd-run --unit=cpu-hog --slice=k0s.slice \
    stress-ng --cpu 8 --timeout 300s
```

Sampled over 15-second windows, 8 CPUs demanded against the 2-core cap:

| window | cores used | periods | throttled | runnable time denied |
|-------:|-----------:|--------:|----------:|---------------------:|
| 15s | 2.01 | 151 | 152 | 90.4 s |
| 30s | 2.01 | 152 | 151 | 90.4 s |
| 45s | 2.02 | 151 | 151 | 91.0 s |
| 60s | 2.02 | 151 | 152 | 90.4 s |
| 75s | 2.01 | 152 | 151 | 90.4 s |

Usage pins at 2.0 cores and essentially every period is throttled. The last
column is the arithmetic check: 8 demanded minus 2 granted is 6 cores' worth of
waiting, and 6 x 15 s = 90 s, which is what `throttled_usec` reported.
Meanwhile the host's load average sat near 1.3 on 8 cores, confirming the cap
was binding rather than the hardware.

Compute it yourself from two samples:

```bash
# cores used over N seconds = delta(usage_usec) / (N * 1e6)
awk '/usage_usec|nr_periods|nr_throttled|throttled_usec/' \
  /sys/fs/cgroup/k0s.slice/cpu.stat
```

### 7.6 Memory

```console
$ systemd-run --unit=mem-hog --slice=k0s.slice \
    stress-ng --vm 2 --vm-bytes 3G --vm-populate --timeout 45s

$ cat /sys/fs/cgroup/k0s.slice/memory.peak
3983450112                       # never breached the 4 GiB ceiling
$ cat /sys/fs/cgroup/k0s.slice/memory.events
low 0
high 164008                      # MemoryHigh absorbed it
max 0
oom 0
oom_kill 0
```

`high` climbing with `max` and `oom_kill` at zero is the healthy signature: the
cap is holding by throttling allocation. `oom_kill` above zero means a workload
would not back off.

### 7.7 Pressure (PSI), the best early warning

PSI tells you how long tasks were *stalled* waiting for a resource, which is
more actionable than utilisation. All three are available on the slice:

```console
$ cat /sys/fs/cgroup/k0s.slice/cpu.pressure
some avg10=0.02 avg60=0.36 avg300=0.20 total=892102
$ cat /sys/fs/cgroup/k0s.slice/memory.pressure
some avg10=0.00 avg60=0.00 avg300=0.00 total=5
$ cat /sys/fs/cgroup/k0s.slice/io.pressure
some avg10=0.00 avg60=0.00 avg300=0.00 total=76702580
```

`some` is the share of time at least one task was stalled; `full` (where present)
is the share where every task was. Rising `cpu.pressure` with flat utilisation
is the signature of a quota that is too tight for the workload's burstiness.

### 7.8 Live view

```console
$ systemd-cgtop k0s.slice          # per-unit CPU, memory and IO, refreshing
$ systemd-cgls k0s.slice           # the tree, with process names
$ cat /sys/fs/cgroup/k0s.slice/io.stat
254:0 rbytes=3319975936 wbytes=25616449536 rios=33992 wios=422875 dbytes=0 dios=0
$ cat /sys/fs/cgroup/k0s.slice/pids.current
104
```

### 7.9 From inside: the workload cannot see what binds it

Read the same instant from the pod's own cgroup and the picture is completely
different:

```console
$ nproc                                     # inside the container
8
$ cat /sys/fs/cgroup/cpu.max                # its own limit
max 100000
$ grep nr_throttled /sys/fs/cgroup/cpu.stat
nr_throttled 0
```

The workload believes it has 8 CPUs, has no limit of its own, and reports zero
throttling, while being held to 2 cores by an ancestor cgroup.

!!! important "Throttling from a parent slice is invisible at the child"
    Diagnose this class of problem at the slice, never from inside the workload.
    A container's `nr_throttled 0` is **not** evidence that it is running
    unconstrained, and `nproc` inside a container tells you about the host, not
    about the budget.

---

## 8. Choosing values

- **Start from the incumbent, not the workload.** The cap exists to protect
  whatever else lives on the host. Decide what the incumbent must always have,
  and give k0s the remainder.
- **Leave headroom above the workload's real peak.** The cap covers kubelet,
  containerd and the node's agents too (section 9). Sizing it to the workload's
  peak leaves nothing to manage the node with.
- **`MemoryHigh` at roughly 85 to 90% of `MemoryMax`.** The reference run used
  3584M under 4G. Too close together and you lose the grace period; too far
  apart and you waste the budget.
- **Prefer `CPUQuota` plus `CPUWeight` together.** The quota is the promise you
  made to the host's owner; the weight decides who yields first while you are
  still inside it.
- **Prefer `IOWeight` over hard IO caps.** Hard caps name a device, which does
  not belong in a portable machine definition, and they throttle image pulls.
- **Set `TasksMax` generously.** It is a fork-bomb bound, not a tuning knob.
- **Mirror every cap into `systemReserved`.** Otherwise the scheduler does not
  know about any of this (section 6.3).

---

## 9. Know what you are trading away

A single cap over `k0sworker.service` **and** `kubepods.slice` is what makes the
guarantee simple, and it is also the cost: kubelet, containerd and the node's own
agents share the budget with the workloads. Under sustained saturation the
management path degrades along with everything else. In the reference run, with
every period throttled, `kubectl exec` into the node's pods began failing with
`No agent available` while `kubectl logs` still worked, because the konnectivity
agent is itself a pod inside the capped slice.

That is the right trade for a spot worker whose whole purpose is to stay inside a
budget. It does mean the cap must leave genuine headroom above real demand.

If you need the agents insulated, give them a sibling slice outside the cap:
point kubelet at `systemReservedCgroup` and `kubeReservedCgroup` on slices that
are **not** under `k0s.slice`, and accept that the host-level guarantee becomes
the sum of two budgets rather than one number. Which is the honest trade: one
number that includes the agents, or two numbers that protect them.

---

## 10. Reference

- `systemd.resource-control(5)`: every directive in section 5, authoritative
- [Linux cgroup v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html):
  the interface files and PSI
- [k0s worker configuration](https://docs.k0sproject.io/stable/worker-node-config/):
  worker profiles and `KubeletConfiguration`
- [Kubernetes node resource management](https://kubernetes.io/docs/concepts/scheduling-eviction/node-pressure-eviction/):
  `systemReserved`, `kubeReserved`, eviction thresholds
- [Roadmap 06](https://github.com/finos/5-spot/blob/main/.github/community/06-k0smotron-e2e-and-worker-confinement.md):
  the run these measurements come from, and the open items
