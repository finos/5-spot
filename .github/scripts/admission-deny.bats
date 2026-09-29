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
  # The value lands inside a JSON string, so its own quotes must be escaped:
  # a ref value like {"namespace":"x"} otherwise breaks the patch document
  # itself, and kubectl fails to PARSE it — exit non-zero with a parse error,
  # no request ever sent. That fooled the deny tests (non-zero status, wrong
  # message) and broke the allow tests outright, while the quote-free probe
  # and applied-hash values sailed through.
  local escaped=${value//\"/\\\"}
  kc --as="$as" patch "$NODE" --type=merge \
    -p "{\"metadata\":{\"annotations\":{\"${key}\":\"${escaped}\"}}}" 2>&1
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

  # RBAC first, as its own assertion. Without this the probe below cannot tell
  # "the policy is not denying" from "the identity could never patch anyway":
  # both surface as Forbidden, and the first run of this suite spent 60s
  # reporting the wrong one.
  #
  # stderr stays OUT of the substitution: kubectl prepends
  # "Warning: resource 'nodes' is not namespace scoped" on stderr, and with
  # 2>&1 that warning is what `yes*` was matched against — the poll timed out
  # forever on a grant that had applied (its own failure diagnostics printed
  # the warning followed by "yes"). can-i's stdout is exactly yes/no, so
  # match it exactly.
  local deadline=$((SECONDS + 30))
  until [[ "$(kc auth can-i patch nodes --as="$KATA_SA" 2>/dev/null)" == "yes" ]]; do
    if (( SECONDS > deadline )); then
      echo "SETUP FAILED: $KATA_SA still cannot patch nodes after 30s." >&2
      echo "The test grant did not apply, so a denial would prove nothing:" >&2
      kc auth can-i patch nodes --as="$KATA_SA" >&2 || true
      kc get clusterrolebinding 5spot-test-node-patcher -o yaml >&2 || true
      exit 1
    fi
    sleep 2
  done

  kc apply -f "$POLICY_FILE" || {
    echo "SETUP FAILED: the policy manifest did not apply" >&2
    exit 1
  }

  # A CEL expression that fails type-checking still applies cleanly; the API
  # server records the problem in status.typeChecking and the policy does not
  # do what it says. Surface it before spending a minute on a poll.
  local typecheck
  typecheck=$(kc get validatingadmissionpolicy 5spot-kata-config-annotation \
    -o jsonpath='{.status.typeChecking}' 2>&1 || true)
  if [[ -n "$typecheck" && "$typecheck" != "{}" ]]; then
    echo "NOTE: the API server reported type-checking output for the policy:" >&2
    echo "  $typecheck" >&2
  fi

  # Start from a known state: an earlier run (or a failed probe) may have left
  # the annotation set.
  kc annotate "$NODE" "$REF_KEY-" >/dev/null 2>&1 || :

  # Now poll the behaviour — the thing we actually need true is the denial —
  # keeping the last response so a timeout reports the real reason.
  #
  # Each attempt MUST use a different value. The policy fires on a *change*, so
  # a probe that repeats one value is only a change the first time: if that
  # first attempt lands before the policy is enforcing it succeeds, writes the
  # value, and every later attempt is a no-op the policy correctly allows. The
  # poll could then only ever time out — which is exactly how the first CI run
  # of this suite failed, reporting "policy never became active" for a policy
  # that was working.
  local out="" attempt=0
  deadline=$((SECONDS + 90))
  until [[ "$out" == *"5-Spot controller only"* ]]; do
    if (( SECONDS > deadline )); then
      echo "SETUP FAILED: the policy is not denying after 90s." >&2
      echo "Last response to the probe patch:" >&2
      echo "  ${out:-<empty>}" >&2
      echo "Policy and binding as the server sees them:" >&2
      kc get validatingadmissionpolicy,validatingadmissionpolicybinding \
        -l app.kubernetes.io/name=5spot -o yaml >&2 || true
      exit 1
    fi
    sleep 2
    attempt=$((attempt + 1))
    # `|| true` is load-bearing: bats runs setup_file with errexit, and the
    # probe SUCCEEDING is kubectl exiting non-zero (the policy denied it).
    # Without the guard, the poll survives only while the policy is not yet
    # enforcing, and the first denial — the condition we are waiting for —
    # aborts the whole file at this line.
    out=$(patch_annotation "$KATA_SA" "$REF_KEY" "probe-${attempt}") || true
  done

  # Leave no probe value behind: the tests below assert on transitions.
  kc annotate "$NODE" "$REF_KEY-" >/dev/null 2>&1 || :
}

teardown_file() {
  # Every cleanup is best-effort: a teardown that fails masks the real result,
  # and bats surfaced exactly that on the first run. `|| :` on each, and an
  # explicit success at the end.
  kc delete -f "$POLICY_FILE" --ignore-not-found >/dev/null 2>&1 || :
  kc delete clusterrolebinding 5spot-test-node-patcher --ignore-not-found >/dev/null 2>&1 || :
  kc delete clusterrole 5spot-test-node-patcher --ignore-not-found >/dev/null 2>&1 || :
  kc annotate "$NODE" "$REF_KEY-" "$APPLIED_KEY-" >/dev/null 2>&1 || :
  return 0
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

# NOTE: this test depends on the one above having set the value it re-applies.
# bats runs tests in file order, so that is stable — but do not reorder them.
@test "re-applying the same kata-config-ref value is not a change" {
  # oldObject == object for this key, so the expression must not fire. Without
  # this, an agent's unrelated Node patch that happens to echo the annotation
  # back would be rejected.
  run patch_annotation "$KATA_SA" "$REF_KEY" '{"namespace":"5spot-system"}'
  [ "$status" -eq 0 ]
}
