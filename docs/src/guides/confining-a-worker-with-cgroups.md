<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Confining a Scheduled Worker with cgroups

A `ScheduledMachine` decides **when** a host belongs to a cluster. It says
nothing about **how much** of that host k0s may consume while it is there. On a
host shared with anything else, both questions matter.

This guide caps a k0s worker and every pod it runs at a fixed CPU and memory
budget, enforced by the kernel through cgroup v2, and delivers that cap as part
of the machine definition so it is reapplied every time the schedule recreates
the worker.

The worked example caps the worker at **2 cores and 4 GiB** on a host with 8
vCPUs and 8 GiB, so the cap and not the hardware is the binding constraint.
Every value and measurement below was taken from a live run; see
[roadmap 06](https://github.com/finos/5-spot/blob/main/.github/community/06-k0smotron-e2e-and-worker-confinement.md).

!!! danger "Do not set `cgroupDriver: systemd` on k0s"
    k0s's containerd runs with `SystemdCgroup = false`, the **cgroupfs** driver.
    Overriding kubelet to `systemd` is a driver mismatch that destroys pod
    lifecycle on the node while looking like nothing of the sort. This guide
    leaves `cgroupDriver` unset deliberately. See
    [Troubleshooting](#troubleshooting) before changing it.

## How it works

A cap is only worth having if it covers the pods, and that needs two separate
things to line up:

1. **A systemd slice carrying the limits**, holding `k0sworker.service`. This
   bounds k0s, containerd and kubelet.
2. **kubelet creating its pod cgroups inside that slice**, via `cgroupRoot`.
   Without this, pods land in a sibling `kubepods` tree and the cap does not
   apply to them at all.

Cap only the service and a runaway pod is unaffected. Both together and a single
`CPUQuota` governs the union:

```text
k0s.slice                   CPUQuota=200%  MemoryMax=4G   <-- the limit lives here
├── k0sworker.service       k0s + containerd + kubelet
└── kubepods                every workload pod
    ├── burstable/pod<uid>
    └── besteffort/pod<uid>
```

`kubepods` carries no `.slice` suffix because kubelet is on the cgroupfs driver,
matching containerd. That naming is the quickest way to confirm the drivers
agree.

## Prerequisites

| Requirement | Why | Check |
| --- | --- | --- |
| **cgroup v2** | Everything here uses the v2 interface files; v1 has no `MemoryHigh` equivalent | `stat -fc %T /sys/fs/cgroup` returns `cgroup2fs` |
| **systemd** as PID 1 | The limits are expressed as unit properties | `systemctl --version` (257 in the reference run) |
| **kubelet and containerd on the same cgroup driver** | A mismatch silently destroys pod lifecycle | `sudo grep SystemdCgroup /run/k0s/containerd-cri.toml` |
| **A host with more capacity than the cap** | Otherwise you cannot tell confinement from exhaustion | 8 vCPU / 8 GiB for a 2-core / 4 GiB cap |

A host with no swap, as the reference run had, simplifies memory behaviour:
`MemoryMax` becomes a hard wall with no swap escape and `MemorySwapMax` is moot.
Check with `free -m`.

## Step 1: set the kubelet half on the control plane

kubelet's cgroup settings come from a k0s **worker profile**, which lives on the
control plane and is selected by name. Prefer a profile over
`--kubelet-extra-args`: it is structured YAML, so there is no multi-valued
argument string to quote through a shell.

```yaml
apiVersion: controlplane.cluster.x-k8s.io/v1beta2
kind: K0smotronControlPlane
metadata:
  name: spot-demo-control-plane
  namespace: spot-demo
spec:
  replicas: 3
  version: v1.34.11-k0s.1
  k0sConfig:
    apiVersion: k0s.k0sproject.io/v1beta1
    kind: ClusterConfig
    spec:
      workerProfiles:
        - name: confined
          values:
            # NO cgroupDriver here. k0s's containerd is cgroupfs; overriding
            # kubelet to systemd breaks the node. See Troubleshooting.
            cgroupRoot: /k0s.slice
            # Make the Node's advertised capacity match the cap (Step 4).
            systemReserved:
              cpu: "6"
              memory: 4Gi
```

Under the cgroupfs driver `cgroupRoot` is a **literal cgroup path**, so
`/k0s.slice` means exactly the directory `/sys/fs/cgroup/k0s.slice` that systemd
creates for the slice. kubelet builds `kubepods` inside it and the slice's limits
apply to the whole subtree.

`values` is a `KubeletConfiguration`, so anything kubelet accepts is available
here, including `evictionHard`, `kubeReserved` and `cpuManagerPolicy`.

Changing a profile requires the worker to re-read it: patch the control plane,
then `systemctl restart k0sworker` on the host, or let the schedule recreate the
machine.

## Step 2: declare the host half on the ScheduledMachine

`K0sWorkerConfig` accepts `files`, `preK0sCommands` and `args`, so the whole
confinement travels with the machine definition. Nothing needs to be baked into
the host image, and it is reapplied on every schedule activation.

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
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta2
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

        # (c) delegate the controller kubelet requires and systemd will not.
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
      preK0sCommands:
        - systemctl daemon-reload
        - systemctl enable --now k0s-cgroup-controllers.service
      args:
        - --profile=confined
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta2
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

!!! note "Field names differ by API version"
    The example targets `v1beta2`, the storage version. If you write `v1beta1`
    instead, several names differ, and a field written at `v1beta1` that does
    not exist in `v1beta2` is **silently discarded** on conversion:

    | v1beta1 | v1beta2 |
    | --- | --- |
    | `preStartCommands` | `preK0sCommands` |
    | `postStartCommands` | `postK0sCommands` |
    | `RemoteMachine.customCleanUpCommands` | `cleanUpCommands` |

    `files`, `args` and `version` are unchanged. Check the storage version's
    schema rather than trusting the served one:

    ```sh
    kubectl get crd k0sworkerconfigs.bootstrap.cluster.x-k8s.io -o json \
      | jq -r '.spec.versions[] | select(.storage)
               | .schema.openAPIV3Schema.properties.spec.properties | keys'
    ```

!!! warning "`bootstrapSpec` runs as root on the worker"
    `files` and `preK0sCommands` are executed by the bootstrap provider with
    root privilege on the target host: arbitrary files, arbitrary systemd units,
    arbitrary commands. That power is k0smotron's and this guide only uses it,
    but it is why `create` on `scheduledmachines` is a privileged grant. See the
    `bootstrapSpec` residual in the
    [threat model](../security/threat-model.md).

## Step 3: confirm the confinement landed

Apply the manifests and wait for the Machine to reach `Running` and the Node to
go `Ready`, then check all four of these on the worker.

**The limits exist:**

```console
$ systemctl show k0s.slice -p CPUQuotaPerSecUSec -p MemoryMax -p MemoryHigh -p TasksMax
CPUQuotaPerSecUSec=2s
MemoryMax=4294967296
MemoryHigh=3758096384
TasksMax=8192

$ cat /sys/fs/cgroup/k0s.slice/cpu.max
200000 100000            # 200ms per 100ms period = 2 cores
```

**k0s is inside the slice.** Trust `ControlGroup`, not `Slice`: systemd reports
the configured slice immediately but does not re-parent a running unit's cgroup
until it restarts.

```console
$ systemctl show k0sworker -p ControlGroup
ControlGroup=/k0s.slice/k0sworker.service
```

**The pods are inside it too.** This is the check that catches a `cgroupRoot`
that silently did not apply, which would leave the cap covering k0s but not your
workloads:

```console
$ find /sys/fs/cgroup/k0s.slice -maxdepth 1 -type d
/sys/fs/cgroup/k0s.slice
/sys/fs/cgroup/k0s.slice/k0sworker.service
/sys/fs/cgroup/k0s.slice/kubepods
```

**The drivers agree.** `kubepods` with no `.slice` suffix is what you want. If
you see `k0s-kubepods.slice`, kubelet is on the systemd driver and the node is
heading for the failure in [Troubleshooting](#troubleshooting):

```console
$ sudo grep -c 'SystemdCgroup = false' /run/k0s/containerd-cri.toml   # containerd: cgroupfs
1
$ find /sys/fs/cgroup/k0s.slice -maxdepth 1 -name 'kubepods*'          # kubelet: cgroupfs
/sys/fs/cgroup/k0s.slice/kubepods
```

**The slice has the controllers kubelet needs:**

```console
$ cat /sys/fs/cgroup/k0s.slice/cgroup.controllers
cpuset cpu io memory hugetlb pids rdma misc
```

## Step 4: make the Node's capacity match the cap

No cgroup setting fixes this one. kubelet reads CPU and memory from `/proc`, not
from its cgroup, so the Node advertises the whole host while the slice permits a
fraction of it. The scheduler will then admit pods the worker can never serve.

Without `systemReserved`, on a 2-core / 4 GiB slice:

```console
capacity    cpu=8  mem=8133048Ki
allocatable cpu=8  mem=8030648Ki     <-- the whole host
```

With the `systemReserved` from Step 1, after restarting `k0sworker`:

```console
capacity    cpu=8  mem=8133048Ki
allocatable cpu=2  mem=3836344Ki     <-- matches what the slice permits
```

`cpu=2` lines up exactly with `CPUQuota=200%`. Memory lands a little *under*
`MemoryMax` because kubelet also withholds its hard-eviction threshold on top of
`systemReserved`. Aim `systemReserved` at the gap between the host and the cap
and let that margin fall where it may; do not try to make allocatable equal
`MemoryMax` to the byte.

**The cgroup is the enforcement; the reservation is the advertisement.** Set one
without the other and you get either an unbounded worker or a lying one.

## Step 5: prove the limits actually bite

A structurally correct cap that does not enforce is worse than no cap. Put a
load generator in the slice and measure.

### CPU

```console
$ systemd-run --unit=cpu-hog --slice=k0s.slice stress-ng --cpu 8 --timeout 120s
```

Demanding 8 cores against the 2-core cap, sampled over 30 seconds:

| metric | value |
| --- | --- |
| cores actually used | **2.01** |
| periods throttled | **301 of 301** |
| pods during saturation | all `1/1 Running`, **0 restarts** |

Usage pins at 2.0 and every period is throttled. Compute it yourself from two
samples of `cpu.stat`: cores used over N seconds is
`delta(usage_usec) / (N * 1e6)`.

### Memory

```console
$ systemd-run --unit=mem-hog --slice=k0s.slice \
    stress-ng --vm 2 --vm-bytes 3G --vm-populate --timeout 60s
```

Demanding 6 GiB against the 4 GiB cap:

| metric | value |
| --- | --- |
| `memory.current` plateau | ~3.76 GB, at `MemoryHigh` |
| `memory.peak` | **3,874,889,728** |
| `memory.max` | 4,294,967,296 (**never breached**) |
| `memory.events` | `high 224`, `max 0`, `oom 0`, **`oom_kill 0`** |
| k0s health | `active`, kube-api probing successful |

**Always set `MemoryHigh` below `MemoryMax`.** This is the most consequential
choice on this page. With both set, a workload that respects allocation
back-pressure is slowed rather than killed: the ceiling held with zero OOM kills.
A workload that refuses to back off and holds unreclaimable anonymous pages will
still reach `MemoryMax` and be OOM-killed. `MemoryHigh` buys grace, not immunity.

### The insight worth keeping

Read the same moment from inside the workload and the picture is completely
different:

```console
$ nproc                                     # inside the container
8
$ cat /sys/fs/cgroup/cpu.max                # its own limit
max 100000
$ grep nr_throttled /sys/fs/cgroup/cpu.stat
nr_throttled 0
```

!!! important "Throttling from a parent slice is invisible at the child"
    The workload believes it has 8 CPUs, has no limit of its own, and reports
    zero throttling, while being held to 2 cores by an ancestor cgroup.
    Diagnose this class of problem **at the slice**, never from inside the
    workload. A container's `nr_throttled 0` is not evidence that it is
    unconstrained, and `nproc` inside a container tells you about the host, not
    about the budget.

## Tuning: the full control surface

Every directive below is a systemd unit property that writes a cgroup v2
interface file. The mappings were confirmed by applying each property to a live
worker with `systemctl set-property --runtime` and reading the file back.

### CPU

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `CPUQuota=200%` | `cpu.max` -> `200000 100000` | **Hard cap.** 200 ms per 100 ms period, so 2 cores |
| `CPUQuotaPeriodSec=100ms` | `cpu.max` period field | Enforcement window. The quota rescales to keep the ratio, so set it before `CPUQuota` if you change both |
| `CPUWeight=100` | `cpu.weight` | **Soft share**, 1 to 10000. Only matters under contention; never caps |
| `AllowedCPUs=0-3` | `cpuset.cpus` | Pins to specific CPUs. Changes *which* cores, not how many core-seconds |
| `CPUAccounting=yes` | enables `cpu.stat` | Required for the throttling counters |
| (read-only) | `cpu.stat` | `usage_usec`, `nr_periods`, `nr_throttled`, `throttled_usec` |
| (read-only) | `cpu.pressure` | PSI: time tasks spent stalled waiting for CPU |

**Quota or weight?** `CPUQuota` for a guarantee the worker cannot exceed, which
is the point of a spot worker. `CPUWeight` when you only want it to yield to an
incumbent under contention and use the whole box when idle. They compose: a
weight to lose politely, a quota to never exceed.

### Memory

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `MemoryMax=4G` | `memory.max` | **Hard wall.** Allocation past it triggers reclaim, then the cgroup OOM killer |
| `MemoryHigh=3584M` | `memory.high` | **Throttle point.** Over it the allocator is slowed and reclaim forced, but nothing is killed |
| `MemoryLow` / `MemoryMin` | `memory.low` / `memory.min` | Best-effort and hard protection from reclaim |
| `MemorySwapMax=0` | `memory.swap.max` | Caps swap. Moot with no swap configured |
| `MemoryAccounting=yes` | enables the counters | Required for the files below |
| (read-only) | `memory.current`, `memory.peak` | Live and high-water usage |
| (read-only) | `memory.events` | `low`, `high`, `max`, `oom`, `oom_kill` |
| (read-only) | `memory.stat` | Breakdown: `anon`, `file`, `slab`, `sock`, `pgmajfault` |
| (read-only) | `memory.pressure` | PSI for memory stalls |

### IO

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `IOWeight=100` | `io.weight` | Soft share of disk bandwidth, 1 to 10000 |
| `IOReadBandwidthMax=/dev/vda 50M` | `io.max` | Hard per-device read cap |
| `IOWriteBandwidthMax=/dev/vda 50M` | `io.max` | Hard per-device write cap |
| `IOReadIOPSMax` / `IOWriteIOPSMax` | `io.max` | Hard per-device IOPS caps |
| `IOAccounting=yes` | enables `io.stat` | Required for the counters |
| (read-only) | `io.stat`, `io.pressure` | Per-device bytes and IOPS; PSI for IO stalls |

The hard IO caps name a **device**, so they are host-specific and awkward in a
portable machine definition. `IOWeight` is usually the better choice: it protects
the incumbent under contention without pinning the config to a device path. Note
image pulls pass through this, so an aggressive read cap makes pod startup slow
in a way that is easy to misdiagnose.

### Processes and delegation

| Directive | cgroup file | Effect |
| --- | --- | --- |
| `TasksMax=8192` | `pids.max` | Hard cap on tasks, the fork-bomb bound |
| `TasksAccounting=yes` | enables `pids.current` | |

Size `TasksMax` generously: it is a safety bound, not a tuning knob. Hitting it
produces confusing `fork: Resource temporarily unavailable` failures deep inside
the runtime. 8192 was comfortable for a worker running ~104 tasks at idle.

`Delegate=` interacts with the cgroup driver, so it is worth stating carefully:

- **With the cgroupfs driver** (k0s's default, and what this guide uses) kubelet
  creates plain cgroup directories under `cgroupRoot` without going through
  systemd. The verified configuration works with `Delegate` left at its default;
  setting `Delegate=yes` also worked but `systemctl show` still reported `no`, so
  do not treat it as load-bearing. What matters is that the drivers match.
- **With the systemd driver** kubelet creates its pod slices *through* systemd,
  so delegation and that driver are contradictory.

Controller **delegation** in the kernel sense is a separate thing and is
required either way: see the `hugetlb` item in Troubleshooting.

### Observing

```console
$ systemd-cgtop k0s.slice          # per-unit CPU, memory and IO, refreshing
$ systemd-cgls k0s.slice           # the tree, with process names
$ cat /sys/fs/cgroup/k0s.slice/cpu.pressure
some avg10=0.02 avg60=0.36 avg300=0.20 total=892102
```

PSI is the best early warning: it reports how long tasks were *stalled* waiting
for a resource, which is more actionable than utilisation. Rising
`cpu.pressure` with flat utilisation is the signature of a quota too tight for
the workload's burstiness.

## Choosing values

- **Start from the incumbent, not the workload.** The cap exists to protect
  whatever else lives on the host. Decide what the incumbent must always have
  and give k0s the remainder.
- **Leave headroom above the workload's real peak.** The cap covers kubelet,
  containerd and the node's agents too.
- **`MemoryHigh` at roughly 85 to 90% of `MemoryMax`.** The reference run used
  3584M under 4G. Too close and you lose the grace period; too far and you waste
  the budget.
- **Prefer `CPUQuota` plus `CPUWeight` together.** The quota is the promise to
  the host's owner; the weight decides who yields first inside it.
- **Prefer `IOWeight` over hard IO caps.**
- **Mirror every cap into `systemReserved`** (Step 4).

## Updating the confinement

Edit the `files` content and the worker profile, then let the schedule recreate
the machine, or restart `k0sworker` on the host to pick up a profile change. For
an immediate change without re-provisioning, `systemctl set-property` applies at
runtime:

```sh
sudo systemctl set-property k0s.slice CPUQuota=300% MemoryMax=6G
```

Add `--runtime` to make it non-persistent. Note that clearing `AllowedCPUs` with
an empty value does **not** reset `cpuset.cpus`; set the full range explicitly.

## Tearing down

Deleting the `ScheduledMachine` lets 5-Spot remove the Machine and k0smotron
reset the host. The files this guide wrote are **not** removed by that reset, so
clean them up if the host is being returned to another use:

```sh
sudo systemctl disable --now k0s-cgroup-controllers.service
sudo rm -f /etc/systemd/system/k0s-cgroup-controllers.service \
           /etc/systemd/system/k0s.slice
sudo rm -rf /etc/systemd/system/k0sworker.service.d
sudo systemctl daemon-reload
```

The `+hugetlb` written to the root `cgroup.subtree_control` is runtime state and
clears on the next reboot.

## Troubleshooting

### Every pod restarts silently and the Node stays Ready

**Cause: a cgroup driver mismatch.** This is the expensive one. k0s's containerd
is configured with the cgroupfs driver:

```console
$ sudo grep SystemdCgroup /run/k0s/containerd-cri.toml
            SystemdCgroup = false
```

Setting `cgroupDriver: systemd` on kubelet makes it create and expect systemd
slices while containerd puts containers in cgroupfs paths. kubelet cannot
reconcile the two and tears down and recreates pod sandboxes indefinitely.

The symptom looks nothing like a cgroup problem:

- every pod, `hostNetwork` or not, starts perfectly (caches synced, DNS
  answering), idles, and is SIGKILLed 60 to 90 seconds later;
- containerd reports `exit_status:137` while the container's own cgroup is
  unlimited and reports `oom_kill 0`;
- `kube-proxy` dies with **no error logged at all** and has no probe that could
  explain it;
- the **Node stays `Ready`** throughout, because kubelet's own health is fine;
- restart counts climb into the hundreds over a day;
- events show `SandboxChanged: Pod sandbox changed, it will be killed and
  re-created`, and kubelet loops on `StopPodSandbox` / `TearDown network` /
  `RemovePodSandbox`;
- **no limit is approached**: 76 throttled periods of 684,221, 779 MB of 4 GiB,
  `pids` 85 of 8192, PSI 0.00. Removing the limits, the `cgroupRoot` or the
  slice entirely changes nothing, because the mismatch survives all three.

**Fix:** leave `cgroupDriver` unset. If you genuinely want the systemd driver you
must switch **both** sides, giving containerd a drop-in under
`/etc/k0s/containerd.d/` that sets `SystemdCgroup = true`, and only then setting
`cgroupDriver: systemd`. In that case `cgroupRoot` becomes a systemd *slice name*
rather than a path, and the driver appends `.slice` to each component, so it must
be written `/k0s`; `/k0s.slice` asks for `k0s.slice.slice` and kubelet refuses to
start.

### kubelet: `cgroup ["k0s.slice"] has some missing controllers: hugetlb`

kubelet checks that its cgroup root carries every controller it needs. systemd
enables controllers for a subtree from its accounting directives, and it has no
HugeTLB accounting directive, so `hugetlb` is never delegated even though the
kernel offers it:

```console
$ cat /sys/fs/cgroup/cgroup.controllers        # the kernel has it
cpuset cpu io memory hugetlb pids rdma misc
$ cat /sys/fs/cgroup/cgroup.subtree_control    # systemd delegates a subset
cpuset cpu io memory pids
```

The `k0s-cgroup-controllers.service` unit in Step 2 fixes this, ordered
`Before=k0sworker.service` so it wins at boot. Verified across a reboot: the unit
ran to `success`, `hugetlb` appeared without manual intervention, and kubelet
logged **0** missing-controller failures against **17** on the boot before the
unit existed. A manual `echo` does **not** survive a reboot, which is why this is
a unit and not a one-off command.

### `could not read controllers for cgroup ["k0s.slice"]: ... k0s.slice.slice`

You set `cgroupRoot: /k0s.slice` while kubelet is on the **systemd** driver,
which appends `.slice` to each component. Either drop the `cgroupDriver` override
(preferred, and what this guide does) or write `cgroupRoot: /k0s`.

### The cap looks set but pods are not inside it

`find /sys/fs/cgroup/k0s.slice -maxdepth 1 -type d` shows `k0sworker.service` but
no `kubepods`. The worker profile did not apply. Check that `--profile=confined`
is in the k0s argv and that the profile name matches the control plane:

```sh
systemctl cat k0sworker | grep -o '\-\-profile=[^ ]*'
```

A profile change needs `systemctl restart k0sworker` to take effect.

### `systemctl show -p Slice` disagrees with reality

systemd reports the *configured* slice immediately but does not re-parent a
running unit's cgroup on `daemon-reload`. `ControlGroup` is the authority:

```console
$ systemctl show k0sworker -p Slice -p ControlGroup
Slice=system.slice
ControlGroup=/k0s.slice/k0sworker.service     # <-- still in the old slice
```

Restart the unit, or reboot if you need certainty.

## Know what you are trading away

A single cap over `k0sworker.service` **and** `kubepods` is what makes the
guarantee simple, and it is also the cost: kubelet, containerd and the node's own
agents share the budget with the workloads. Sizing the cap to the workload's peak
leaves nothing for the node to manage itself with.

In the reference run this held up well: under full CPU saturation, with every
period throttled, all five system pods stayed `1/1 Running` with zero restarts.
So 2 cores was enough for both the agents and a saturating workload there. That
is a data point, not a guarantee, and it will not hold for an arbitrarily tight
cap.

If you need the agents insulated, give them a sibling slice outside the cap by
pointing kubelet at `systemReservedCgroup` and `kubeReservedCgroup` on slices
that are **not** under `k0s.slice`, and accept that the host-level guarantee
becomes the sum of two budgets rather than one number.

## Related

- [`systemd.resource-control(5)`](https://www.freedesktop.org/software/systemd/man/systemd.resource-control.html):
  every directive above, authoritative
- [Linux cgroup v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html):
  the interface files and PSI
- [k0s worker configuration](https://docs.k0sproject.io/stable/worker-node-config/):
  worker profiles and `KubeletConfiguration`
- [Kubernetes node-pressure eviction](https://kubernetes.io/docs/concepts/scheduling-eviction/node-pressure-eviction/):
  `systemReserved`, `kubeReserved`, eviction thresholds
- [Roadmap 06](https://github.com/finos/5-spot/blob/main/.github/community/06-k0smotron-e2e-and-worker-confinement.md):
  the run these measurements come from, including the driver-mismatch
  investigation
