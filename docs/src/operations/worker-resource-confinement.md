<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Confining a scheduled worker with systemd

A `ScheduledMachine` hands a host to a cluster for part of the day. On a host
that is *shared* with something else, "part of the day" is not enough: while
k0s is running it should also be held to a bounded slice of the machine. This
page shows how to cap a k0s worker and everything it runs at a fixed CPU and
memory budget, delivered as part of the machine definition rather than as
manual host setup, and how to prove the cap is real.

The worked example caps the worker at **2 cores and 4 GiB** on a host with 8
vCPUs and 8 GiB, so the cap (not the hardware) is the binding constraint.

## The two halves

A cap is only worth having if it covers the pods too, and that needs two
separate things to line up:

1. **A systemd slice with the limits**, holding `k0sworker.service`. This
   bounds k0s, containerd and kubelet.
2. **kubelet creating its pod cgroups inside that slice**, via `cgroupRoot`
   with the systemd cgroup driver. Without this, pods land in a sibling
   `kubepods` tree and the cap does not apply to them at all.

Cap only the service and a runaway pod is unaffected. Both halves together and
the single `CPUQuota` governs the union.

```text
k0s.slice                      CPUQuota=200%  MemoryMax=4G
├── k0sworker.service          k0s + containerd + kubelet
└── k0s-kubepods.slice         every workload pod
    ├── k0s-kubepods-burstable.slice
    └── k0s-kubepods-besteffort.slice
```

## Delivering it from the ScheduledMachine

`K0sWorkerConfig` accepts `files`, `preStartCommands` and `args`, so the whole
confinement travels with the machine definition and is reapplied every time the
schedule recreates the worker. Nothing has to be pre-baked into the host image.

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
        # 1. the slice that carries the limits
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
            CPUQuota=200%
            MemoryMax=4G
            MemoryHigh=3584M
            TasksMax=8192

        # 2. put k0s itself in it. A drop-in for a unit that does not exist
        #    yet is fine: it applies when `k0s install worker` creates it.
        - path: /etc/systemd/system/k0sworker.service.d/10-confine.conf
          permissions: "0644"
          content: |
            [Service]
            Slice=k0s.slice

        # 3. kubelet needs the hugetlb controller delegated. See the gotcha
        #    below: systemd will not do this for you.
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
      # selects the worker profile defined on the control plane (below)
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

The kubelet half is a k0s **worker profile**, which lives on the control plane
and is selected by name with `--profile`. Using a profile avoids quoting a
multi-valued `--kubelet-extra-args` string through a shell:

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
            cgroupDriver: systemd
            cgroupRoot: /k0s          # NOT /k0s.slice - see below
```

## Three things that will catch you

### `cgroupRoot` must not carry the `.slice` suffix

The systemd cgroup driver appends `.slice` to every path component. Writing
`cgroupRoot: /k0s.slice` therefore asks for `k0s.slice.slice`, and kubelet
crash-loops:

```
failed to run Kubelet: invalid configuration: could not read controllers for
cgroup ["k0s.slice"]: open /sys/fs/cgroup/k0s.slice.slice/cgroup.controllers:
no such file or directory
```

Use `cgroupRoot: /k0s` to target the slice named `k0s.slice`.

### systemd does not delegate `hugetlb`, and kubelet requires it

kubelet checks that its cgroup root carries every controller it needs. systemd
enables controllers for a subtree based on its accounting directives, and it
has no HugeTLB accounting directive, so `hugetlb` is never delegated even
though the kernel offers it:

```console
$ cat /sys/fs/cgroup/cgroup.controllers      # the kernel has it
cpuset cpu io memory hugetlb pids rdma misc
$ cat /sys/fs/cgroup/cgroup.subtree_control  # systemd delegates a subset
cpuset cpu io memory pids
```

Without it, kubelet refuses to start:

```
failed to run Kubelet: invalid configuration:
cgroup ["k0s"] has some missing controllers: hugetlb
```

Enabling it on the root's `subtree_control` propagates it to `k0s.slice`, which
is what the `k0s-cgroup-controllers.service` unit above does. It is ordered
`Before=k0sworker.service` so it is in place before kubelet's check runs.

### The node still advertises the whole machine

This is the important one, and it is not fixable with a cgroup. kubelet reads
the host's CPU and memory from `/proc`, not from its cgroup, so the Node object
advertises the whole box while the slice permits a fraction of it:

```console
$ kubectl get node spot-worker-01 \
    -o jsonpath='{.status.allocatable.cpu} {.status.allocatable.memory}'
8 8030652Ki                 # what the scheduler will commit
```

against `CPUQuota=200%` and `MemoryMax=4G`. The scheduler will happily admit
pods requesting 6 CPU and 6 GiB onto a worker that can never deliver them.
Reconcile the two views with reservations in the same worker profile, so
allocatable matches what the slice actually permits:

```yaml
      workerProfiles:
        - name: confined
          values:
            cgroupDriver: systemd
            cgroupRoot: /k0s
            systemReserved:
              cpu: "6"
              memory: 4Gi
