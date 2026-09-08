# Building

Detailed instructions for building 5-Spot.

## Build Commands

### Debug Build

```bash
cargo build
```

Output: `target/debug/5spot`

### Release Build

```bash
cargo build --release
```

Output: `target/release/5spot`

### Build Binaries

```bash
# Main controller
cargo build --bin 5spot

# CRD generator
cargo build --bin crdgen

# Documentation generator
cargo build --bin crddoc
```

## Docker Build

### Basic Build

```bash
docker build -t 5spot:latest .
```

### Multi-Architecture Build

```bash
# Build for multiple platforms
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/finos/5-spot:latest \
  --push .
```

### Dockerfiles

Two runtime images are built from pre-built binaries (the Rust build happens
outside the image — `make prepare-binaries-linux-amd64` and friends stage them
under `binaries/<arch>/`):

| File | Base image | Published as |
|------|-----------|--------------|
| `Dockerfile` | `gcr.io/distroless/cc-debian13:nonroot` | `ghcr.io/finos/5-spot*-distroless` |
| `Dockerfile.chainguard` | `cgr.dev/chainguard/glibc-dynamic:latest` | `ghcr.io/finos/5-spot` |

Both bases are **pinned by digest on the `FROM` line**, and Dependabot's
`docker` ecosystem opens a PR with the new digest when either tag is rebuilt:

```dockerfile
ARG BASE_IMAGE=pinned-base

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:8f96… AS pinned-base

FROM ${BASE_IMAGE}
```

The pin has to sit on a real `FROM` instruction — Dependabot does not expand
`ARG`, so a digest written as `ARG BASE_IMAGE=…@sha256:…` + `FROM ${BASE_IMAGE}`
is never updated. `BASE_IMAGE` defaults to the `pinned-base` stage, and stays
overridable for air-gapped or mirrored builds. See
[ADR 0010](https://github.com/finos/5-spot/blob/main/docs/adr/0010-base-image-pins-on-from-line.md).

```bash
# Ordinary build: uses the pinned digest
make docker-build-amd64

# Mirrored / air-gapped build: your registry becomes the trusted source
make docker-build-amd64 BASE_IMAGE=<mirror>/distroless/cc-debian13:nonroot

# Chainguard FIPS variant
make docker-build-chainguard \
  CHAINGUARD_BASE_IMAGE=cgr.dev/chainguard/glibc-dynamic:latest-fips
```

## Generated Artifacts

### CRD Generation

```bash
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml
```

Generates the Kubernetes Custom Resource Definition.

### API Documentation

```bash
cargo run --bin crddoc > docs/src/reference/api.md
```

Generates Markdown documentation from CRD schema.

## Build Configuration

### Cargo.toml Features

```toml
[features]
default = []
integration-tests = []  # Enable integration test helpers
```

### Build Profiles

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

## Cross-Compilation

### Linux (from macOS)

```bash
# Install cross
cargo install cross

# Build for Linux
cross build --release --target x86_64-unknown-linux-musl
```

### ARM64

```bash
cross build --release --target aarch64-unknown-linux-musl
```

## CI/CD Build

### GitHub Actions

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: dtolnay/rust-action@stable

      - name: Build
        run: cargo build --release

      - name: Run tests
        run: cargo test
```

## Troubleshooting

### OpenSSL Errors

Use `rustls` instead of native OpenSSL:

```toml
[dependencies]
kube = { version = "0.87", default-features = false, features = ["client", "runtime", "rustls-tls"] }
```

### musl Build Issues

Install musl tools:

```bash
# Alpine
apk add musl-dev

# Ubuntu
apt-get install musl-tools
```

## Related

- [Development Setup](./setup.md) - Environment setup
- [Testing](./testing.md) - Test execution
- [Contributing](./contributing.md) - Contribution guidelines
