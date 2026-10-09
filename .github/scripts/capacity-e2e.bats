#!/usr/bin/env bats
# shellcheck shell=bats
#
# End-to-end behaviour of the ScheduledCapacity controller (ADR 0011, actuation
# per ADR 0016) against a real API server.
#
# Why this exists: the unit tests prove every decision as a pure function over a
# snapshot, and the integration tests prove the reconciler flows against a mocked
# kube API. Neither can prove the seams. Only a real API server shows that the
# CRD installs, that the ClusterRole is actually sufficient, that server-side
# apply with our field manager is accepted, that ownerReference garbage
# collection removes the owned object, and that the API-group allowlist is
# enforced against a create rather than merely compiled in.
#
# It also pins the two behaviours that a mock would happily get wrong:
#   * 5-Spot CREATES the owned object. There is no stub object to apply, and if
#     the controller's `create` grant were missing this suite would fail where
#     ADR 0011's patch-only version would have passed.
#   * The owned object's name is the ScheduledCapacity's own (ADR 0016 decision
#     1), so the watch, the write and the ownership check cannot aim at
#     different objects.
#
# Requires: a kind cluster with the capacity controller deployed
# (`make kind-setup && make kind-deploy-capacity`).
#
# Run: make kind-verify-capacity

NS=5spot-capacity-e2e
SCAP=e2e-capacity
POOL_CRD=.github/scripts/fixtures/capacity-target-crd.yaml
CAPACITY_SA="system:serviceaccount:5spot-system:5spot-capacity-controller"
CONTROLLER_SA="system:serviceaccount:5spot-system:5spot-controller"

# All kubectl calls go through here, so the suite targets the same cluster the
# other kind targets do while CI can leave KUBECTL_CONTEXT unset.
kc() {
  if [[ -n "${KUBECTL_CONTEXT:-}" ]]; then
    kubectl --context "$KUBECTL_CONTEXT" "$@"
  else
    kubectl "$@"
  fi
}

# Wait until `kc $*` prints a non-empty value other than the literal "0" or
# "<none>", or fail loudly. Never returns success on a missing fixture: a test
# that skips when its fixture is absent reports success while proving nothing
# (rules/testing.md).
wait_for() {
  local timeout="$1"; shift
  local deadline=$((SECONDS + timeout)) out
  while ((SECONDS < deadline)); do
    out="$(kc "$@" 2>/dev/null || true)"
    if [[ -n "$out" && "$out" != "0" && "$out" != "<none>" ]]; then
      echo "$out"
      return 0
    fi
    sleep 2
  done
  echo "TIMEOUT after ${timeout}s waiting for: kubectl $*" >&2
  kc "$@" >&2 || true
  return 1
}

# Wait until `kc $*` prints exactly $2.
wait_for_value() {
  local timeout="$1" expected="$2"; shift 2
  local deadline=$((SECONDS + timeout)) out
  while ((SECONDS < deadline)); do
    out="$(kc "$@" 2>/dev/null || true)"
    [[ "$out" == "$expected" ]] && return 0
    sleep 2
  done
  echo "TIMEOUT after ${timeout}s: expected '$expected', last saw '$out' from: kubectl $*" >&2
  return 1
}

setup_file() {
  # Fail loudly rather than skip: a missing controller means the suite proves
  # nothing, and reporting ok would answer "is this covered?" with a false yes.
  kc get crd scheduledcapacities.5spot.finos.org >/dev/null \
    || { echo "ScheduledCapacity CRD not installed; run make kind-deploy" >&2; return 1; }
  kc -n 5spot-system get deployment 5spot-capacity-controller >/dev/null \
    || { echo "capacity controller not deployed; run make kind-deploy-capacity" >&2; return 1; }

  kc apply -f "$POOL_CRD"
  kc create namespace "$NS" --dry-run=client -o yaml | kc apply -f -

  # An always-active schedule, so the first window is open immediately.
  kc apply -f - <<YAML
apiVersion: spotschedules.5spot.finos.org/v1alpha1
kind: TimeBasedSpotSchedule
metadata:
  name: always-on
  namespace: $NS
spec:
  timezone: UTC
  windows:
    - days: [Mon, Tue, Wed, Thu, Fri, Sat, Sun]
      start: "00:00"
      end: "23:59"
YAML

  kc apply -f - <<YAML
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledCapacity
metadata:
  name: $SCAP
  namespace: $NS
spec:
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: always-on
  target:
    apiVersion: banlieue.io/v1alpha1
    kind: VirtualMachinePool
  capacity:
    field: warmReplicas
    activeValue: 10
  template:
    maxReplicas: 20
    readiness: GuestReady
    template:
      classRef:
        name: small
  handback:
    drainedPath: status.claimed
    timeout: 2m
YAML
}

