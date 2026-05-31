#!/usr/bin/env bash
# Tear down the 5-Spot workshop environment.
#
# Order matters: delete the ScheduledMachine first so 5-Spot drains the Node and
# removes its CAPI objects cleanly, then delete the workload Cluster (CAPD
# removes its containers), then delete the management kind cluster.
set -euo pipefail

MGMT_CONTEXT="kind-5spot-mgmt"
KIND_CLUSTER="5spot-mgmt"

echo "==> Removing the ScheduledMachine (lets 5-Spot drain + clean up the worker)"
kubectl --context "${MGMT_CONTEXT}" delete -f scheduledmachine-business-hours.yaml --ignore-not-found
# Give the finalizer a moment to drain the node and delete the Machine/Docker/Kubeadm objects.
sleep 15

echo "==> Deleting the workload Cluster (CAPD tears down its containers)"
kubectl --context "${MGMT_CONTEXT}" delete -f workload-cluster.yaml --ignore-not-found --wait=false || true
# Wait for the Cluster object to be fully removed so CAPD finishes container cleanup.
kubectl --context "${MGMT_CONTEXT}" wait --for=delete cluster/dev-cluster -n default --timeout=180s || true

echo "==> Deleting the management kind cluster"
kind delete cluster --name "${KIND_CLUSTER}"

echo "==> Done. Run 'docker ps' to confirm no stray 'dev-cluster-*' containers remain."
