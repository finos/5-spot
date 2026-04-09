[![FINOS - Incubating](https://cdn.jsdelivr.net/gh/finos/contrib-toolbox@master/images/badge-incubating.svg)](https://community.finos.org/docs/governance/Software-Projects/stages/incubating)

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
- 🔄 **Graceful shutdown** - Configurable grace periods with automatic node draining
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

### Cross-Compilation Setup

To build Docker images for Linux from macOS, you need a cross-compilation toolchain. The recommended approach is using the `cross` tool:

```bash
# Install cross (recommended - handles everything via Docker)
cargo install cross

# Build Docker images
make docker-build          # Auto-detect, defaults to linux/amd64
make docker-build-amd64    # Explicitly build for linux/amd64
make docker-build-arm64    # Explicitly build for linux/arm64
```

**Alternative:** Install GNU cross-compilation toolchains directly (for faster builds without Docker overhead):

```bash
# Add the cross-toolchains tap
brew tap messense/macos-cross-toolchains

# For linux/amd64 from macOS
brew install x86_64-unknown-linux-gnu
rustup target add x86_64-unknown-linux-gnu

# For linux/arm64 from macOS
brew install aarch64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu
```

The project includes `.cargo/config.toml` with linker configuration for these targets. If building in a different directory, create this config:

```toml
# .cargo/config.toml
[target.x86_64-unknown-linux-gnu]
linker = "x86_64-unknown-linux-gnu-gcc"

[target.aarch64-unknown-linux-gnu]
linker = "aarch64-unknown-linux-gnu-gcc"
```

**Note:** The `rustup target add` alone is not sufficient - crates with C dependencies (like `ring`) require a linker from the GNU toolchain.

### Air-Gapped / Corporate Network Setup

If you're in an air-gapped environment or behind a corporate firewall without access to public registries (`crates.io`, `static.rust-lang.org`, `pypi.org`, `gcr.io`), you'll need to configure alternative registries.

#### 1. Cargo Registry (Artifactory)

Create a `.cargo/config.toml` in a dedicated directory to route Rust crates through your Artifactory mirror:

```bash
mkdir -p ~/.cargo-airgap
cat > ~/.cargo-airgap/config.toml << 'EOF'
# Air-gapped Cargo configuration using Artifactory
[registry]
default = "artifactory"

[registries.artifactory]
index = "sparse+https://artifactory.example.com/artifactory/api/cargo/crates-io-remote/index/"

[source.artifactory-remote]
registry = "sparse+https://artifactory.example.com/artifactory/api/cargo/crates-io-remote/index/"

[source.crates-io]
replace-with = "artifactory-remote"
EOF
```

Then use `AIRGAP_CARGO_HOME` when building:

```bash
# Build binaries using Artifactory registry
AIRGAP_CARGO_HOME=~/.cargo-airgap make docker-build-amd64

# Or for arm64
AIRGAP_CARGO_HOME=~/.cargo-airgap make docker-build-arm64
```

#### 2. PyPI Mirror (Documentation)

Set `PYPI_INDEX_URL` for documentation builds:

```bash
export PYPI_INDEX_URL=https://artifactory.example.com/api/pypi/pypi/simple
make docs-serve
```

#### 3. Docker Buildx (Container Registries)

For Docker builds, configure buildx to trust your Artifactory mirrors for container images:

```bash
# Create buildx config directory
mkdir -p ~/.docker/buildx

# Create buildkitd.toml for Artifactory registries
cat > ~/.docker/buildx/buildkitd.toml << 'EOF'
# BuildKit daemon configuration for air-gapped environments
# Skip TLS verification for internal registries

[registry."artifactory.example.com"]
  insecure = true

# Add mirrors for common registries (gcr.io, ghcr.io)
[registry."oss-docker-gcr.artifactory.example.com"]
  insecure = true

[registry."oss-docker-ghcr.artifactory.example.com"]
  insecure = true
EOF

# Create the buildx builder with the config
docker buildx create --name fivespot-builder --config ~/.docker/buildx/buildkitd.toml
docker buildx use fivespot-builder
```

Then build with your mirrored base image:

```bash
make docker-build-amd64 BASE_IMAGE=oss-docker-gcr.artifactory.example.com/distroless/cc-debian12:nonroot
```

#### 4. Complete Air-Gapped Build Example

```bash
# Set environment variables
export AIRGAP_CARGO_HOME=~/.cargo-airgap
export PYPI_INDEX_URL=https://artifactory.example.com/api/pypi/pypi/simple
export BASE_IMAGE=oss-docker-gcr.artifactory.example.com/distroless/cc-debian12:nonroot

# Build Docker image for amd64
make docker-build-amd64

# Or build for arm64
make docker-build-arm64
```

Add these to your shell profile (`~/.bashrc`, `~/.zshrc`) for persistence:

```bash
export AIRGAP_CARGO_HOME=~/.cargo-airgap
export PYPI_INDEX_URL=https://artifactory.example.com/api/pypi/pypi/simple
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
4. **Removing** → Grace period, node draining, preparing for removal
5. **Inactive** → Removed from cluster
6. **UnScheduled** → Outside time window
7. **Error** → Recoverable error state

### Node Draining

During the **Removing** phase, the controller performs automatic node draining:

1. **Cordon** - Marks the node as unschedulable
2. **Evict pods** - Gracefully evicts all pods (except DaemonSets)
3. **Timeout** - Respects `nodeDrainTimeout` configuration
4. **Delete** - Removes the CAPI Machine after drain completes

Configure drain behavior in your `ScheduledMachine`:

```yaml
spec:
  gracefulShutdownTimeout: 5m  # Grace period before draining starts
  nodeDrainTimeout: 5m         # Maximum time for node drain
```

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

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

## Acknowledgments

- Built with [kube-rs](https://github.com/kube-rs/kube-rs)
- Designed for [k0smotron](https://github.com/k0sproject/k0smotron)
- Inspired by the need for time-based infrastructure automation