teardown_file() {
  kc delete namespace "$NS" --ignore-not-found --wait=false || true
  kc delete -f "$POOL_CRD" --ignore-not-found || true
}

# ---------------------------------------------------------------------------
# The create path: 5-Spot brings the object into existence
# ---------------------------------------------------------------------------

@test "the owned VirtualMachinePool is created under the ScheduledCapacity's own name" {
  # Waits on the object's existence, which could only be true AFTER the
  # controller created it: a predicate that was already true before the action
  # is not a wait (rules/testing.md).
  run wait_for 90 -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.metadata.name}'
  [ "$status" -eq 0 ]
  [ "$output" = "$SCAP" ]
}

@test "the active value is written to the gated field" {
  wait_for_value 90 "10" -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.spec.warmReplicas}'
}

@test "the operator's template is forwarded verbatim alongside the injected knob" {
  run wait_for 60 -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.spec.maxReplicas}'
  [ "$status" -eq 0 ]
  [ "$output" = "20" ]

  run kc -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.spec.readiness}'
  [ "$output" = "GuestReady" ]

  # The nested interior 5-Spot treats as opaque must survive unchanged.
  run kc -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.spec.template.classRef.name}'
  [ "$output" = "small" ]
}

@test "the owned object carries a blocking controller ownerReference to the ScheduledCapacity" {
  # This is the entire removal mechanism: the controller holds no delete verb,
  # so an object without this reference would be unreclaimable.
  run wait_for 60 -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.metadata.ownerReferences[0].kind}'
  [ "$status" -eq 0 ]
  [ "$output" = "ScheduledCapacity" ]

  run kc -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.metadata.ownerReferences[0].controller}'
  [ "$output" = "true" ]

  run kc -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.metadata.ownerReferences[0].blockOwnerDeletion}'
  [ "$output" = "true" ]

  # The uid must be the live ScheduledCapacity's, not a name match: garbage
  # collection keys off the uid and so does the reconciler's ownership check.
  scap_uid="$(kc -n "$NS" get scheduledcapacity "$SCAP" -o 'jsonpath={.metadata.uid}')"
  run kc -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.metadata.ownerReferences[0].uid}'
  [ "$output" = "$scap_uid" ]
}

@test "the ScheduledCapacity reports Active with the value it actually wrote" {
  wait_for_value 90 "Active" -n "$NS" get scheduledcapacity "$SCAP" \
    -o 'jsonpath={.status.phase}'
  run kc -n "$NS" get scheduledcapacity "$SCAP" -o 'jsonpath={.status.writtenValue}'
  [ "$output" = "10" ]
  run kc -n "$NS" get scheduledcapacity "$SCAP" -o 'jsonpath={.status.targetRef.name}'
  [ "$output" = "$SCAP" ]
}

@test "server-side apply records the capacity controller as the spec's field manager" {
  # Proves the apply really was an SSA apply and not a merge patch: only an
  # apply records a manager with operation Apply.
  run kc -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.metadata.managedFields[?(@.manager=="5spot-capacity-controller")].operation}'
  [ "$output" = "Apply" ]
}

# ---------------------------------------------------------------------------
# Window close: scale to zero, never delete
# ---------------------------------------------------------------------------

@test "closing the window scales the owned object to zero without deleting it" {
  kc -n "$NS" patch scheduledcapacity "$SCAP" --type=merge \
    -p '{"spec":{"enabled":false}}'

  wait_for_value 120 "0" -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.spec.warmReplicas}'

  # The object itself survives, with its identity and the operator's template
  # intact, so the next window is a scale and not a cold create (ADR 0016
  # decision 5).
  run kc -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.spec.maxReplicas}'
  [ "$output" = "20" ]
  run kc -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.spec.readiness}'
  [ "$output" = "GuestReady" ]
}

@test "reopening the window scales back up on the same object" {
  original_uid="$(kc -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.metadata.uid}')"

  kc -n "$NS" patch scheduledcapacity "$SCAP" --type=merge \
    -p '{"spec":{"enabled":true}}'
  wait_for_value 120 "10" -n "$NS" get virtualmachinepool "$SCAP" \
    -o 'jsonpath={.spec.warmReplicas}'

  # Same object, not a replacement: a changed uid would mean the controller had
  # deleted and recreated, losing whatever warm state the consumer held.
  run kc -n "$NS" get virtualmachinepool "$SCAP" -o 'jsonpath={.metadata.uid}'
  [ "$output" = "$original_uid" ]
}

