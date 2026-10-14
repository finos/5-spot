#!/usr/bin/env bats
# shellcheck shell=bats
#
# End-to-end: a ScheduledMachine against real Cluster API and real k0smotron,
# with a worker provisioned over SSH (ADR 0019).
#
# Why this exists: the unit and integration tiers prove 5-Spot's decisions
# against a mocked API. Two defects shipped anyway because the mock accepted
# what the real system rejects (rules/testing.md, rule 1):
#   * ADR 0017: the Machine's references used the v1beta1 shape on a v1beta2
#     contract. Cluster API v1.14 refused the Machine.
#   * bug-007: the secondary watches selected on a label in a retired API
#     group, so they matched nothing.
# This suite pins both, then drives the whole lifecycle the controller exists
# for: the schedule opens, a Node joins the child cluster; it closes, the Node
# and every derived object go away; it reopens, the Node comes back; the
# ScheduledMachine is deleted, and its finalizer cleans up.
#
# Requires the environment from `.github/scripts/e2e-k0smotron.sh up`.
# Run: make e2e-k0smotron

# Generous bounds: a worker join downloads k0s and starts a kubelet.
JOIN_TIMEOUT=900
REMOVE_TIMEOUT=600
SHORT_TIMEOUT=120

SM=e2e-worker
SCHEDULE=e2e-window
LABEL_KEY=5spot.finos.org/scheduled-machine

# The agent scenarios (ADR 0020, ADR 0021). Each is its own ScheduledMachine on
# the same worker, so the Node is named after it.
RECLAIM_SM=e2e-reclaim
KATA_SM=e2e-kata
# A process name the reclaim agent matches. Short enough that /proc/<pid>/comm
# (15 characters) holds it whole.
RECLAIM_MARKER=e2ereclaim
KATA_SOURCE=e2e-kata
KATA_DROPIN=/etc/k0s/containerd.d/kata.toml
HARNESS="$BATS_TEST_DIRNAME/e2e-k0smotron.sh"

load_env() {
  local env_file="${E2E_WORKDIR:-${TMPDIR:-/tmp}/5spot-e2e}/e2e.env"
  if [[ ! -f "$env_file" ]]; then
    echo "missing ${env_file}: run '.github/scripts/e2e-k0smotron.sh up' (make e2e-k0smotron-up) first" >&2
    return 1
  fi
  # Exported, so the harness script inherits it when a test calls it.
  set -a
  # shellcheck disable=SC1090
  source "$env_file"
  set +a
}

kc() { kubectl --context "$KUBECTL_CONTEXT" -n "$E2E_NS" "$@"; }
# The child API is reached from inside the kind node: it is on the kind network
# under every container engine, and the host is not under rootless podman.
# -i so manifests piped in reach `kubectl apply -f -` inside the node.
kchild() { "$CONTAINER_TOOL" exec -i "$NODE_CONTAINER" kubectl --kubeconfig "$CHILD_KUBECONFIG_IN_NODE" "$@"; }

# Wait until `$2..` prints exactly $1, or fail with what it last printed.
wait_for_value() {
  local timeout="$1" expected="$2"; shift 2
  local deadline=$((SECONDS + timeout)) out=""
  while ((SECONDS < deadline)); do
    out="$("$@" 2>/dev/null || true)"
    [[ "$out" == "$expected" ]] && return 0
    sleep 5
  done
  echo "TIMEOUT after ${timeout}s: expected '${expected}', last saw '${out}' from: $*" >&2
  return 1
}

# Wait until `$2..` prints something non-empty, then print it.
wait_for_output() {
  local timeout="$1"; shift
  local deadline=$((SECONDS + timeout)) out=""
  while ((SECONDS < deadline)); do
    out="$("$@" 2>/dev/null || true)"
    if [[ -n "$out" ]]; then
      echo "$out"
      return 0
    fi
    sleep 5
  done
  echo "TIMEOUT after ${timeout}s waiting for output from: $*" >&2
  return 1
}

# The Node is named after the Machine (k0smotron passes the Machine name as the
# kubelet hostname unless K0sWorkerConfig sets useSystemHostname), and 5-Spot
# names the Machine after the ScheduledMachine.
NODE_NAME="$SM"

