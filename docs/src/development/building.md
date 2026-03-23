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
  -t ghcr.io/firestoned/5-spot:latest \
  --push .
```

### Dockerfile

```dockerfile
FROM rust:1.75-alpine AS builder

WORKDIR /app
COPY . .

RUN apk add --no-cache musl-dev
RUN cargo build --release

FROM alpine:3.19

COPY --from=builder /app/target/release/5spot /usr/local/bin/

ENTRYPOINT ["5spot"]
```

## Generated Artifacts

### CRD Generation

```bash
cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml
```

Generates the Kubernetes Custom Resource Definition.

### API Documentation

```bash
cargo run --bin crddoc > docs/reference/api.md
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
