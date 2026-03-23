# 5-Spot Machine Scheduler

A cloud-native Kubernetes controller for managing time-based machine scheduling on physical nodes using Cluster API (CAPI).

## Technology & Compatibility

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Kubernetes](https://img.shields.io/badge/kubernetes-1.27+-326CE5.svg?logo=kubernetes&logoColor=white)](https://kubernetes.io)
[![Cluster API](https://img.shields.io/badge/Cluster%20API-CAPI-326CE5.svg?logo=kubernetes&logoColor=white)](https://cluster-api.sigs.k8s.io/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black)](https://www.linux.org/)
[![Docker](https://img.shields.io/badge/Docker-2496ED?logo=docker&logoColor=white)](https://www.docker.com/)

## Security & Compliance

[![SPDX](https://img.shields.io/badge/SPDX-License--Identifier-blue)](https://spdx.dev/)
[![Gitleaks](https://img.shields.io/badge/Gitleaks-Secret%20Scanning-blue)](https://github.com/gitleaks/gitleaks)
[![Snyk](https://img.shields.io/badge/Snyk-SAST%20Scanning-purple)](https://snyk.io/)
[![Aqua](https://img.shields.io/badge/Aqua-Container%20Scanning-00ADD8)](https://www.aquasec.com/)

## License

[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

---

## Overview

5-Spot automatically adds and removes machines from your CAPI clusters based on time schedules. Perfect for:

- **Cost optimization**: Only run expensive hardware during business hours
- **Resource management**: Scale clusters based on predictable workload patterns
- **Energy efficiency**: Reduce power consumption during off-hours
- **Testing and staging**: Automatically provision test environments

## Features

- ⏰ **Time-based scheduling** with timezone support
- 📅 **Flexible schedules** - Support for day ranges (mon-fri) and hour ranges (9-17)
- 🔄 **Graceful shutdown** - Configurable grace periods for safe machine removal
- 🎯 **Priority-based** - Resource distribution across controller instances
- 🚨 **Kill switch** - Emergency immediate removal capability
- 📊 **Multi-instance** - Horizontal scaling with consistent hashing
- 🔍 **Full observability** - Prometheus metrics and health checks

## Quick Start

### Prerequisites

- Kubernetes cluster (1.27+)
- kubectl configured
- Cluster API (CAPI) installed

### Installation

1. **Apply the CRD:**

```bash
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

2. **Deploy the controller:**

```bash
kubectl apply -f deploy/deployment/
```

3. **Create a ScheduledMachine:**

```bash
kubectl apply -f examples/scheduledmachine-basic.yaml
```

## Example

```yaml
apiVersion: capi.5spot.io/v1alpha1
kind: ScheduledMachine
metadata:
  name: business-hours-machine
  namespace: default
spec:
  schedule:
    daysOfWeek:
      - mon-fri
    hoursOfDay:
      - 9-17
    timezone: America/New_York
    enabled: true

  machine:
    address: 192.168.1.100
    user: admin
    port: 22
    files: []

  bootstrapRef:
    apiVersion: bootstrap.cluster.x-k8s.io/v1beta1
    kind: KubeadmConfigTemplate
    name: worker-bootstrap-config
    namespace: default

  infrastructureRef:
    apiVersion: infrastructure.cluster.x-k8s.io/v1beta1
    kind: MachineTemplate
    name: worker-machine-template
    namespace: default

  clusterName: my-k0s-cluster

  priority: 50
  gracefulShutdownTimeout: 5m
```

## Development

### Prerequisites

- Rust 1.75+ (install via [rustup](https://rustup.rs))
- Python 3.10+ (for documentation)
- Poetry (install via `curl -sSL https://install.python-poetry.org | python3 -`)

### Corporate Network Setup (Optional)

If you're behind a corporate firewall or need to use an internal PyPI mirror (e.g., Artifactory), set the `PYPI_INDEX_URL` environment variable before running documentation commands:

```bash
# Set your corporate PyPI mirror URL
export PYPI_INDEX_URL=https://artifactory.example.com/api/pypi/pypi/simple

# Now documentation commands will use your mirror
make docs-serve
```

Add this to your shell profile (`~/.bashrc`, `~/.zshrc`, etc.) for persistence:

```bash
export PYPI_INDEX_URL=https://your-artifactory.example.com/api/pypi/pypi/simple
```

### Building

```bash
# Build the project
cargo build --release

# Generate CRDs
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml

# Generate API documentation
cargo run --bin crddoc > docs/reference/api.md

# Run tests
cargo test

# Run with formatting and linting
cargo fmt
cargo clippy -- -D warnings
```

### Documentation

```bash
# Serve documentation locally with live reload
make docs-serve

# Build all documentation (MkDocs + rustdoc)
make docs

# Build only Rust API docs
make docs-rustdoc
```

### Security Scanning

5-Spot includes [gitleaks](https://github.com/gitleaks/gitleaks) for secret scanning to prevent accidental credential exposure.

#### Quick Start

```bash
# Run a one-time scan of the repository
make gitleaks

# Install pre-commit hook (recommended for all developers)
make install-git-hooks

# Run all local security scans
make security-scan-local
```

#### Setting Up Gitleaks

1. **Install gitleaks** (automatic with make targets):
   ```bash
   make gitleaks-install
   ```
   This downloads and installs gitleaks with checksum verification.

2. **Install pre-commit hook** (prevents committing secrets):
   ```bash
   make install-git-hooks
   ```
   This creates a `.git/hooks/pre-commit` hook that scans staged changes before each commit.

3. **Configure allowlists** (for false positives):
   
   Edit `.gitleaks.toml` to add exceptions:
   ```toml
   [allowlist]
   paths = [
       '''tests/fixtures/''',  # Test data with fake secrets
   ]
   regexes = [
       '''example-token-.*''',  # Example tokens in docs
   ]
   ```

#### CI Integration

Gitleaks runs automatically in CI via the security scan workflow. Failed scans will:
- Block PR merges
- Create GitHub issues for detected secrets
- Generate reports in workflow artifacts

#### Troubleshooting

**False positives**: Add patterns to `.gitleaks.toml` allowlist
**Pre-commit too slow**: Use `gitleaks protect --staged` (default) instead of full repo scan
**Secrets in history**: Use [BFG Repo-Cleaner](https://rtyley.github.io/bfg-repo-cleaner/) to remove from git history

### Project Structure

```
src/
├── main.rs              # Entry point and controller setup
├── lib.rs               # Library exports
├── crd.rs               # CRD definitions (source of truth)
├── crd_tests.rs         # CRD tests
├── constants.rs         # Global constants
├── labels.rs            # Kubernetes labels
├── reconcilers/         # Reconciliation logic
│   ├── mod.rs
│   ├── scheduled_machine.rs
│   ├── helpers.rs
│   └── scheduled_machine_tests.rs
└── bin/
    ├── crdgen.rs        # CRD YAML generator
    └── crddoc.rs        # Documentation generator
```

## Architecture

### Machine Lifecycle Phases

1. **Pending** → Initial state, evaluating schedule
2. **Scheduled** → Within time window, being added to cluster
3. **Active** → Running and part of cluster
4. **Removing** → Grace period, preparing for removal
5. **Inactive** → Removed from cluster
6. **UnScheduled** → Outside time window
7. **Error** → Recoverable error state

### Schedule Evaluation

The controller evaluates schedules every 60 seconds:
- Checks current day against `daysOfWeek`
- Checks current hour against `hoursOfDay`
- Respects configured timezone
- Handles grace periods for smooth transitions

### Multi-Instance Support

Use leader election to run multiple controller replicas for high availability:

```bash
# Set environment variables
ENABLE_LEADER_ELECTION=true
LEASE_NAME=5spot-leader
```

## Configuration

### Controller Options

- `--enable-leader-election` / `ENABLE_LEADER_ELECTION`: Enable leader election (default: false)
- `--lease-name` / `LEASE_NAME`: Lease resource name (default: 5spot-leader)
- `--metrics-port` / `METRICS_PORT`: Metrics server port (default: 8080)
- `--health-port` / `HEALTH_PORT`: Health check port (default: 8081)
- `--verbose` / `-v`: Enable verbose logging

### ScheduledMachine Spec

See [API Reference](docs/reference/api.md) for complete field documentation.

## Monitoring

### Metrics

Prometheus metrics exposed on port 8080:
- `five_spot_up`: Operator health
- (More metrics to be implemented)

### Health Checks

- `/health`: Liveness probe (port 8081)
- `/ready`: Readiness probe (port 8081)

## Contributing

Contributions welcome! Please:

1. Follow Rust best practices
2. Add tests for new features
3. Update documentation
4. Run `cargo fmt` and `cargo clippy`
5. Ensure all tests pass
6. Install git hooks: `make install-git-hooks` (prevents committing secrets)

## Security

- **Gitleaks**: Pre-commit and CI secret scanning
- **Signed Commits**: Recommended for all contributors

Report security issues to the maintainers.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

- Built with [kube-rs](https://github.com/kube-rs/kube-rs)
- Designed for [k0smotron](https://github.com/k0sproject/k0smotron)
- Inspired by the need for time-based infrastructure automation