sm_phase() { kc get scheduledmachine "$SM" -o jsonpath='{.status.phase}'; }
machines_for_sm() { kc get machines.v1beta2.cluster.x-k8s.io -l "${LABEL_KEY}=${SM}" -o name; }
child_node_ready() {
  kchild get node "$NODE_NAME" \
    -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}'
}
child_node_absent() {
  if kchild get node "$NODE_NAME" >/dev/null 2>&1; then
    echo "present"
    return
  fi
  echo "absent"
}

set_window_open() {
  kc patch timebasedspotschedule "$SCHEDULE" --type merge -p "{\"spec\":{\"enabled\":$1}}"
}

setup_file() {
  load_env
  # Fail rather than skip: a missing fixture means the suite proves nothing,
  # and reporting ok would answer "is this covered?" with a false yes.
  kubectl --context "$KUBECTL_CONTEXT" -n 5spot-system rollout status \
    deployment/5spot-controller --timeout=60s >/dev/null
  kubectl --context "$KUBECTL_CONTEXT" -n 5spot-system rollout status \
    deployment/spot-schedule-time-based --timeout=60s >/dev/null
  kchild get --raw /readyz >/dev/null

  kc apply -f - <<EOF
apiVersion: spotschedules.5spot.finos.org/v1alpha1
kind: TimeBasedSpotSchedule
metadata:
  name: ${SCHEDULE}
spec:
  # Every hour of every day: the window is controlled by spec.enabled below,
  # so the suite never depends on the wall clock.
  daysOfWeek: ["mon-sun"]
  hoursOfDay: ["0-23"]
  timezone: UTC
  enabled: true
---
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: ${SM}
spec:
  clusterName: ${E2E_CHILD}
  enabled: true
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: ${SCHEDULE}
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta2
    kind: K0sWorkerConfig
    spec:
      version: ${K0S_VERSION}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta2
    kind: RemoteMachine
    spec:
      address: ${WORKER_IP}
      port: 22
      user: root
      sshKeyRef:
        name: ${E2E_CHILD}-ssh
  gracefulShutdownTimeout: 2m
  nodeDrainTimeout: 2m
EOF
}

teardown_file() {
  load_env || return 0
  # Best effort: leave the fixture reusable for a re-run.
  "$CONTAINER_TOOL" exec "$WORKER_NAME" pkill -x "$RECLAIM_MARKER" >/dev/null 2>&1 || true
  for sm in "$SM" "$RECLAIM_SM" "$KATA_SM"; do
    kc delete scheduledmachine "$sm" --ignore-not-found --wait=false >/dev/null 2>&1 || true
  done
  kc delete timebasedspotschedule "$SCHEDULE" --ignore-not-found >/dev/null 2>&1 || true
}

setup() {
  load_env
}

@test "the schedule opens and 5-Spot writes the Machine at v1beta2 (ADR 0017 passthrough)" {
  local machine
  machine="$(wait_for_output "$SHORT_TIMEOUT" machines_for_sm)"
  # managedFields record the API version of each writer's own request, so this
  # proves 5-Spot wrote at v1beta2. Reading the object back cannot: CAPI's
  # conversion would serve any stored Machine at v1beta2.
  run kc get "$machine" -o jsonpath='{range .metadata.managedFields[*]}{.apiVersion}{"\n"}{end}'
  [ "$status" -eq 0 ]
  [[ "$output" == *"cluster.x-k8s.io/v1beta2"* ]]
  [[ "$output" != *"cluster.x-k8s.io/v1beta1"* ]]
}

@test "the Machine's references use the v1beta2 ContractVersionedObjectReference shape (ADR 0017)" {
  # Meaningful together with the test above: a v1beta2 create with the legacy
  # apiVersion/namespace shape is what Cluster API v1.14 rejected.
  local machine
  machine="$(wait_for_output "$SHORT_TIMEOUT" machines_for_sm)"
  run kc get "$machine" \
    -o jsonpath='{.spec.bootstrap.configRef.apiGroup}|{.spec.bootstrap.configRef.kind}|{.spec.infrastructureRef.apiGroup}|{.spec.infrastructureRef.kind}'
  [ "$status" -eq 0 ]
  [ "$output" = "bootstrap.cluster.x-k8s.io|K0sWorkerConfig|infrastructure.cluster.x-k8s.io|RemoteMachine" ]
}

