# Prerequisites

Before installing 5-Spot, ensure your environment meets the following requirements.

## Kubernetes Cluster

- **Version**: Kubernetes 1.27 or later
- **Access**: `kubectl` configured with cluster-admin privileges
- **RBAC**: Role-Based Access Control enabled

## Cluster API (CAPI)

5-Spot integrates with Cluster API for machine management.

### Required Components

- CAPI Core Provider (v1.5+)
- Bootstrap Provider (e.g., KubeadmBootstrapProvider)
- Infrastructure Provider (e.g., Metal3, Packet, vSphere)

### Verify CAPI Installation

```bash
# Check CAPI components
kubectl get pods -n capi-system
kubectl get pods -n capi-kubeadm-bootstrap-system

# Verify CRDs
kubectl get crds | grep cluster.x-k8s.io
```

## Network Requirements

| Source | Destination | Port | Protocol | Purpose |
|--------|-------------|------|----------|---------|
| Operator Pod | Kubernetes API | 443/6443 | HTTPS | API access |
| Operator Pod | Target Machines | 22 | SSH | Machine provisioning |

## Resource Requirements

### Operator Pod

| Resource | Minimum | Recommended |
|----------|---------|-------------|
| CPU | 100m | 250m |
| Memory | 128Mi | 256Mi |

### Storage

- No persistent storage required
- ConfigMaps for configuration

## Optional Components

### Prometheus (Recommended)

For metrics collection and monitoring:

```bash
kubectl get pods -n monitoring | grep prometheus
```

### Cert-Manager (For Webhooks)

If using admission webhooks:

```bash
kubectl get pods -n cert-manager
```

## Next Steps

- [Quick Start](./quickstart.md) - Get started quickly
- [Installing CRDs](./crds.md) - Install Custom Resource Definitions
- [Deploying Operator](./controller.md) - Deploy the 5-Spot controller
