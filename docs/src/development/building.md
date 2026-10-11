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

Use the Makefile targets rather than calling `cargo build --target` yourself.
They pick a strategy based on what the host actually is, and only they know
which linker the host needs:

```bash
make build-linux-amd64        # -> target/x86_64-unknown-linux-gnu/release/
make build-linux-arm64        # -> target/aarch64-unknown-linux-gnu/release/
```

Each target tries three things in order:

1. **`cross`**, if it is installed. Builds in a container, so no host
   toolchain is involved. `cargo install cross` to get it.
2. **A native build**, when the host already is that OS and architecture. No
   cross toolchain is needed or used.
3. **A cross toolchain on `PATH`**, with the Makefile exporting
   `CARGO_TARGET_<TRIPLE>_LINKER` for it. Both the
   `x86_64-unknown-linux-gnu-gcc` (Homebrew / messense) and
   `x86_64-linux-gnu-gcc` (distro) spellings are accepted.

On macOS, install a toolchain for option 3:

```bash
brew tap messense/macos-cross-toolchains
brew install x86_64-unknown-linux-gnu     # or aarch64-unknown-linux-gnu
```

### Do not pin the linker in `.cargo/config.toml`

This is worth knowing before you "helpfully" add it back. A
`[target.<triple>] linker = …` entry applies whenever that triple is the
**target**, including when it is also the **host**:

```toml
# WRONG: breaks every native linux/amd64 build
[target.x86_64-unknown-linux-gnu]
linker = "x86_64-unknown-linux-gnu-gcc"
```

On a native linux/amd64 machine, cargo then tries to exec a macOS
cross-toolchain binary that is not installed there, and every build script
fails at once:

```text
error: linker `x86_64-unknown-linux-gnu-gcc` not found
error: could not compile `serde_json` (build script) due to 1 previous error
...
```

A cross-linker is a property of the host/target **pair**, which cargo's config
cannot express, so the choice lives in the Makefile instead. `.cargo/config.toml`
carries a comment saying so. Recorded as F-3 in
[roadmap 06](https://github.com/finos/5-spot/blob/main/.github/community/06-k0smotron-e2e-and-worker-confinement.md).

If you must drive cargo directly, supply the linker yourself:

```bash
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-gcc \
  cargo build --release --target x86_64-unknown-linux-gnu
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
