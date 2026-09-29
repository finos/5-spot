#!/usr/bin/env bats
# shellcheck shell=bats
#
# Behavioural tests for the admission policies in deploy/admission/ — that they
# actually DENY, against a real API server.
#
# Why this exists: every control in the threat model's TB-1 and §6.5 K5 is a
# ValidatingAdmissionPolicy whose entire guarantee is one CEL expression. A typo,
# a wrong matchConstraint, or a binding that never applied all fail **open** and
# silently. The unit test in src/kata_config_agent_tests.rs asserts the
# manifest's shape; only an API server can tell you the policy fires.
#
# Requires: a reachable cluster where the caller is cluster-admin (a kind
# cluster is the intended target — `make kind-create`). Creates and deletes its
# own namespace, ServiceAccounts and RBAC; applies deploy/admission/.
#
# Run: make kind-verify-admission

NS=5spot-system
KATA_SA="system:serviceaccount:${NS}:5spot-kata-config-agent"
RECLAIM_SA="system:serviceaccount:${NS}:5spot-reclaim-agent"
CONTROLLER_SA="system:serviceaccount:${NS}:5spot-controller"
REF_KEY="5spot.finos.org/kata-config-ref"
APPLIED_KEY="5spot.finos.org/kata-config-applied"
POLICY_FILE="deploy/admission/kata-config-annotation-policy.yaml"

# All kc calls go through here, so the suite targets the same cluster the
# other kind targets do (`--context kind-<name>`) while CI can leave
# KUBECTL_CONTEXT unset and use the ambient kubeconfig.
kc() {
  if [[ -n "${KUBECTL_CONTEXT:-}" ]]; then
    kubectl --context "$KUBECTL_CONTEXT" "$@"
  else
    kubectl "$@"
  fi
}

# A patch that sets one annotation to a value, as $1 (an impersonated user).
patch_annotation() {
  local as="$1" key="$2" value="$3"
  kc --as="$as" patch "$NODE" --type=merge \
    -p "{\"metadata\":{\"annotations\":{\"${key}\":\"${value}\"}}}" 2>&1
}

setup_file() {
  cd "$BATS_TEST_DIRNAME/../.." || exit 1
  kc get nodes >/dev/null || {
    echo "no reachable cluster — run make kind-create first" >&2
    exit 1
  }
  NODE=$(kc get nodes -o name | head -1)
  export NODE

  kc create namespace "$NS" --dry-run=client -o yaml | kc apply -f - >/dev/null
  for sa in 5spot-kata-config-agent 5spot-reclaim-agent 5spot-controller; do
    kc -n "$NS" create serviceaccount "$sa" \
      --dry-run=client -o yaml | kc apply -f - >/dev/null
  done

  # The identities MUST be able to patch nodes, or a denial proves nothing: the
  # request would fail at authorization and never reach admission. This is the
  # cluster-wide grant the threat model's §8 MEDIUM entry is about, recreated
  # here on purpose.
  kc apply -f - >/dev/null <<YAML
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: 5spot-test-node-patcher
rules:
  - apiGroups: [""]
    resources: ["nodes"]
    verbs: ["get", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: 5spot-test-node-patcher
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: 5spot-test-node-patcher
subjects:
  - kind: ServiceAccount
    name: 5spot-kata-config-agent
    namespace: ${NS}
  - kind: ServiceAccount
    name: 5spot-reclaim-agent
    namespace: ${NS}
  - kind: ServiceAccount
    name: 5spot-controller
    namespace: ${NS}
YAML

  kc apply -f "$POLICY_FILE" >/dev/null

  # A policy and its binding take a moment to become active. Poll the behaviour
  # rather than a status field: the thing we need true is the denial itself.
  local deadline=$((SECONDS + 60))
  until [[ "$(patch_annotation "$KATA_SA" "$REF_KEY" probe)" == *"5-Spot controller only"* ]]; do
    if (( SECONDS > deadline )); then
      echo "policy never became active within 60s" >&2
      exit 1
    fi
    sleep 2
  done
}

teardown_file() {
  kc delete -f "$POLICY_FILE" --ignore-not-found >/dev/null 2>&1 || true
  kc delete clusterrolebinding 5spot-test-node-patcher --ignore-not-found >/dev/null 2>&1 || true
  kc delete clusterrole 5spot-test-node-patcher --ignore-not-found >/dev/null 2>&1 || true
  kc annotate "$NODE" "$REF_KEY-" "$APPLIED_KEY-" >/dev/null 2>&1 || true
}

@test "the kata agent cannot change kata-config-ref" {
  run patch_annotation "$KATA_SA" "$REF_KEY" '{"namespace":"x"}'
  [ "$status" -ne 0 ]
  # The message proves ADMISSION denied it, not RBAC: an authorization failure
  # says "cannot patch resource", and carries none of our policy's text.
  [[ "$output" == *"5-Spot controller only"* ]]
  [[ "$output" != *"cannot patch resource"* ]]
}

@test "the reclaim agent cannot change kata-config-ref either" {
  run patch_annotation "$RECLAIM_SA" "$REF_KEY" '{"namespace":"x"}'
  [ "$status" -ne 0 ]
  [[ "$output" == *"5-Spot controller only"* ]]
}

@test "the kata agent CAN still write kata-config-applied" {
  # The restart-loop guard depends on this. A policy that rejected any Node
  # update from an agent would pass the tests above and break the agent.
  run patch_annotation "$KATA_SA" "$APPLIED_KEY" "deadbeef"
  [ "$status" -eq 0 ]
}

@test "a non-agent identity CAN set kata-config-ref" {
  # The controller must not be caught by its own guard rail.
  run patch_annotation "$CONTROLLER_SA" "$REF_KEY" '{"namespace":"5spot-system"}'
  [ "$status" -eq 0 ]
}

@test "re-applying the same kata-config-ref value is not a change" {
  # oldObject == object for this key, so the expression must not fire. Without
  # this, an agent's unrelated Node patch that happens to echo the annotation
  # back would be rejected.
  run patch_annotation "$KATA_SA" "$REF_KEY" '{"namespace":"5spot-system"}'
  [ "$status" -eq 0 ]
}
