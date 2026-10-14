#!/usr/bin/env bash
# Copyright (c) 2026 Erick Bourgeois, 5-Spot
# SPDX-License-Identifier: Apache-2.0
#
# Bring up and tear down the k0smotron end-to-end environment (ADR 0019):
#
#   kind management cluster
#     ├── cert-manager, Cluster API, k0smotron     (clusterctl, pinned)
#     ├── 5-Spot controller + time-based provider  (deploy/, image under test)
#     └── namespace $E2E_NS
#           ├── Cluster / RemoteCluster / K0smotronControlPlane   (hosted, NodePort)
#           └── Secret with a per-run SSH key
#   worker container on the kind network            (systemd + sshd, pinned digest)
#
# The ScheduledMachine itself is created by the bats suite, not here, so the
# suite owns the behaviour under test and this script owns only the fixture.
#
# Usage: e2e-k0smotron.sh cluster|up|logs|down|env
#   cluster  create only the kind cluster (so `make kind-load` has a target)
#
# Runs the same way in CI (docker) and locally (CONTAINER_TOOL=podman). Every
# step is idempotent, so `up` can be re-run after a failure.

set -euo pipefail

# ── Configuration (override from the environment) ─────────────────────────────
E2E_CLUSTER="${E2E_CLUSTER:-5spot-e2e}"
E2E_NS="${E2E_NS:-5spot-e2e}"
E2E_CHILD="${E2E_CHILD:-e2e-child}"
E2E_WORKDIR="${E2E_WORKDIR:-${TMPDIR:-/tmp}/5spot-e2e}"
CONTAINER_TOOL="${CONTAINER_TOOL:-docker}"
KIND_NODE_IMAGE="${KIND_NODE_IMAGE:-}"

CAPI_VERSION="${CAPI_VERSION:-v1.14.3}"
K0SMOTRON_VERSION="${K0SMOTRON_VERSION:-v2.2.0}"
# k0s version of the hosted control plane; the worker joins at the same one.
K0S_VERSION="${K0S_VERSION:-v1.34.11+k0s.1}"

# The worker is the image k0s uses for its own SSH-provisioned tests: systemd as
# PID 1 plus sshd. Pinned by digest so a re-push of :latest cannot change the run.
WORKER_IMAGE="${WORKER_IMAGE:-quay.io/k0sproject/bootloose-ubuntu22.04@sha256:bc20d6a1623174e35ed00ba8c72f8b08046e5f2fd48fdafe665d9015c562f4aa}"
WORKER_NAME="${WORKER_NAME:-${E2E_CLUSTER}-worker}"

# The image under test. Empty means `make kind-load` already built and loaded
# KIND_IMAGE from this checkout (CI). Set it to a published image by digest to
# run on a host that does not build images.
E2E_IMAGE="${E2E_IMAGE:-}"
KIND_IMAGE="${KIND_IMAGE:-ghcr.io/finos/5-spot:local-dev}"

# Upper bounds for the waits below, in seconds.
PROVIDERS_TIMEOUT=600
ROLLOUT_TIMEOUT=300
CONTROL_PLANE_TIMEOUT=600

CTX="kind-${E2E_CLUSTER}"
ENV_FILE="${E2E_WORKDIR}/e2e.env"
CHILD_KUBECONFIG_IN_NODE=/root/5spot-e2e-child.kubeconfig

if [[ "$CONTAINER_TOOL" == "podman" ]]; then
  export KIND_EXPERIMENTAL_PROVIDER=podman
  # podman caps every container at 2048 pids by default (docker sets no cap).
  # The kind node holds the whole management cluster plus the hosted k0s
  # control plane, which exceeds that and dies with "pthread_create failed".
  # Lift the cap for this harness's own containers only, through an override
  # file rather than any change to the host's containers.conf.
  mkdir -p "$E2E_WORKDIR"
  printf '[containers]\npids_limit = 0\n' > "${E2E_WORKDIR}/containers.conf"
  export CONTAINERS_CONF_OVERRIDE="${E2E_WORKDIR}/containers.conf"
fi