```

Treat the cgroup as the enforcement and the reservation as the advertisement.
Setting one without the other gives you either an unbounded worker or a lying
one.

## Verifying it

### From outside: the slice is where the limit lives

```console
$ systemctl show k0s.slice -p CPUQuotaPerSecUSec -p MemoryMax
CPUQuotaPerSecUSec=2s
MemoryMax=4294967296

$ cat /sys/fs/cgroup/k0s.slice/cpu.max
200000 100000            # 200ms per 100ms period = 2 cores

$ systemctl show k0sworker -p Slice
Slice=k0s.slice
```

Confirm the pods really are inside it. This is the check that catches a
`cgroupRoot` that silently did not apply:

```console
$ find /sys/fs/cgroup/k0s.slice -maxdepth 1 -type d
/sys/fs/cgroup/k0s.slice
/sys/fs/cgroup/k0s.slice/k0sworker.service
/sys/fs/cgroup/k0s.slice/k0s-kubepods.slice
```

### Raise the demand past the cap

Put a load generator in the slice and watch `cpu.stat`. `nr_throttled`
approaching `nr_periods` is the cap doing its job:

```console
$ systemd-run --unit=cpu-hog --slice=k0s.slice \
    stress-ng --cpu 8 --timeout 300s
```

Sampled over 15-second windows with 8 CPUs demanded against a 2-core cap:

| window | cores used | periods | throttled | runnable time denied |
|-------:|-----------:|--------:|----------:|---------------------:|
| 15s | 2.01 | 151 | 152 | 90.4 s |
| 30s | 2.01 | 152 | 151 | 90.4 s |
| 45s | 2.02 | 151 | 151 | 91.0 s |
| 60s | 2.02 | 151 | 152 | 90.4 s |
| 75s | 2.01 | 152 | 151 | 90.4 s |

Usage pins at 2.0 cores, essentially every period is throttled, and ~90.4s of
runnable time is denied per 15s of wall clock. That last figure is the
arithmetic check: 8 demanded minus 2 granted is 6 cores' worth of waiting, and
6 x 15s = 90s. Meanwhile the host's load average sits near 1.3 on 8 cores,
confirming the cap is binding rather than the hardware.

### From inside: the workload cannot see what binds it

Read the same moment from the pod's own cgroup and the picture is completely
different:

```console
$ nproc                                  # inside the container
8
$ cat /sys/fs/cgroup/cpu.max             # its own limit
max 100000
$ grep nr_throttled /sys/fs/cgroup/cpu.stat
nr_throttled 0
```

The workload believes it has 8 CPUs, has no limit of its own, and reports zero
throttling, while being held to 2 cores by an ancestor cgroup. **Throttling
from a parent slice is invisible at the child.** Diagnose this class of problem
at the slice, never from inside the workload, and do not trust a container's
`nr_throttled 0` as evidence that it is running unconstrained.

### Memory

The memory ceiling holds without an OOM kill, provided `MemoryHigh` is set
below `MemoryMax`. Asking for 6 GiB inside the 4 GiB slice:

```console
$ systemd-run --unit=mem-hog --slice=k0s.slice \
    stress-ng --vm 2 --vm-bytes 3G --vm-populate --timeout 45s

$ cat /sys/fs/cgroup/k0s.slice/memory.peak     # never breached 4 GiB
3983450112
$ cat /sys/fs/cgroup/k0s.slice/memory.events
low 0
high 164008
max 0
oom 0
oom_kill 0
```

`MemoryHigh=3584M` throttled the allocator and reclaimed instead:
164,008 high-watermark events, zero `max` breaches, zero OOM kills, and the
k0s worker stayed healthy throughout. A workload that backs off under
allocation pressure is slowed rather than killed. One that refuses to back off
and holds unreclaimable anonymous pages will reach `MemoryMax` and be
OOM-killed, so set `MemoryHigh` deliberately: it is the difference between a
slow worker and a dead pod.

## Know what you are trading away

A single cap over `k0sworker.service` **and** `kubepods.slice` is what makes
the guarantee simple, and it is also the cost: kubelet, containerd and the
node's own agents share the budget with the workloads. Under sustained
saturation the management path degrades with everything else. In the run above,
with every period throttled, `kubectl exec` into the node's pods began failing
with `No agent available` while `kubectl logs` still worked, because the
konnectivity agent is itself a pod inside the capped slice.

That is the right trade for a spot worker whose whole purpose is to stay inside
a budget, but it means the cap must leave genuine headroom above the workload's
real demand. Sizing it to the workload's peak leaves nothing for the node to
manage itself with. If you need the agents insulated, give them a sibling slice
outside the cap and accept that the host-level guarantee becomes the sum of
two budgets rather than one number.