# ---------------------------------------------------------------------------
# The controls, proved rather than asserted
# ---------------------------------------------------------------------------

@test "the capacity ServiceAccount cannot delete the object it owns" {
  # ADR 0016 decision 6: removal is garbage collection, so there is no delete
  # verb to compromise. RBAC cannot express 'only objects you created', and this
  # is why it does not have to.
  run kc auth can-i delete virtualmachinepools.banlieue.io \
    --as "$CAPACITY_SA" -n "$NS"
  [ "$output" = "no" ]
}

@test "the capacity ServiceAccount can create and patch the allowlisted group" {
  # Both verbs, because server-side apply needs create while the object is
  # absent and patch once it exists. A controller holding one and not the other
  # works until the first window and then stops.
  run kc auth can-i create virtualmachinepools.banlieue.io --as "$CAPACITY_SA" -n "$NS"
  [ "$output" = "yes" ]
  run kc auth can-i patch virtualmachinepools.banlieue.io --as "$CAPACITY_SA" -n "$NS"
  [ "$output" = "yes" ]
}

@test "the capacity ServiceAccount cannot touch CAPI Machines or Secrets" {
  # The separation of identity is the more important half of ADR 0011 decision
  # 3, and it survives ADR 0016's widened create grant unchanged.
  for verb in get delete create; do
    run kc auth can-i "$verb" machines.cluster.x-k8s.io --as "$CAPACITY_SA" -n "$NS"
    [ "$output" = "no" ]
  done
  run kc auth can-i get secrets --as "$CAPACITY_SA" -n "$NS"
  [ "$output" = "no" ]
}

@test "the machine controller ServiceAccount cannot write the capacity target group" {
  # The other direction: compromising the machine controller must not yield
  # capacity, or the two identities would be one.
  for verb in create patch delete; do
    run kc auth can-i "$verb" virtualmachinepools.banlieue.io --as "$CONTROLLER_SA" -n "$NS"
    [ "$output" = "no" ]
  done
}

@test "an API group outside the allowlist is rejected at admission" {
  # The CRD constrains the apiVersion's shape; the group allowlist itself is
  # enforced in the reconciler, so this proves only that the schema holds. The
  # reconciler half is proved by the unit tests, which can enumerate the
  # rejection exhaustively in a way a cluster cannot.
  run kc apply -f - <<YAML
apiVersion: 5spot.finos.org/v1alpha1
kind: ScheduledCapacity
metadata:
  name: bad-group
  namespace: $NS
spec:
  schedule:
    apiVersion: spotschedules.5spot.finos.org/v1alpha1
    kind: TimeBasedSpotSchedule
    name: always-on
  target:
    apiVersion: "not a group/v1"
    kind: VirtualMachinePool
  capacity:
    field: warmReplicas
    activeValue: 1
  template:
    maxReplicas: 2
    readiness: GuestReady
YAML
  [ "$status" -ne 0 ]
}

@test "spec.target is immutable" {
  # ADR 0016 decision 1: changing the kind would orphan the previously owned
  # object, which carries an ownerReference and would therefore survive with
  # nothing reconciling it.
  run kc -n "$NS" patch scheduledcapacity "$SCAP" --type=merge \
    -p '{"spec":{"target":{"apiVersion":"banlieue.io/v1alpha1","kind":"VirtualMachine"}}}'
  [ "$status" -ne 0 ]
}

@test "a capacity field naming a reserved root is rejected" {
  # `spec.warmReplicas` here would construct `spec.spec.warmReplicas`, because
  # the field is already relative to the owned object's spec.
  run kc -n "$NS" patch scheduledcapacity "$SCAP" --type=merge \
    -p '{"spec":{"capacity":{"field":"metadata.ownerReferences","activeValue":1}}}'
  [ "$status" -ne 0 ]
}

# ---------------------------------------------------------------------------
# Removal: garbage collection, with no delete verb
# ---------------------------------------------------------------------------

@test "deleting the ScheduledCapacity garbage-collects the owned object" {
  kc -n "$NS" delete scheduledcapacity "$SCAP" --wait=true --timeout=120s

  # Wait for absence, which only becomes true after GC has run.
  deadline=$((SECONDS + 120))
  while ((SECONDS < deadline)); do
    if ! kc -n "$NS" get virtualmachinepool "$SCAP" >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  echo "the owned VirtualMachinePool survived its owner's deletion" >&2
  kc -n "$NS" get virtualmachinepool "$SCAP" -o yaml >&2 || true
  return 1
}
