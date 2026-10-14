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

load_env() {
  local env_file="${E2E_WORKDIR:-${TMPDIR:-/tmp}/5spot-e2e}/e2e.env"
  if [[ ! -f "$env_file" ]]; then
    echo "missing ${env_file}: run '.github/scripts/e2e-k0smotron.sh up' (make e2e-k0smotron-up) first" >&2
    return 1
  fi
  # shellcheck disable=SC1090
  source "$env_file"
}

kc() { kubectl --context "$KUBECTL_CONTEXT" -n "$E2E_NS" "$@"; }
# The child API is reached from inside the kind node: it is on the kind network
# under every container engine, and the host is not under rootless podman.
kchild() { "$CONTAINER_TOOL" exec "$NODE_CONTAINER" kubectl --kubeconfig "$CHILD_KUBECONFIG_IN_NODE" "$@"; }

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
  kc delete scheduledmachine "$SM" --ignore-not-found --wait=false >/dev/null 2>&1 || true
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
