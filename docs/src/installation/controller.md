# Deploying the Controller

This guide covers deploying the 5-Spot controller to your Kubernetes cluster.

## Deployment Methods

### Using kubectl

```bash
kubectl apply -f deploy/deployment/
```

### Using Helm (Coming Soon)

```bash
helm repo add 5spot https://finos.github.io/5-spot
helm install 5spot 5spot/5spot-controller
```

## Manual Deployment

### 1. Create Namespace

```bash
kubectl create namespace 5spot-system
```

### 2. Apply RBAC

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: 5spot-controller
  namespace: 5spot-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: 5spot-controller
rules:
  - apiGroups: ["5spot.finos.org"]
    resources: ["scheduledmachines", "scheduledmachines/status"]
    verbs: ["*"]
  - apiGroups: ["cluster.x-k8s.io"]
    resources: ["machines"]
    verbs: ["*"]
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]
  # Leases for leader election
  - apiGroups: ["coordination.k8s.io"]
    resources: ["leases"]
    verbs: ["get", "create", "update", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: 5spot-controller
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: 5spot-controller
subjects:
  - kind: ServiceAccount
    name: 5spot-controller
    namespace: 5spot-system
```

### 3. Deploy the Controller

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: 5spot-controller
  namespace: 5spot-system
spec:
  replicas: 1
  selector:
    matchLabels:
      app: 5spot-controller
  template:
    metadata:
      labels:
        app: 5spot-controller
    spec:
      serviceAccountName: 5spot-controller
      containers:
        - name: controller
          image: ghcr.io/finos/5-spot:latest
          env:
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
          ports:
            - containerPort: 8080
              name: metrics
            - containerPort: 8081
              name: health
          resources:
            requests:
              cpu: 100m
              memory: 128Mi
            limits:
              cpu: 500m
              memory: 256Mi
          livenessProbe:
            httpGet:
              path: /health
              port: health
          readinessProbe:
            httpGet:
              path: /ready
              port: health
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `POD_NAME` | - | Pod name (from downward API) |
| `POD_NAMESPACE` | - | Pod namespace (from downward API) |
| `ENABLE_LEADER_ELECTION` | `false` | Enable leader election for HA |
| `LEASE_NAME` | `5spot-leader` | Lease resource name |
| `LEASE_DURATION_SECONDS` | `15` | Lease validity duration |
| `METRICS_PORT` | `8080` | Prometheus metrics port |
| `HEALTH_PORT` | `8081` | Health check port |
| `RUST_LOG` | `info` | Log level |

### High Availability Deployment

For high availability, deploy multiple replicas with leader election:

```yaml
spec:
  replicas: 2
  template:
    spec:
      containers:
        - name: controller
          env:
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: ENABLE_LEADER_ELECTION
              value: "true"
            - name: LEASE_NAME
              value: "5spot-leader"
```

## Verify Deployment

```bash
# Check pods
kubectl get pods -n 5spot-system

# Check logs
kubectl logs -n 5spot-system -l app=5spot-controller

# Check health
kubectl port-forward -n 5spot-system svc/5spot-controller 8081:8081
curl http://localhost:8081/health
```

## Next Steps

- [Quick Start](./quickstart.md) - Create your first ScheduledMachine
- [Configuration](../operations/configuration.md) - Advanced configuration options
- [Monitoring](../operations/monitoring.md) - Set up monitoring
