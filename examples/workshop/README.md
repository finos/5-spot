# Workshop: Time-Scheduled Worker Nodes with 5-Spot + Cluster API

A hands-on, fully **local** walkthrough of 5-Spot using upstream **Cluster API**
with the **Docker provider (CAPD)** and the **kubeadm** bootstrap provider. No
cloud account, no SSH hosts, no k0smotron, no k0rdent — everything runs in Docker
on your laptop.

## The scenario

You run a dev/CI Kubernetes cluster that only needs extra worker capacity during
working hours. Paying for that worker 24/7 is wasteful. With 5-Spot you declare a
**`ScheduledMachine`** that adds a worker node when a time window opens and drains
+ removes it when the window closes — "spot capacity, on a schedule".

```
            MANAGEMENT CLUSTER (kind: 5spot-mgmt)
   ┌─────────────────────────────────────────────────────────┐
   │  Cluster API core + kubeadm + CAPD                        │
   │  5-Spot controller                                        │
   │                                                           │
   │   ScheduledMachine "business-hours-worker"                │
   │        │  (window OPEN)                                   │
   │        ▼                                                  │
   │   KubeadmConfig  +  DockerMachine  +  Machine ───────────┐│
   └──────────────────────────────────────────────────────────┘│
                                                                ▼
            WORKLOAD CLUSTER "dev-cluster" (CAPD containers)
   ┌─────────────────────────────────────────────────────────┐
   │  control-plane node  ◄── always on (KubeadmControlPlane) │
   │  business-hours-worker ◄── added/removed by 5-Spot       │
   └─────────────────────────────────────────────────────────┘
```

## What you'll learn

- How 5-Spot turns one `ScheduledMachine` into a CAPI `Machine` +
  bootstrap + infrastructure resource.
- How the time schedule drives a real Node joining and leaving a cluster.
- How 5-Spot drains the workload-cluster Node (via the CAPI-generated
  `<clusterName>-kubeconfig` Secret) before removing it.
- The kill switch and Node taints.

## Prerequisites

| Tool        | Notes |
|-------------|-------|
| Docker      | Running, with enough headroom for ~4 small containers. |
| `kind`      | ≥ v0.24. The CAPD node image tag must match — see below. |
| `kubectl`   | Any recent version. |
| `clusterctl`| The Cluster API CLI. **Pin to a release that still serves `cluster.x-k8s.io/v1beta1`** (the v1.8 / v1.9 lines do — 5-Spot emits v1beta1 Machines). |
| `make`      | To build + load the 5-Spot image (you run this; the repo never pushes images). |

> **Version pinning matters.** These manifests use `kindest/node:v1.31.0`
> throughout. Pick a `kindest/node` tag listed for *your* `kind` version, and set
> the **same** tag in `workload-cluster.yaml` (control plane) and
> `scheduledmachine-business-hours.yaml` (worker).

All commands below are run from this directory (`examples/workshop/`).

---

## Part 1 — Management cluster (kind + Cluster API + 5-Spot)

**1.1 Create the management cluster** (mounts the Docker socket so CAPD can
create sibling containers):

```bash
kind create cluster --config kind-management.yaml
# context is now: kind-5spot-mgmt
kubectl cluster-info --context kind-5spot-mgmt
```

**1.2 Install Cluster API with the Docker provider:**

```bash
clusterctl init --infrastructure docker
```

Wait until every provider Deployment is `Available`:

```bash
kubectl wait --for=condition=Available --timeout=300s -n capi-system        deploy --all
kubectl wait --for=condition=Available --timeout=300s -n capd-system        deploy --all
kubectl wait --for=condition=Available --timeout=300s -n capi-kubeadm-bootstrap-system      deploy --all
kubectl wait --for=condition=Available --timeout=300s -n capi-kubeadm-control-plane-system  deploy --all
```

**1.3 Build, load, and deploy the 5-Spot controller.** From the **repository
root** (you run the image build — the project never builds/pushes images for you):

