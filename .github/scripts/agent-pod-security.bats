#!/usr/bin/env bats
# shellcheck shell=bats
#
# Behavioural tests for deploy/admission/agent-pod-security-policy.yaml: the
# pod-security exception boundary for the two node agents (ADR 0004, as
# amended by ADR 0021).
#
# Why this exists: the policy is the only thing that stops an arbitrary pod in
# 5spot-system from claiming the agents' privileges, and a CEL typo or an
# unapplied binding fails OPEN, silently. Nothing exercised it before. It also
# pins the opposite failure: the policy must ADMIT the agent pods exactly as
# shipped, or a correct-looking policy would block the components it exists
# to allow. So the "admitted" cases are rendered from the real DaemonSets in
# deploy/, not hand-written pods.
#
# Every create is a server-side dry run: admission runs in full, nothing is
# persisted, and no image is pulled.
#
# Run: make kind-verify-admission

NS=5spot-system
POLICY_FILE="deploy/admission/agent-pod-security-policy.yaml"
BINDING_FILE="deploy/admission/agent-pod-security-binding.yaml"
RECLAIM_DS="deploy/node-agent/daemonset.yaml"
KATA_DS="deploy/kata-config-agent/daemonset.yaml"
CAP_DENIED="adding Linux capabilities is not allowed"

kc() {
  if [[ -n "${KUBECTL_CONTEXT:-}" ]]; then
    kubectl --context "$KUBECTL_CONTEXT" "$@"
  else
    kubectl "$@"
  fi
}

# Turn a DaemonSet manifest into the Pod it would create in $NS, optionally
# passing the pod spec through a jq filter first.
pod_from_daemonset() {
  local ds="$1" name="$2" filter="${3:-.}"
  kc create --dry-run=client -f "$ds" -o json \
    | jq --arg ns "$NS" --arg name "$name" \
      "{apiVersion: \"v1\", kind: \"Pod\",
        metadata: {name: \$name, namespace: \$ns},
        spec: (.spec.template.spec | ${filter})}"
}

# Server-side dry-run create of a Pod read from stdin; prints the response.
dry_run_create() {
  kc create --dry-run=server -f - 2>&1
}

# Render a DaemonSet's pod (through an optional jq filter) and submit it.
submit_pod() {
  pod_from_daemonset "$@" | dry_run_create
}

# The capabilities a DaemonSet's containers add, as a compact JSON list.
added_capabilities() {
  kc create --dry-run=client -f "$1" -o json \
    | jq -c '[.spec.template.spec.containers[].securityContext.capabilities.add // empty] | flatten'
}

setup_file() {
  cd "$BATS_TEST_DIRNAME/../.." || exit 1
  kc get nodes >/dev/null || {
    echo "no reachable cluster: run make kind-create first" >&2
    exit 1
  }
  kc create namespace "$NS" --dry-run=client -o yaml | kc apply -f - >/dev/null
  # The ServiceAccount admission plugin rejects a pod whose ServiceAccount
  # does not exist, which would look exactly like a policy denial.
  for sa in 5spot-reclaim-agent 5spot-kata-config-agent; do
    kc -n "$NS" create serviceaccount "$sa" --dry-run=client -o yaml | kc apply -f - >/dev/null
  done
  kc apply -f "$POLICY_FILE" -f "$BINDING_FILE" >/dev/null || {
    echo "SETUP FAILED: the policy or binding did not apply" >&2
    exit 1
  }

  # Wait until the policy is enforcing, keeping the last response so a
  # timeout reports the real reason rather than "never became active".
  local out="" deadline=$((SECONDS + 90))
  until [[ "$out" == *"$CAP_DENIED"* ]]; do
    if (( SECONDS > deadline )); then
      echo "SETUP FAILED: the policy is not denying after 90s. Last response:" >&2
      echo "  ${out:-<empty>}" >&2
      kc get validatingadmissionpolicy 5spot-agent-pod-security -o jsonpath='{.status}' >&2 || true
      exit 1
    fi
    sleep 2
    out=$(submit_pod "$RECLAIM_DS" probe \
      '.containers[0].securityContext.capabilities.add = ["NET_ADMIN"]') || true
  done
}

teardown_file() {
  kc delete -f "$BINDING_FILE" -f "$POLICY_FILE" --ignore-not-found >/dev/null 2>&1 || :
  for sa in 5spot-reclaim-agent 5spot-kata-config-agent; do
    kc -n "$NS" delete serviceaccount "$sa" --ignore-not-found >/dev/null 2>&1 || :
  done
  return 0
}

@test "the reclaim agent pod is admitted exactly as shipped" {
  run submit_pod "$RECLAIM_DS" reclaim-as-shipped
  [ "$status" -eq 0 ] || { echo "$output"; false; }
}

@test "the reclaim agent may no longer add NET_ADMIN (ADR 0021)" {
  run submit_pod "$RECLAIM_DS" reclaim-net-admin \
    '.containers[0].securityContext.capabilities.add = ["NET_ADMIN"]'
  [ "$status" -ne 0 ]
  [[ "$output" == *"$CAP_DENIED"* ]] || { echo "$output"; false; }
}

@test "the shipped reclaim agent adds no capability" {
  run added_capabilities "$RECLAIM_DS"
  [ "$status" -eq 0 ]
  [ "$output" = "[]" ]
}

@test "the kata-config agent pod is admitted exactly as shipped" {
  run submit_pod "$KATA_DS" kata-as-shipped
  [ "$status" -eq 0 ] || { echo "$output"; false; }
}

@test "the kata-config agent may not add a capability either" {
  run submit_pod "$KATA_DS" kata-sys-module \
    '.containers[0].securityContext.capabilities.add = ["SYS_MODULE"]'
  [ "$status" -ne 0 ]
  [[ "$output" == *"$CAP_DENIED"* ]] || { echo "$output"; false; }
}

@test "the kata-config agent may not mount the host root (ADR 0005 clamp)" {
  # Before the fix the policy demanded '/', so the shipped /etc/k0s mount was
  # rejected; now only /etc/k0s is admitted, and the whole root is not.
  run submit_pod "$KATA_DS" kata-host-root \
    '.volumes = [.volumes[] | if .hostPath then .hostPath.path = "/" else . end]'
  [ "$status" -ne 0 ]
  [[ "$output" == *"may only mount hostPath '/etc/k0s'"* ]] || { echo "$output"; false; }
}
