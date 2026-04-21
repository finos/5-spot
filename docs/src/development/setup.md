# Development Setup

Set up your local environment for 5-Spot development.

## Prerequisites

### Required

- **Rust**: 1.75 or later (install via [rustup](https://rustup.rs/))
- **kubectl**: Configured with cluster access
- **Docker**: For container builds

### Optional

- **kind**: For local Kubernetes clusters
- **minikube**: Alternative local cluster
- **cargo-watch**: For development hot-reload

## Installation

### Clone the Repository

```bash
git clone https://github.com/finos/5-spot.git
cd 5-spot
```

### Install Rust Dependencies

```bash
# Install Rust if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Ensure you have the latest stable Rust
rustup update stable
```

### Install Development Tools

```bash
# Code formatting
rustup component add rustfmt

# Linting
rustup component add clippy

# Watch mode (optional)
cargo install cargo-watch
```

## Building

### Debug Build

```bash
cargo build
```

### Release Build

```bash
cargo build --release
```

### Generate CRDs

```bash
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml
```

### Generate API Documentation

```bash
cargo run --bin crddoc > docs/reference/api.md
```

## Running Locally

### Prerequisites

1. A Kubernetes cluster (kind, minikube, or remote)
2. CAPI installed on the cluster
3. `kubectl` configured to access the cluster

### Install CRDs

```bash
kubectl apply -f deploy/crds/scheduledmachine.yaml
```

### Run the Operator

```bash
# Development mode with debug logging
RUST_LOG=debug cargo run

# Or with cargo-watch for auto-reload
cargo watch -x run
```

## Testing

### Run All Tests

```bash
cargo test
```

### Run Specific Tests

```bash
# Run tests matching a pattern
cargo test schedule

# Run tests in a specific module
cargo test --lib crd_tests
```

### Run with Output

```bash
cargo test -- --nocapture
```

## Code Quality

### Format Code

```bash
cargo fmt
```

### Lint Code

```bash
cargo clippy -- -D warnings
```

### Check All

```bash
# Format, lint, and test
cargo fmt && cargo clippy -- -D warnings && cargo test
```

## IDE Setup

### VS Code

Recommended extensions:
- rust-analyzer
- Even Better TOML
- crates

Settings (`.vscode/settings.json`):
```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "editor.formatOnSave": true
}
```

### IntelliJ IDEA / RustRover

- Install Rust plugin
- Enable rustfmt on save
- Configure clippy as external linter

## Local Kubernetes

### kind

The repo ships `make kind-*` targets that wrap the `kind` CLI for quickly
spinning up a test cluster with the controller and CRDs installed.

```bash
# One-shot: install kind (if missing), create cluster, build+load image,
# apply CRDs, apply controller Deployment/RBAC/Service/…
make kind-setup

# Apply the example ScheduledMachine
make kind-example

# Inspect
make kind-status

# Tear down
make kind-delete
```

Individual steps (run in order if you prefer explicit control):

```bash
make kind-install   # download kind binary with checksum verification
make kind-create    # create cluster named $(KIND_CLUSTER_NAME), default 5spot-dev
make kind-load      # docker-build the controller and load image into the cluster
make kind-deploy    # apply deploy/crds/ + deploy/deployment/
```

Override defaults via environment variables — e.g.
`KIND_CLUSTER_NAME=my-cluster KIND_NODE_IMAGE=kindest/node:v1.30.4 make kind-create`.

Raw `kind` usage is still available if you need a bespoke cluster topology:

```bash
kind create cluster --name 5spot-dev

# Install CAPI (not managed by the Makefile targets above)
clusterctl init

# Apply CRDs
kubectl apply -f deploy/crds/
```

### minikube

```bash
# Start cluster
minikube start --cpus 4 --memory 8g

# Install CAPI
clusterctl init

# Apply CRDs
kubectl apply -f deploy/crds/
```

## Related

- [Building](./building.md) - Detailed build instructions
- [Testing](./testing.md) - Testing strategies
- [Contributing](./contributing.md) - Contribution guidelines