```bash
# Build the controller image for your host arch and load it into the kind cluster.
make kind-load KIND_CLUSTER_NAME=5spot-mgmt

# Install the CRD, the admission policy (RBAC anti-escalation guard), and the controller.
kubectl --context kind-5spot-mgmt apply -f deploy/crds/
kubectl --context kind-5spot-mgmt apply -R -f deploy/deployment/
kubectl --context kind-5spot-mgmt apply -f deploy/admission/validatingadmissionpolicy.yaml
kubectl --context kind-5spot-mgmt apply -f deploy/admission/validatingadmissionpolicybinding.yaml

# Point the Deployment at the image you just loaded and wait for rollout.
# (make kind-load builds and loads ghcr.io/finos/5-spot:local-dev — the KIND_IMAGE.)
kubectl --context kind-5spot-mgmt -n 5spot-system \
  set image deployment/5spot-controller controller=ghcr.io/finos/5-spot:local-dev
kubectl --context kind-5spot-mgmt -n 5spot-system \
  rollout status deployment/5spot-controller --timeout=180s
```

> The shipped 5-Spot ClusterRole already grants `create` on
> `bootstrap.cluster.x-k8s.io/*`, `infrastructure.cluster.x-k8s.io/*`, and
> `cluster.x-k8s.io/machines` — so `KubeadmConfig`, `DockerMachine`, and `Machine`
> all pass 5-Spot's pre-flight permission check out of the box.

Confirm the CRD is healthy:

```bash
kubectl --context kind-5spot-mgmt get crd scheduledmachines.5spot.finos.org
kubectl --context kind-5spot-mgmt get sm -A     # 'sm' is the short name
```

---

## Part 2 — Workload cluster (CAPD)

**2.1 Create the workload cluster** (1 control-plane node, no workers yet):

```bash
kubectl --context kind-5spot-mgmt apply -f workload-cluster.yaml
```

Watch it come up (this provisions the LB + control-plane containers — a few minutes):

```bash
clusterctl --kubeconfig ~/.kube/config describe cluster dev-cluster
kubectl --context kind-5spot-mgmt get kubeadmcontrolplane,machine -w
```

Wait for the control plane to initialize:

```bash
kubectl --context kind-5spot-mgmt wait --for=condition=ControlPlaneInitialized \
  cluster/dev-cluster --timeout=600s
```

**2.2 Grab the workload kubeconfig and install a CNI.** CAPD does not ship a CNI,
so nodes stay `NotReady` until you install one:

```bash
clusterctl get kubeconfig dev-cluster > dev-cluster.kubeconfig

# Calico (matches the 192.168.0.0/16 pod CIDR in workload-cluster.yaml).
kubectl --kubeconfig dev-cluster.kubeconfig apply -f \
  https://raw.githubusercontent.com/projectcalico/calico/v3.28.0/manifests/calico.yaml

# The control-plane node should now go Ready.
kubectl --kubeconfig dev-cluster.kubeconfig get nodes -w
```

> 5-Spot will **auto-discover** the `dev-cluster-kubeconfig` Secret that CAPI
> created in the `default` namespace (it matches the `<clusterName>-kubeconfig`
> convention) and use it to drain/taint the worker Node later. No extra config.

---

## Part 3 — Schedule a worker with 5-Spot

The `ScheduledMachine` ships with an **always-on** window so you see the worker
join right away:

```bash
kubectl --context kind-5spot-mgmt apply -f scheduledmachine-business-hours.yaml
kubectl --context kind-5spot-mgmt get sm business-hours-worker -o wide
```

Watch 5-Spot create the three objects on the **management** cluster:

```bash
kubectl --context kind-5spot-mgmt get kubeadmconfig,dockermachine,machine \
  -l 5spot.finos.org/scheduled-machine=business-hours-worker
```

Then watch the worker Node appear on the **workload** cluster and go Ready:

```bash
kubectl --kubeconfig dev-cluster.kubeconfig get nodes -w
# business-hours-worker  Ready  <none>  ...
```

Confirm the schedule status and the taint 5-Spot applied:

```bash
kubectl --context kind-5spot-mgmt get sm business-hours-worker \
  -o custom-columns=NAME:.metadata.name,PHASE:.status.phase,READY:.status.ready,IN_SCHEDULE:.status.inSchedule
kubectl --kubeconfig dev-cluster.kubeconfig get node business-hours-worker \
  -o jsonpath='{.spec.taints}{"\n"}'
```