@test "the Machine carries the controller-owned label the secondary watches select on (bug-007)" {
  run wait_for_output "$SHORT_TIMEOUT" machines_for_sm
  [ "$status" -eq 0 ]
  [ "$(echo "$output" | wc -l)" -eq 1 ]
}

@test "the worker joins the child cluster and the ScheduledMachine goes Active" {
  wait_for_value "$JOIN_TIMEOUT" "True" child_node_ready
  wait_for_value "$SHORT_TIMEOUT" "Active" sm_phase
  run kc get scheduledmachine "$SM" -o jsonpath='{.status.nodeRef.name}'
  [ "$output" = "$NODE_NAME" ]
}

@test "closing the window removes the Node, the Machine and both derived objects" {
  set_window_open false
  wait_for_value "$REMOVE_TIMEOUT" "absent" child_node_absent
  wait_for_value "$REMOVE_TIMEOUT" "" machines_for_sm
  wait_for_value "$SHORT_TIMEOUT" "" \
    kc get k0sworkerconfigs.bootstrap.cluster.x-k8s.io,remotemachines.infrastructure.cluster.x-k8s.io \
      -l "${LABEL_KEY}=${SM}" -o name
  wait_for_value "$SHORT_TIMEOUT" "Inactive" sm_phase
}

@test "reopening the window brings the same host back as a Ready Node" {
  set_window_open true
  wait_for_value "$JOIN_TIMEOUT" "True" child_node_ready
  wait_for_value "$SHORT_TIMEOUT" "Active" sm_phase
}

@test "deleting the ScheduledMachine removes everything it created" {
  kc delete scheduledmachine "$SM" --wait=false
  wait_for_value "$REMOVE_TIMEOUT" "" \
    kc get scheduledmachine "$SM" --ignore-not-found -o name
  wait_for_value "$SHORT_TIMEOUT" "" machines_for_sm
  wait_for_value "$REMOVE_TIMEOUT" "absent" child_node_absent
}

# ---------------------------------------------------------------------------
# Helpers for the agent scenarios
# ---------------------------------------------------------------------------

# A ScheduledMachine on the shared worker, in the open window, with extra spec.
apply_agent_sm() {
  local name="$1" extra="$2"
  kc apply -f - <<EOF
apiVersion: 5spot.finos.org/v1beta1
kind: ScheduledMachine
metadata:
  name: ${name}
spec:
  clusterName: ${E2E_CHILD}
  enabled: true
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: ${SCHEDULE}
  bootstrapSpec:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta2
    kind: K0sWorkerConfig
    spec:
      version: ${K0S_VERSION}
  infrastructureSpec:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta2
    kind: RemoteMachine
    spec:
      address: ${WORKER_IP}
      port: 22
      user: root
      sshKeyRef:
        name: ${E2E_CHILD}-ssh
  gracefulShutdownTimeout: 2m
  nodeDrainTimeout: 2m
${extra}
EOF
}

node_ready() {
  kchild get node "$1" -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}'
}
node_absent() {
  if kchild get node "$1" >/dev/null 2>&1; then
    echo "present"
    return
  fi
  echo "absent"
}
node_label() {
  kchild get node "$1" -o jsonpath="{.metadata.labels.${2//./\\.}}"
}
agent_pod_phase() {
  kchild -n 5spot-system get pods -l "app=$1" --field-selector "spec.nodeName=$2" \
    -o jsonpath='{.items[0].status.phase}'
}
agent_container_ready() {
  kchild -n 5spot-system get pods -l "app=$1" --field-selector "spec.nodeName=$2" \
    -o jsonpath='{.items[0].status.containerStatuses[0].ready}'
}
worker() { "$CONTAINER_TOOL" exec "$WORKER_NAME" "$@"; }

# ---------------------------------------------------------------------------
# Reclaim agent (ADR 0020 subcommand, ADR 0021 detector)
# ---------------------------------------------------------------------------

@test "reclaim: a ScheduledMachine with killIfCommands gets the agent on its node" {
  apply_agent_sm "$RECLAIM_SM" "  killIfCommands: [\"${RECLAIM_MARKER}\"]"
  wait_for_value "$JOIN_TIMEOUT" "True" node_ready "$RECLAIM_SM"
  "$HARNESS" load-worker-image
  # The controller opts the node in; the shipped DaemonSet (poll detector, no
  # added capability) then lands on it and must actually run.
  wait_for_value "$SHORT_TIMEOUT" "enabled" node_label "$RECLAIM_SM" "5spot.finos.org/reclaim-agent"
  wait_for_value "$SHORT_TIMEOUT" "Running" agent_pod_phase 5spot-reclaim-agent "$RECLAIM_SM"
  wait_for_value "$SHORT_TIMEOUT" "true" agent_container_ready 5spot-reclaim-agent "$RECLAIM_SM"
}