kc() { kubectl --context "$CTX" "$@"; }
log() { printf '\n==> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

require() {
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || die "'$tool' is required but not on PATH"
  done
}

# Poll `$@` until it succeeds, or fail loudly with the last output.
retry_until() {
  local timeout="$1" what="$2"; shift 2
  local deadline=$((SECONDS + timeout))
  until "$@" >/dev/null 2>&1; do
    if ((SECONDS >= deadline)); then
      echo "TIMEOUT after ${timeout}s waiting for: ${what}" >&2
      "$@" >&2 || true
      return 1
    fi
    sleep 5
  done
}

cluster_exists() { kind get clusters 2>/dev/null | grep -qx "$E2E_CLUSTER"; }
node_container() { echo "${E2E_CLUSTER}-control-plane"; }
child_kubectl() {
  "$CONTAINER_TOOL" exec "$(node_container)" kubectl --kubeconfig "$CHILD_KUBECONFIG_IN_NODE" "$@"
}

# ── Steps ─────────────────────────────────────────────────────────────────────

create_cluster() {
  if cluster_exists; then
    log "kind cluster '$E2E_CLUSTER' already exists"
    return
  fi
  log "Creating kind cluster '$E2E_CLUSTER'"
  local args=(--name "$E2E_CLUSTER" --wait 180s)
  [[ -n "$KIND_NODE_IMAGE" ]] && args+=(--image "$KIND_NODE_IMAGE")
  kind create cluster "${args[@]}"
}

install_providers() {
  if kc get deploy -n capi-system capi-controller-manager >/dev/null 2>&1 \
     && kc get deploy -n k0smotron k0smotron-controller-manager-infrastructure >/dev/null 2>&1; then
    log "Cluster API and k0smotron already installed"
  else
    # kubeadm and Docker (CAPD) are installed only so validate-examples.py can
    # check the workshop manifests against their real schemas and webhooks; no
    # CAPD machine is created, so CAPD needs no container-engine socket here.
    log "Installing cert-manager, Cluster API ${CAPI_VERSION} (+kubeadm, CAPD), k0smotron ${K0SMOTRON_VERSION}"
    clusterctl init --kubeconfig-context "$CTX" \
      --core "cluster-api:${CAPI_VERSION}" \
      --bootstrap "k0sproject-k0smotron:${K0SMOTRON_VERSION},kubeadm:${CAPI_VERSION}" \
      --control-plane "k0sproject-k0smotron:${K0SMOTRON_VERSION},kubeadm:${CAPI_VERSION}" \
      --infrastructure "k0sproject-k0smotron:${K0SMOTRON_VERSION},docker:${CAPI_VERSION}" \
      --wait-providers --wait-provider-timeout "$PROVIDERS_TIMEOUT"
  fi
  kc wait --for=condition=Available --timeout="${PROVIDERS_TIMEOUT}s" \
    -n capi-system deploy --all
  kc wait --for=condition=Available --timeout="${PROVIDERS_TIMEOUT}s" \
    -n k0smotron deploy --all
}

load_image() {
  if [[ -z "$E2E_IMAGE" ]]; then
    log "Using ${KIND_IMAGE}, built and loaded by 'make kind-load'"
    return
  fi
  log "Loading published image ${E2E_IMAGE}"
  mkdir -p "$E2E_WORKDIR"
  "$CONTAINER_TOOL" pull -q "$E2E_IMAGE" >/dev/null
  # Re-tag to KIND_IMAGE so the rest of the harness has one name to deploy.
  "$CONTAINER_TOOL" tag "$E2E_IMAGE" "$KIND_IMAGE"
  "$CONTAINER_TOOL" save -o "${E2E_WORKDIR}/image.tar" "$KIND_IMAGE"
  kind load image-archive "${E2E_WORKDIR}/image.tar" --name "$E2E_CLUSTER"
  rm -f "${E2E_WORKDIR}/image.tar"
}

deploy_5spot() {
  log "Deploying 5-Spot (CRDs, controller, time-based provider, admission policy)"
  kc apply -f deploy/crds/
  kc apply -f deploy/deployment/namespace.yaml
  retry_until 60 "namespace 5spot-system" kc get namespace 5spot-system
  kc apply -R -f deploy/deployment/
  kc apply -k deploy/spot-schedule-providers/time-based/
  kc apply -f deploy/admission/validatingadmissionpolicy.yaml
  kc apply -f deploy/admission/validatingadmissionpolicybinding.yaml

  # The image under test must already be in the node, and must never be
  # pulled: a registry tag with the same name is a different build. A failed
  # load once let the run fall back to the manifest's published image.
  if ! "$CONTAINER_TOOL" exec "$(node_container)" crictl inspecti "$KIND_IMAGE" >/dev/null 2>&1; then
    die "image ${KIND_IMAGE} is not loaded into the kind node; the build or 'kind load' failed"
  fi
  kc -n 5spot-system set image deployment/5spot-controller controller="$KIND_IMAGE"
  kc -n 5spot-system set image deployment/spot-schedule-time-based provider="$KIND_IMAGE"
  kc -n 5spot-system patch deployment/5spot-controller --type json -p \
    '[{"op":"replace","path":"/spec/template/spec/containers/0/imagePullPolicy","value":"Never"}]'
  kc -n 5spot-system patch deployment/spot-schedule-time-based --type json -p \
    '[{"op":"replace","path":"/spec/template/spec/containers/0/imagePullPolicy","value":"Never"}]'
  kc -n 5spot-system rollout status deployment/5spot-controller --timeout="${ROLLOUT_TIMEOUT}s"
  kc -n 5spot-system rollout status deployment/spot-schedule-time-based --timeout="${ROLLOUT_TIMEOUT}s"
}

start_worker() {
  mkdir -p "$E2E_WORKDIR"
  if [[ ! -f "${E2E_WORKDIR}/id_ed25519" ]]; then
    log "Generating a per-run SSH key (never leaves this run)"
    ssh-keygen -q -t ed25519 -N '' -C 5spot-e2e -f "${E2E_WORKDIR}/id_ed25519"
  fi

  if [[ "$("$CONTAINER_TOOL" inspect -f '{{.State.Running}}' "$WORKER_NAME" 2>/dev/null)" != "true" ]]; then
    log "Starting worker container ${WORKER_NAME}"
    "$CONTAINER_TOOL" rm -f -v "$WORKER_NAME" >/dev/null 2>&1 || true
    # /var/lib/k0s is a volume because k0s's containerd keeps its overlay
    # snapshots there, and overlayfs cannot be mounted on the container's own
    # overlay root ("failed to mount rootfs"). kind does the same for /var.
    local args=(run -d --name "$WORKER_NAME" --hostname "$WORKER_NAME"
      --network kind --privileged
      -v /lib/modules:/lib/modules:ro
      -v /var/lib/k0s
      --tmpfs /run --tmpfs /tmp)
    # podman needs to be told the command is an init system; docker infers it.
    [[ "$CONTAINER_TOOL" == "podman" ]] && args+=(--systemd=always)
    "$CONTAINER_TOOL" "${args[@]}" "$WORKER_IMAGE" /sbin/init >/dev/null
  fi

  retry_until 120 "sshd in ${WORKER_NAME}" \
    "$CONTAINER_TOOL" exec "$WORKER_NAME" systemctl is-active ssh
  "$CONTAINER_TOOL" exec -i "$WORKER_NAME" sh -c \
    'install -d -m 700 /root/.ssh && cat > /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys' \
    < "${E2E_WORKDIR}/id_ed25519.pub"
}

container_ip() {
  local ip
  ip="$("$CONTAINER_TOOL" inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' "$1" | awk '{print $1}')"
  [[ -n "$ip" ]] || die "no IPv4 address for container '$1' on the kind network"
  echo "$ip"
}

create_child_cluster() {
  local node_ip
  node_ip="$(container_ip "$(node_container)")"
  log "Creating child cluster ${E2E_NS}/${E2E_CHILD} (externalAddress ${node_ip})"

  kc create namespace "$E2E_NS" --dry-run=client -o yaml | kc apply -f -
  kc -n "$E2E_NS" create secret generic "${E2E_CHILD}-ssh" \
    --from-file=value="${E2E_WORKDIR}/id_ed25519" \
    --dry-run=client -o yaml | kc apply -f -

  # externalAddress must be set before the control plane exists: the generated
  # kubeconfig and the serving certificate SANs are fixed at creation and
  # k0smotron reconciles neither afterwards (roadmap 06).
  kc apply -f - <<EOF
apiVersion: cluster.x-k8s.io/v1beta2
kind: Cluster
metadata:
  name: ${E2E_CHILD}
  namespace: ${E2E_NS}
spec:
  clusterNetwork:
    pods:
      cidrBlocks: ["10.244.0.0/16"]
    services:
      cidrBlocks: ["10.96.0.0/12"]
  controlPlaneRef:
    apiGroup: controlplane.cluster.x-k8s.io
    kind: K0smotronControlPlane
    name: ${E2E_CHILD}
  infrastructureRef:
    apiGroup: infrastructure.cluster.x-k8s.io
    kind: RemoteCluster
    name: ${E2E_CHILD}
---
apiVersion: controlplane.cluster.x-k8s.io/v1beta2
kind: K0smotronControlPlane
metadata:
  name: ${E2E_CHILD}
  namespace: ${E2E_NS}
spec:
  replicas: 1
  version: ${K0S_VERSION/+/-}
  externalAddress: ${node_ip}
  service:
    type: NodePort
    apiPort: 30443
    konnectivityPort: 30132
  persistence:
    type: emptyDir
---
apiVersion: infrastructure.cluster.x-k8s.io/v1beta2
kind: RemoteCluster
metadata:
  name: ${E2E_CHILD}
  namespace: ${E2E_NS}
spec: {}
EOF

  retry_until "$CONTROL_PLANE_TIMEOUT" "${E2E_CHILD}-kubeconfig Secret" \
    kc -n "$E2E_NS" get secret "${E2E_CHILD}-kubeconfig"
  kc -n "$E2E_NS" get secret "${E2E_CHILD}-kubeconfig" -o jsonpath='{.data.value}' \
    | base64 -d > "${E2E_WORKDIR}/child.kubeconfig"
  # The child API is reached from inside the kind node, which is on the kind
  # network under every engine. The host is not, under rootless podman: its
  # networks live in a user namespace the host cannot route to.
  "$CONTAINER_TOOL" exec -i "$(node_container)" sh -c "cat > ${CHILD_KUBECONFIG_IN_NODE}" \
    < "${E2E_WORKDIR}/child.kubeconfig"
  retry_until "$CONTROL_PLANE_TIMEOUT" "child API server at ${node_ip}" child_kubectl get --raw /readyz
}

write_env() {
  cat > "$ENV_FILE" <<EOF
E2E_CLUSTER=${E2E_CLUSTER}
E2E_NS=${E2E_NS}
E2E_CHILD=${E2E_CHILD}
E2E_WORKDIR=${E2E_WORKDIR}
KUBECTL_CONTEXT=${CTX}
CONTAINER_TOOL=${CONTAINER_TOOL}
NODE_CONTAINER=$(node_container)
CHILD_KUBECONFIG_IN_NODE=${CHILD_KUBECONFIG_IN_NODE}
WORKER_NAME=${WORKER_NAME}
WORKER_IP=$(container_ip "$WORKER_NAME")
K0S_VERSION=${K0S_VERSION}
EOF
  log "Environment written to ${ENV_FILE}"
  cat "$ENV_FILE"
}

up() {
  require kind kubectl clusterctl ssh-keygen "$CONTAINER_TOOL"
  create_cluster
  install_providers
  load_image
  deploy_5spot
  start_worker
  create_child_cluster
  write_env
}

# Everything worth reading after a failure, in one place.
section() { printf '\n===== %s =====\n' "$*"; }

logs() {
  require kubectl "$CONTAINER_TOOL"
  section "ScheduledMachines, Machines and provider objects"
  kc get scheduledmachines,timebasedspotschedules -A -o wide || true
  kc get machines.cluster.x-k8s.io,k0sworkerconfigs.bootstrap.cluster.x-k8s.io,remotemachines.infrastructure.cluster.x-k8s.io -A -o wide || true
  section "Events in ${E2E_NS}"
  kc -n "$E2E_NS" get events --sort-by=.lastTimestamp || true
  section "5-Spot controller"
  kc -n 5spot-system logs deploy/5spot-controller --tail=300 || true
  section "time-based provider"
  kc -n 5spot-system logs deploy/spot-schedule-time-based --tail=100 || true
  section "k0smotron infrastructure provider"
  kc -n k0smotron logs deploy/k0smotron-controller-manager-infrastructure --tail=200 || true
  section "child cluster nodes"
  child_kubectl get nodes -o wide || true
  section "worker: k0sworker journal"
  "$CONTAINER_TOOL" exec "$WORKER_NAME" journalctl -u k0sworker --no-pager -n 200 || true
}

down() {
  require kind "$CONTAINER_TOOL"
  log "Removing worker container and kind cluster '$E2E_CLUSTER'"
  "$CONTAINER_TOOL" rm -f -v "$WORKER_NAME" >/dev/null 2>&1 || true
  if cluster_exists; then
    kind delete cluster --name "$E2E_CLUSTER"
  fi
  rm -rf "$E2E_WORKDIR"
}

case "${1:-}" in
  cluster) require kind; create_cluster ;;
  up) up ;;
  logs) logs ;;
  down) down ;;
  env) write_env ;;
  *) die "usage: $0 cluster|up|logs|down|env" ;;
esac