---

## Part 4 — Watch scheduling actually schedule

**4a. Close the window** by disabling the schedule. 5-Spot cordons + drains the
Node, then deletes the Machine/DockerMachine/KubeadmConfig:

```bash
kubectl --context kind-5spot-mgmt patch sm business-hours-worker \
  --type merge -p '{"spec":{"schedule":{"enabled":false}}}'

# Phase moves Active -> ShuttingDown -> Inactive; the Node drains and disappears.
kubectl --context kind-5spot-mgmt get sm business-hours-worker -w
kubectl --kubeconfig dev-cluster.kubeconfig get nodes -w
```

**4b. Re-open the window** — the worker comes back:

```bash
kubectl --context kind-5spot-mgmt patch sm business-hours-worker \
  --type merge -p '{"spec":{"schedule":{"enabled":true}}}'
kubectl --kubeconfig dev-cluster.kubeconfig get nodes -w
```

**4c. Try a real time window.** Edit the SM and set a window that does **not**
include "now" (in your timezone) to prove time-based removal, e.g. a past hour
range, then set it back. This is what `daysOfWeek: ["mon-fri"]` /
`hoursOfDay: ["9-17"]` does in production — outside the window, the worker is
gone (and you stop paying for it).

---

## Part 5 (bonus) — The kill switch

`killSwitch: true` removes the worker **immediately**, bypassing the graceful
window — your "get it out now" lever:

```bash
kubectl --context kind-5spot-mgmt patch sm business-hours-worker \
  --type merge -p '{"spec":{"killSwitch":true}}'
# ...then flip it back to false to let the schedule resume.
```

---

## Troubleshooting

- **Worker stuck `Provisioning`** — check the bootstrap and infra objects:
  ```bash
  kubectl --context kind-5spot-mgmt describe machine business-hours-worker
  kubectl --context kind-5spot-mgmt describe dockermachine business-hours-worker
  kubectl --context kind-5spot-mgmt logs -n capd-system deploy/capd-controller-manager
  ```
- **`ScheduledMachine` rejected at apply time** — the admission policy enforces
  that *you* (the applying user) can `create` the embedded `KubeadmConfig` /
  `DockerMachine`, and forbids `metadata.name` / `metadata.namespace` on them.
  Running as cluster-admin (the kind default) satisfies the permission check.
- **Node never goes Ready** — you skipped the CNI (Part 2.2), or the
  `kindest/node` tag doesn't match your `kind` version.
- **5-Spot can't drain the Node** — confirm the Secret exists and is discovered:
  ```bash
  kubectl --context kind-5spot-mgmt get secret dev-cluster-kubeconfig
  kubectl --context kind-5spot-mgmt logs -n 5spot-system deploy/5spot-controller | grep -i kubeconfig
  ```
- **Image pull errors for the worker** — pre-pull the node image and load it:
  `docker pull kindest/node:v1.31.0 && kind load docker-image kindest/node:v1.31.0 --name 5spot-mgmt`.

## Teardown

```bash
./teardown.sh
```

---

## How this maps to production

The only things that change for a real environment are the **providers** behind
`bootstrapSpec` and `infrastructureSpec`:

| Workshop (here)            | Production examples                                  |
|----------------------------|-----------------------------------------------------|
| `DockerMachine` (CAPD)     | `AWSMachine`, `AzureMachine`, `Metal3Machine`, `RemoteMachine`, … |
| `KubeadmConfig`            | `KubeadmConfig`, or another allowed bootstrap provider |
| `kindest/node` customImage | a real AMI / image / bare-metal host                |

The `ScheduledMachine` shape — `schedule`, `clusterName`, `machineTemplate`,
`nodeTaints`, `gracefulShutdownTimeout`, `killSwitch` — is identical. Swap the
embedded specs for your provider's types (whose API groups must be in 5-Spot's
allowlist: `bootstrap.cluster.x-k8s.io`, `infrastructure.cluster.x-k8s.io`) and
the same schedule logic provisions and reclaims real machines.