@test "reclaim: a matching process on the node triggers an emergency removal" {
  # Start the process on the node itself: the worker container is the node.
  "$CONTAINER_TOOL" exec -d "$WORKER_NAME" /bin/sh -c \
    "cp /bin/sleep /usr/local/bin/${RECLAIM_MARKER} && exec /usr/local/bin/${RECLAIM_MARKER} 3600"
  # The controller ejects the node and disables the ScheduledMachine so it does
  # not come straight back.
  wait_for_value "$REMOVE_TIMEOUT" "false" \
    kc get scheduledmachine "$RECLAIM_SM" -o jsonpath='{.spec.enabled}'
  wait_for_value "$REMOVE_TIMEOUT" "" \
    kc get machines.v1beta2.cluster.x-k8s.io -l "${LABEL_KEY}=${RECLAIM_SM}" -o name
  wait_for_value "$REMOVE_TIMEOUT" "absent" node_absent "$RECLAIM_SM"
}

@test "reclaim: clean up" {
  worker pkill -x "$RECLAIM_MARKER" || true
  kc delete scheduledmachine "$RECLAIM_SM" --ignore-not-found --wait=false
  wait_for_value "$REMOVE_TIMEOUT" "" kc get scheduledmachine "$RECLAIM_SM" --ignore-not-found -o name
}

# ---------------------------------------------------------------------------
# Kata-config agent: the restart goes through `5spot nsenter` (ADR 0020)
# ---------------------------------------------------------------------------

@test "kata: the drop-in reaches the node and k0sworker restarts through 5spot nsenter" {
  local content="# 5-Spot e2e Kata drop-in, run ${BATS_ROOT_PID:-$$}"
  kchild -n 5spot-system create configmap "$KATA_SOURCE" \
    --from-literal=kata-containers.toml="$content" --dry-run=client -o yaml \
    | kchild apply -f -
  apply_agent_sm "$KATA_SM" "  kata:
    kind: ConfigMap
    name: ${KATA_SOURCE}"
  wait_for_value "$JOIN_TIMEOUT" "True" node_ready "$KATA_SM"

  # The worker's k0s service instance before the agent can run (the image is
  # not on the node yet, so the agent pod cannot have started).
  local before
  before="$(worker systemctl show -p InvocationID --value k0sworker)"
  [ -n "$before" ]
  "$HARNESS" load-worker-image

  wait_for_value "$SHORT_TIMEOUT" "enabled" node_label "$KATA_SM" "5spot.finos.org/kata-config"
  # The privileged agent writes the fixed host path...
  wait_for_value "$JOIN_TIMEOUT" "$content" worker cat "$KATA_DROPIN"
  # ...records what it applied on its own Node...
  wait_for_output "$SHORT_TIMEOUT" \
    kchild get node "$KATA_SM" -o 'jsonpath={.metadata.annotations.5spot\.finos\.org/kata-config-applied}'
  # ...and restarts the host k0s service by entering host PID 1's namespaces.
  # The image has no nsenter or systemctl, so a new invocation of the unit can
  # only come from the 5spot nsenter subcommand.
  local deadline=$((SECONDS + JOIN_TIMEOUT)) after="$before"
  while ((SECONDS < deadline)) && [[ "$after" == "$before" ]]; do
    sleep 5
    after="$(worker systemctl show -p InvocationID --value k0sworker 2>/dev/null || echo "$before")"
  done
  [[ "$after" != "$before" ]] || { echo "k0sworker was never restarted (InvocationID still $before)" >&2; false; }
  wait_for_value "$JOIN_TIMEOUT" "True" node_ready "$KATA_SM"
}

@test "kata: clean up" {
  kc delete scheduledmachine "$KATA_SM" --ignore-not-found --wait=false
  wait_for_value "$REMOVE_TIMEOUT" "" kc get scheduledmachine "$KATA_SM" --ignore-not-found -o name
  wait_for_value "$REMOVE_TIMEOUT" "absent" node_absent "$KATA_SM"
}

