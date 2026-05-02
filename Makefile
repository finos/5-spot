# Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
# SPDX-License-Identifier: Apache-2.0

.PHONY: help install build build-debug build-linux-amd64 build-linux-arm64 build-macos-arm64 prepare-binaries-linux-amd64 prepare-binaries-linux-arm64 test test-lib lint format clean crds crddoc docs docs-serve docs-clean docs-rustdoc calm-diagrams calm-validate run-local docker-build docker-build-amd64 docker-build-arm64 docker-build-chainguard docker-push docker-buildx docker-buildx-chainguard gitleaks gitleaks-install install-git-hooks security-scan-local sbom audit vexctl-install vex-validate vex-assemble vex-auto-presence vex-auto-reachability kind-install kind-create kind-delete kind-load kind-deploy kind-example kind-setup kind-status

# CALM (FINOS Common Architecture Language Model) configuration
CALM_CLI_VERSION ?= 1.37.0
CALM_ARCH        := docs/architecture/calm/architecture.json
CALM_TEMPLATES   := docs/architecture/calm/templates/mermaid
CALM_DIAGRAMS_OUT := docs/src/architecture

# Image configuration
REGISTRY ?= ghcr.io
IMAGE_NAME ?= 5spot
IMAGE_TAG ?= latest-dev
NAMESPACE ?= 5spot-system

# Platform configuration for builds
# Default is linux/amd64 (most common for Kubernetes deployments)
# Override with: make docker-buildx BUILD_PLATFORMS=linux/arm64
PLATFORM ?= linux/amd64
BUILD_PLATFORMS ?= linux/amd64

# Base images for containers (glibc-based for GNU target compatibility)
BASE_IMAGE ?= gcr.io/distroless/cc-debian12:nonroot

# Chainguard images (zero CVE, glibc-based for regulated environments)
CHAINGUARD_BASE_IMAGE ?= cgr.dev/chainguard/glibc-dynamic:latest

# Version information
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
GIT_SHA ?= $(shell git rev-parse HEAD 2>/dev/null || echo "unknown")

# Container tool (docker or podman)
CONTAINER_TOOL ?= docker

# Security tool versions
GITLEAKS_VERSION ?= 8.21.2
VEXCTL_VERSION   ?= 0.4.1

# Kind (local Kubernetes) configuration
KIND_VERSION ?= 0.24.0
KIND_CLUSTER_NAME ?= 5spot-dev
KIND_NODE_IMAGE ?= kindest/node:v1.31.0
# Image reference used for the locally built kind image. The `local-dev` tag
# makes it unambiguous that the image is a developer build (not a released
# version). `kind-deploy` applies the checked-in Deployment (pinned to
# ghcr.io/finos/5-spot:v0.1.0) and then overrides the container image to
# $(KIND_IMAGE) via `kubectl set image` so the locally loaded image is used.
KIND_IMAGE ?= ghcr.io/finos/5-spot:local-dev

# Python/Poetry package index configuration (for corporate environments)
# Set PYPI_INDEX_URL to use a custom PyPI mirror (e.g., Artifactory)
# Example: export PYPI_INDEX_URL=https://artifactory.example.com/api/pypi/pypi/simple
PYPI_INDEX_URL ?=

# Suppress MkDocs 2.0 incompatibility warning from Material for MkDocs
# MkDocs 2.0 is not yet released and we're staying on 1.x
export NO_MKDOCS_2_WARNING := 1

# Helper to configure Poetry with custom index if PYPI_INDEX_URL is set
define configure_poetry_index
	@if [ -n "$(PYPI_INDEX_URL)" ]; then \
		echo "Configuring Poetry to use custom PyPI index..."; \
		cd docs && poetry source add --priority=primary custom-pypi $(PYPI_INDEX_URL) 2>/dev/null || true; \
	fi
endef

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-24s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ============================================================
# Development
# ============================================================

install: ## Install dependencies (ensure Rust toolchain)
	@echo "Ensure Rust toolchain is installed (rustup)."
	@rustup --version || echo "Install Rust from https://rustup.rs"

build: ## Build the Rust binary (release, native platform)
	cargo build --release

build-debug: ## Build the Rust binary (debug)
	cargo build

build-linux-amd64: ## Build for Linux x86_64 (requires cross toolchain)
	@if command -v cross >/dev/null 2>&1; then \
		echo "Building with cross for x86_64-unknown-linux-gnu..."; \
		if [ -n "$(AIRGAP_CARGO_HOME)" ]; then \
			CARGO_HOME="$(AIRGAP_CARGO_HOME)" cross build --release --target x86_64-unknown-linux-gnu; \
		else \
			cross build --release --target x86_64-unknown-linux-gnu; \
		fi; \
	elif [ "$$(uname -s)" = "Linux" ] && [ "$$(uname -m)" = "x86_64" ]; then \
		echo "Building natively on Linux x86_64..."; \
		cargo build --release --target x86_64-unknown-linux-gnu; \
	elif command -v x86_64-linux-gnu-gcc >/dev/null 2>&1; then \
		echo "Building with cargo + x86_64-linux-gnu toolchain..."; \
		cargo build --release --target x86_64-unknown-linux-gnu; \
	else \
		echo "ERROR: Cross-compilation to Linux x86_64 requires one of:"; \
		echo "  1. cross tool (recommended): cargo install cross"; \
		echo "  2. GNU toolchain: brew tap messense/macos-cross-toolchains && brew install x86_64-unknown-linux-gnu"; \
		echo "  3. Run on native Linux x86_64"; \
		exit 1; \
	fi

build-macos-arm64: ## Build for macOS ARM64 (Apple Silicon)
	@if [ "$$(uname -s)" = "Darwin" ] && [ "$$(uname -m)" = "arm64" ]; then \
		cargo build --release --target aarch64-apple-darwin; \
	else \
		echo "ERROR: This target requires macOS on Apple Silicon (arm64)."; \
		exit 1; \
	fi

build-linux-arm64: ## Build for Linux ARM64 (requires cross toolchain)
	@if command -v cross >/dev/null 2>&1; then \
		echo "Building with cross for aarch64-unknown-linux-gnu..."; \
		cross build --release --target aarch64-unknown-linux-gnu; \
	elif [ "$$(uname -s)" = "Linux" ] && [ "$$(uname -m)" = "aarch64" ]; then \
		echo "Building natively on Linux ARM64..."; \
		cargo build --release --target aarch64-unknown-linux-gnu; \
	elif command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then \
		echo "Building with cargo + aarch64-linux-gnu toolchain..."; \
		cargo build --release --target aarch64-unknown-linux-gnu; \
	else \
		echo "ERROR: Cross-compilation to Linux ARM64 requires one of:"; \
		echo "  1. cross tool (recommended): cargo install cross"; \
		echo "  2. GNU toolchain: brew tap messense/macos-cross-toolchains && brew install aarch64-unknown-linux-gnu"; \
		echo "  3. Run on native Linux ARM64"; \
		exit 1; \
	fi

prepare-binaries-linux-amd64: build-linux-amd64 ## Build and prepare Linux x86_64 binary
	@echo "Preparing Linux x86_64 binary for Docker build..."
	@mkdir -p binaries/amd64
	@cp target/x86_64-unknown-linux-gnu/release/5spot binaries/amd64/
	@echo "✓ Binary ready: binaries/amd64/5spot"
	@ls -lh binaries/amd64/5spot

prepare-binaries-linux-arm64: build-linux-arm64 ## Build and prepare Linux ARM64 binary
	@echo "Preparing Linux ARM64 binary for Docker build..."
	@mkdir -p binaries/arm64
	@cp target/aarch64-unknown-linux-gnu/release/5spot binaries/arm64/
	@echo "✓ Binary ready: binaries/arm64/5spot"
	@ls -lh binaries/arm64/5spot

test: ## Run all tests
	cargo test --all

test-lib: ## Run library tests only
	cargo test --lib

lint: ## Run linting and checks
	cargo fmt -- --check
	cargo clippy -- -D warnings

format: ## Format code
	cargo fmt

clean: ## Clean build artifacts
	cargo clean
	rm -rf target/

run-local: ## Run operator locally
	RUST_LOG=info cargo run --release

# ============================================================
# Code Generation
# ============================================================

crds: ## Generate CRD YAML files from Rust types
	@echo "Generating CRD YAML files from src/crd.rs..."
	@cargo run --quiet --bin crdgen > deploy/crds/scheduledmachine.yaml
	@echo "✓ CRD YAML file generated: deploy/crds/scheduledmachine.yaml"

crddoc: ## Generate API documentation from CRD types
	@echo "Generating API documentation..."
	@cargo run --quiet --bin crddoc > docs/src/reference/api.md
	@echo "✓ API documentation generated: docs/src/reference/api.md"

# ============================================================
# Documentation
# ============================================================

calm-diagrams: ## Render CALM flow diagrams (Mermaid) into docs/src/architecture/
	@if [ "$(SKIP_CALM_DIAGRAMS)" = "1" ]; then \
	  echo "SKIP_CALM_DIAGRAMS=1 — using existing files in $(CALM_DIAGRAMS_OUT)"; \
	  for f in flows.md system.md; do \
	    test -f $(CALM_DIAGRAMS_OUT)/$$f || { echo "Error: $(CALM_DIAGRAMS_OUT)/$$f missing"; exit 1; }; \
	  done; \
	else \
	  command -v npx >/dev/null 2>&1 || { echo "Error: npx not found. Install Node.js from https://nodejs.org"; exit 1; }; \
	  echo "Rendering CALM diagrams via @finos/calm-cli@$(CALM_CLI_VERSION)..."; \
	  mkdir -p $(CALM_DIAGRAMS_OUT); \
	  npx --yes @finos/calm-cli@$(CALM_CLI_VERSION) template \
	    -a $(CALM_ARCH) \
	    -d $(CALM_TEMPLATES) \
	    -o $(CALM_DIAGRAMS_OUT) \
	    --clear-output-directory; \
	  echo "Stripping .hbs suffix from rendered files..."; \
	  for f in $(CALM_DIAGRAMS_OUT)/*.hbs; do \
	    [ -e "$$f" ] || continue; \
	    mv "$$f" "$${f%.hbs}"; \
	  done; \
	fi

calm-validate: ## Validate the CALM architecture against the meta-schema
	@command -v npx >/dev/null 2>&1 || { echo "Error: npx not found. Install Node.js from https://nodejs.org"; exit 1; }
	@npx --yes @finos/calm-cli@$(CALM_CLI_VERSION) validate \
	  -a $(CALM_ARCH) \
	  -f pretty

docs: export PATH := $(HOME)/.local/bin:$(HOME)/.cargo/bin:$(PATH)
docs: calm-diagrams ## Build all documentation (MkDocs + rustdoc + CRD API reference + CALM diagrams)
	@echo "Building all documentation..."
	@echo "Checking Poetry installation..."
	@command -v poetry >/dev/null 2>&1 || { echo "Error: Poetry not found. Install with: curl -sSL https://install.python-poetry.org | python3 -"; exit 1; }
	$(configure_poetry_index)
	@echo "Ensuring documentation dependencies are installed..."
	@cd docs && poetry install --no-interaction --quiet
	@echo "Generating CRD API reference documentation..."
	@cargo run --quiet --bin crddoc > docs/src/reference/api.md
	@echo "Building rustdoc API documentation..."
	@cargo doc --no-deps --all-features
	@echo "Building MkDocs documentation..."
	@cd docs && poetry run mkdocs build
	@echo "Copying rustdoc into documentation..."
	@mkdir -p docs/site/rustdoc
	@cp -r target/doc/* docs/site/rustdoc/
	@echo "Creating rustdoc index redirect..."
	@echo '<!DOCTYPE html>' > docs/site/rustdoc/index.html
	@echo '<html>' >> docs/site/rustdoc/index.html
	@echo '<head>' >> docs/site/rustdoc/index.html
	@echo '    <meta charset="utf-8">' >> docs/site/rustdoc/index.html
	@echo '    <title>5-Spot API Documentation</title>' >> docs/site/rustdoc/index.html
	@echo '    <meta http-equiv="refresh" content="0; url=five_spot/index.html">' >> docs/site/rustdoc/index.html
	@echo '</head>' >> docs/site/rustdoc/index.html
	@echo '<body>' >> docs/site/rustdoc/index.html
	@echo '    <p>Redirecting to <a href="five_spot/index.html">5-Spot API Documentation</a>...</p>' >> docs/site/rustdoc/index.html
	@echo '</body>' >> docs/site/rustdoc/index.html
	@echo '</html>' >> docs/site/rustdoc/index.html
	@echo "✓ Documentation built successfully in docs/site/"
	@echo "  - User guide: docs/site/index.html"
	@echo "  - API reference: docs/site/rustdoc/five_spot/index.html"

docs-serve: export PATH := $(HOME)/.local/bin:$(PATH)
docs-serve: ## Serve documentation locally with live reload (MkDocs)
	@echo "Starting MkDocs development server with live reload..."
	@command -v poetry >/dev/null 2>&1 || { echo "Error: Poetry not found. Install with: curl -sSL https://install.python-poetry.org | python3 -"; exit 1; }
	$(configure_poetry_index)
	@echo "Ensuring documentation dependencies are installed..."
	@cd docs && poetry install --no-interaction --quiet
	@echo ""
	@echo "Documentation server starting at http://127.0.0.1:8000"
	@echo "Live reload enabled - changes will auto-refresh your browser"
	@echo ""
	@echo "Watching:"
	@echo "  - Documentation content: docs/src/"
	@echo "  - Configuration: docs/mkdocs.yml"
	@echo ""
	@echo "Press Ctrl+C to stop"
	@echo ""
	@cd docs && poetry run mkdocs serve --livereload

docs-rustdoc: ## Build and open rustdoc API documentation only
	@echo "Building rustdoc API documentation..."
	@cargo doc --no-deps --all-features --open

docs-clean: ## Clean documentation build artifacts
	@echo "Cleaning documentation build artifacts..."
	@rm -rf docs/site/
	@rm -rf target/doc/
	@rm -rf docs/.venv/
	@rm -rf docs/poetry.lock
	@echo "✓ Documentation artifacts cleaned"

docs-deploy: docs ## Build and deploy documentation to GitHub Pages
	@echo "Deploying documentation to GitHub Pages..."
	@cd docs && poetry run mkdocs gh-deploy --force
	@echo "✓ Documentation deployed to GitHub Pages"

# ============================================================
# Docker (requires binaries to be built first with prepare-binaries)
# ============================================================


docker-build: ## Build Docker image (auto-detect host arch, loads to local docker)
	@echo "Detecting host architecture..."
	@if [ "$$(uname -m)" = "x86_64" ]; then \
		echo "Host: x86_64 -> building linux/amd64"; \
		$(MAKE) docker-build-amd64; \
	elif [ "$$(uname -m)" = "arm64" ] || [ "$$(uname -m)" = "aarch64" ]; then \
		echo "Host: arm64 -> building linux/amd64 (default for k8s)"; \
		$(MAKE) docker-build-amd64; \
	else \
		echo "ERROR: Unsupported architecture: $$(uname -m)"; \
		exit 1; \
	fi


docker-build-chainguard: prepare-binaries ## Build Docker image (Chainguard - zero CVEs)
	$(CONTAINER_TOOL) build --platform $(PLATFORM) -f Dockerfile.chainguard -t $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)-chainguard \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(CHAINGUARD_BASE_IMAGE)" \
		.

docker-push: ## Push Docker image
	$(CONTAINER_TOOL) push $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)

docker-push-chainguard: ## Push Chainguard Docker image
	$(CONTAINER_TOOL) push $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)-chainguard

docker-build-amd64: prepare-binaries-linux-amd64 ## Build Docker image for linux/amd64 (loads to local docker)
	@$(CONTAINER_TOOL) buildx inspect fivespot-builder >/dev/null 2>&1 || \
		$(CONTAINER_TOOL) buildx create --name fivespot-builder --config ~/.docker/buildx/buildkitd.toml
	$(CONTAINER_TOOL) buildx use fivespot-builder
	$(CONTAINER_TOOL) buildx build --load --platform=linux/amd64 -t $(IMAGE_NAME):$(IMAGE_TAG)-amd64 \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(BASE_IMAGE)" \
		.

docker-build-arm64: prepare-binaries-linux-arm64 ## Build Docker image for linux/arm64 (loads to local docker)
	@$(CONTAINER_TOOL) buildx inspect fivespot-builder >/dev/null 2>&1 || \
		$(CONTAINER_TOOL) buildx create --name fivespot-builder --config ~/.docker/buildx/buildkitd.toml
	$(CONTAINER_TOOL) buildx use fivespot-builder
	$(CONTAINER_TOOL) buildx build --load --platform=linux/arm64 -t $(IMAGE_NAME):$(IMAGE_TAG)-arm64 \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(BASE_IMAGE)" \
		.

docker-buildx: prepare-binaries-linux-amd64 ## Build and push Docker image to registry (CI)
	@$(CONTAINER_TOOL) buildx inspect fivespot-builder >/dev/null 2>&1 || \
		$(CONTAINER_TOOL) buildx create --name fivespot-builder --config ~/.docker/buildx/buildkitd.toml
	$(CONTAINER_TOOL) buildx use fivespot-builder
	$(CONTAINER_TOOL) buildx build --push --platform=linux/amd64 -t $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG) \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(BASE_IMAGE)" \
		.

docker-buildx-chainguard: prepare-binaries-linux-amd64 ## Build and push Chainguard image to registry (CI)
	@$(CONTAINER_TOOL) buildx inspect fivespot-builder >/dev/null 2>&1 || \
		$(CONTAINER_TOOL) buildx create --name fivespot-builder --config ~/.docker/buildx/buildkitd.toml
	$(CONTAINER_TOOL) buildx use fivespot-builder
	$(CONTAINER_TOOL) buildx build --push --platform=$(BUILD_PLATFORMS) -f Dockerfile.chainguard -t $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)-chainguard \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(CHAINGUARD_BASE_IMAGE)" \
		.

# ============================================================
# Deployment
# ============================================================

deploy-crds: ## Deploy CRDs to cluster
	kubectl apply -f deploy/crds/

deploy: deploy-crds ## Deploy operator (CRDs + deployment)
	kubectl create namespace $(NAMESPACE) --dry-run=client -o yaml | kubectl apply -f -
	kubectl apply -R -f deploy/deployment/ -n $(NAMESPACE)

undeploy: ## Remove operator from cluster
	kubectl delete -R -f deploy/deployment/ -n $(NAMESPACE) || true
	kubectl delete -f deploy/crds/ || true

# ============================================================
# Security Scanning
# ============================================================

# Sentinel string written into .git/hooks/pre-commit so we can recognise our
# own hook on re-runs and avoid clobbering a developer's custom pre-commit.
GITLEAKS_HOOK_SENTINEL := 5spot-managed-gitleaks-hook

gitleaks-install: ## Install gitleaks (with checksum verification) AND wire the local pre-commit hook
	@if ! command -v gitleaks >/dev/null 2>&1; then \
		echo "Installing gitleaks v$(GITLEAKS_VERSION)..."; \
		OS=$$(uname -s | tr '[:upper:]' '[:lower:]'); \
		ARCH=$$(uname -m); \
		case "$$ARCH" in \
			x86_64) ARCH="x64" ;; \
			aarch64|arm64) ARCH="arm64" ;; \
		esac; \
		PLATFORM="$${OS}_$${ARCH}"; \
		TARBALL="gitleaks_$(GITLEAKS_VERSION)_$${PLATFORM}.tar.gz"; \
		BASE_URL="https://github.com/gitleaks/gitleaks/releases/download/v$(GITLEAKS_VERSION)"; \
		echo "Downloading gitleaks for $${PLATFORM}..."; \
		curl -sSL -o /tmp/$${TARBALL} $${BASE_URL}/$${TARBALL}; \
		echo "Downloading checksums..."; \
		curl -sSL -o /tmp/gitleaks_checksums.txt $${BASE_URL}/gitleaks_$(GITLEAKS_VERSION)_checksums.txt; \
		echo "Verifying checksum..."; \
		cd /tmp && grep "$${TARBALL}" gitleaks_checksums.txt > checksum_file.txt; \
		if command -v sha256sum >/dev/null 2>&1; then \
			sha256sum -c checksum_file.txt; \
		elif command -v shasum >/dev/null 2>&1; then \
			shasum -a 256 -c checksum_file.txt; \
		else \
			echo "WARNING: No checksum tool found, skipping verification"; \
		fi; \
		echo "Extracting gitleaks..."; \
		tar -xzf /tmp/$${TARBALL} -C /tmp gitleaks; \
		sudo mv /tmp/gitleaks /usr/local/bin/; \
		rm -f /tmp/$${TARBALL} /tmp/gitleaks_checksums.txt /tmp/checksum_file.txt; \
		echo "✓ gitleaks v$(GITLEAKS_VERSION) installed successfully"; \
	else \
		echo "✓ gitleaks already installed: $$(gitleaks version)"; \
	fi
	@$(MAKE) --no-print-directory install-git-hooks

gitleaks: gitleaks-install ## Scan for hardcoded secrets and credentials
	@echo "Scanning for secrets with gitleaks..."
	@gitleaks detect --source . --verbose --redact

# install-git-hooks is intentionally decoupled from gitleaks-install — the hook
# only invokes gitleaks at *commit* time, not at install time, so we avoid a
# circular dependency (gitleaks-install → install-git-hooks → gitleaks-install).
# The hook is idempotent: if a custom pre-commit already exists without our
# sentinel we back it up to pre-commit.bak rather than overwriting silently.
install-git-hooks: ## Install git pre-commit hook for secret scanning (idempotent; preserves custom hooks)
	@if [ ! -d .git ]; then \
		echo "✗ Not inside a git repository (.git missing) — skipping hook install"; \
		exit 0; \
	fi
	@mkdir -p .git/hooks
	@if [ -f .git/hooks/pre-commit ] && grep -q "$(GITLEAKS_HOOK_SENTINEL)" .git/hooks/pre-commit 2>/dev/null; then \
		echo "✓ Pre-commit hook already managed by 5spot — leaving in place"; \
	else \
		if [ -f .git/hooks/pre-commit ]; then \
			echo "⚠ Existing pre-commit hook detected — backing up to .git/hooks/pre-commit.bak"; \
			mv .git/hooks/pre-commit .git/hooks/pre-commit.bak; \
		fi; \
		echo "Installing git pre-commit hook..."; \
		printf '%s\n' \
			'#!/bin/sh' \
			'# $(GITLEAKS_HOOK_SENTINEL)' \
			'# Pre-commit hook to scan staged changes for secrets via gitleaks.' \
			'# Reinstall with: make install-git-hooks   (idempotent)' \
			'' \
			'if ! command -v gitleaks >/dev/null 2>&1; then' \
			'    echo "ERROR: gitleaks not found on PATH — run \"make gitleaks-install\" first." >&2' \
			'    exit 1' \
			'fi' \
			'' \
			'echo "Running gitleaks pre-commit scan..."' \
			'gitleaks protect --staged --verbose --redact' \
			'rc=$$?' \
			'if [ $$rc -ne 0 ]; then' \
			'    echo "" >&2' \
			'    echo "ERROR: Secrets detected in staged changes!" >&2' \
			'    echo "Please remove secrets before committing." >&2' \
			'    echo "If this is a false positive, add to .gitleaks.toml allowlist." >&2' \
			'    exit 1' \
			'fi' \
			> .git/hooks/pre-commit; \
		chmod +x .git/hooks/pre-commit; \
		echo "✓ Pre-commit hook installed at .git/hooks/pre-commit"; \
	fi

security-scan-local: gitleaks ## Run local security scans (gitleaks)
	@echo "Running local security scans..."
	@echo ""
	@echo "=== Gitleaks (Secret Scanning) ==="
	@gitleaks detect --source . --verbose --redact || true
	@echo ""
	@echo "✓ Security scan complete"

sbom: ## Generate CycloneDX SBOM (Software Bill of Materials)
	@echo "Generating CycloneDX SBOM..."
	@command -v cargo-cyclonedx >/dev/null 2>&1 || { echo "Installing cargo-cyclonedx..."; cargo install cargo-cyclonedx; }
	@cargo cyclonedx --format json --spec-version 1.4
	@echo "✓ SBOM generated: five_spot.cdx.json"

audit: ## Check dependencies for security vulnerabilities (installs cargo-audit if missing)
	@command -v cargo-audit >/dev/null 2>&1 || { echo "Installing cargo-audit..."; cargo install cargo-audit; }
	@cargo audit

vexctl-install: ## Install vexctl (brew on macOS, pinned raw binary + sha256 on Linux)
	@if command -v vexctl >/dev/null 2>&1; then \
		echo "✓ vexctl already installed: $$(vexctl version 2>&1 | head -1)"; \
		exit 0; \
	fi; \
	echo "Installing vexctl v$(VEXCTL_VERSION)..."; \
	OS=$$(uname -s | tr '[:upper:]' '[:lower:]'); \
	ARCH=$$(uname -m); \
	case "$$ARCH" in \
		x86_64|amd64) ARCH="amd64" ;; \
		aarch64|arm64) ARCH="arm64" ;; \
	esac; \
	case "$$OS" in \
		darwin) \
			if command -v brew >/dev/null 2>&1; then \
				brew install vexctl; \
			else \
				echo "ERROR: Homebrew not found. Install from https://brew.sh or set VEXCTL_VERSION and re-run on a system with brew."; \
				exit 1; \
			fi ;; \
		linux) \
			BINARY="vexctl-linux-$${ARCH}"; \
			BASE_URL="https://github.com/openvex/vexctl/releases/download/v$(VEXCTL_VERSION)"; \
			echo "Downloading $${BINARY}..."; \
			curl -fsSL -o /tmp/$${BINARY} "$${BASE_URL}/$${BINARY}"; \
			curl -fsSL -o /tmp/vexctl_checksums.txt "$${BASE_URL}/vexctl_checksums.txt"; \
			cd /tmp && grep "  $${BINARY}$$" vexctl_checksums.txt > vexctl_checksum_file.txt; \
			if command -v sha256sum >/dev/null 2>&1; then \
				sha256sum -c vexctl_checksum_file.txt; \
			elif command -v shasum >/dev/null 2>&1; then \
				shasum -a 256 -c vexctl_checksum_file.txt; \
			else \
				echo "WARNING: No checksum tool found, skipping verification"; \
			fi; \
			sudo install -m 0755 /tmp/$${BINARY} /usr/local/bin/vexctl; \
			rm -f /tmp/$${BINARY} /tmp/vexctl_checksums.txt /tmp/vexctl_checksum_file.txt ;; \
		*) \
			echo "ERROR: Unsupported OS '$$OS'. Install vexctl manually from https://github.com/openvex/vexctl/releases"; \
			exit 1 ;; \
	esac; \
	echo "✓ vexctl installed: $$(vexctl version 2>&1 | head -1)"

vex-validate: vexctl-install ## Parse every .vex/*.json via vexctl merge (validation = successful parse)
	@echo "Validating .vex/*.json..."
	@vexctl merge --id "https://5-spot/local/validate" --author "local" .vex/*.json > /dev/null
	@echo "✓ all .vex/*.json parsed successfully"

vex-assemble: vexctl-install ## Assemble a local OpenVEX document from .vex/*.json (prints to stdout)
	@vexctl merge \
		--id "https://5-spot/local/assemble" \
		--author "$$(git config user.email 2>/dev/null || echo local)" \
		.vex/*.json

# Inputs for vex-auto-presence. Override on the command line, e.g.
#   make vex-auto-presence GRYPE_JSON=scan.json SBOM_FILES="a.json b.json"
GRYPE_JSON   ?= grype.json
SBOM_FILES   ?= $(wildcard target/release/*.cdx.json docker-sbom-*.json)
PRODUCT_PURL ?= pkg:oci/5-spot

vex-auto-presence: ## Run auto-vex-presence bin over $(GRYPE_JSON) + $(SBOM_FILES) (Phase 2)
	@if [ ! -f "$(GRYPE_JSON)" ]; then \
		echo "ERROR: $(GRYPE_JSON) not found. Override with: make vex-auto-presence GRYPE_JSON=path/to/grype.json"; \
		exit 1; \
	fi
	@if [ -z "$(SBOM_FILES)" ]; then \
		echo "ERROR: no SBOMs found. Override with: make vex-auto-presence SBOM_FILES='a.json b.json'"; \
		exit 1; \
	fi
	@cargo run --quiet --bin auto-vex-presence -- \
		--grype-json "$(GRYPE_JSON)" \
		$(foreach s,$(SBOM_FILES),--sbom "$(s)") \
		--vex-dir .vex \
		--product-purl "$(PRODUCT_PURL)" \
		--id "https://5-spot/local/auto-presence" \
		--author "auto-vex-presence" \
		--output vex.auto-presence.json
	@echo "✓ wrote vex.auto-presence.json"

# Inputs for vex-auto-reachability (Phase 3).
# RELEASE_BINARY defaults to the local release build under
# target/release/5spot. AFFECTED_FUNCTIONS is the curated CVE → fns map.
RELEASE_BINARY     ?= target/release/5spot
AFFECTED_FUNCTIONS ?= .vex/.affected-functions.json

vex-auto-reachability: ## Run auto-vex-reachability over $(GRYPE_JSON) + symbols of $(RELEASE_BINARY) (Phase 3)
	@if [ ! -f "$(GRYPE_JSON)" ]; then \
		echo "ERROR: $(GRYPE_JSON) not found. Override with: make vex-auto-reachability GRYPE_JSON=path/to/grype.json"; \
		exit 1; \
	fi
	@if [ ! -f "$(RELEASE_BINARY)" ]; then \
		echo "ERROR: $(RELEASE_BINARY) not found. Build with: cargo build --release"; \
		exit 1; \
	fi
	@if [ ! -f "$(AFFECTED_FUNCTIONS)" ]; then \
		echo "ERROR: $(AFFECTED_FUNCTIONS) not found"; \
		exit 1; \
	fi
	@# nm -D --undefined-only is the ELF dynamic-symbol-imports view.
	@# On macOS Mach-O, nm reports "no dynamic symbol table" — use
	@# -gU (global undefined) as the closest equivalent.
	@if [ "$$(uname -s)" = "Darwin" ]; then \
		nm -gU "$(RELEASE_BINARY)" > /tmp/avr-symbols.txt 2>/dev/null || \
			nm -D --undefined-only "$(RELEASE_BINARY)" > /tmp/avr-symbols.txt; \
	else \
		nm -D --undefined-only "$(RELEASE_BINARY)" > /tmp/avr-symbols.txt; \
	fi
	@cargo run --quiet --bin auto-vex-reachability -- \
		--grype-json "$(GRYPE_JSON)" \
		--binary-symbols /tmp/avr-symbols.txt \
		--affected-functions "$(AFFECTED_FUNCTIONS)" \
		--vex-dir .vex \
		--product-purl "$(PRODUCT_PURL)" \
		--id "https://5-spot/local/auto-reachability" \
		--author "auto-vex-reachability" \
		--output vex.auto-reachability.json
	@rm -f /tmp/avr-symbols.txt
	@echo "✓ wrote vex.auto-reachability.json"

# ============================================================
# Kind Cluster (local testing for ScheduledMachine)
# ============================================================

kind-install: ## Install kind CLI if missing (verifies checksum)
	@if ! command -v kind >/dev/null 2>&1; then \
		echo "Installing kind v$(KIND_VERSION)..."; \
		OS=$$(uname -s | tr '[:upper:]' '[:lower:]'); \
		ARCH=$$(uname -m); \
		case "$$ARCH" in \
			x86_64) ARCH="amd64" ;; \
			aarch64|arm64) ARCH="arm64" ;; \
		esac; \
		BIN="kind-$${OS}-$${ARCH}"; \
		BASE_URL="https://github.com/kubernetes-sigs/kind/releases/download/v$(KIND_VERSION)"; \
		echo "Downloading $$BIN..."; \
		curl -sSLf -o /tmp/$$BIN "$$BASE_URL/$$BIN"; \
		echo "Downloading checksum..."; \
		curl -sSLf -o /tmp/$$BIN.sha256sum "$$BASE_URL/$$BIN.sha256sum"; \
		echo "Verifying checksum..."; \
		cd /tmp && \
			EXPECTED=$$(awk '{print $$1}' $$BIN.sha256sum) && \
			if command -v sha256sum >/dev/null 2>&1; then \
				ACTUAL=$$(sha256sum $$BIN | awk '{print $$1}'); \
			else \
				ACTUAL=$$(shasum -a 256 $$BIN | awk '{print $$1}'); \
			fi && \
			if [ "$$EXPECTED" != "$$ACTUAL" ]; then \
				echo "ERROR: checksum mismatch (expected $$EXPECTED, got $$ACTUAL)"; \
				rm -f /tmp/$$BIN /tmp/$$BIN.sha256sum; \
				exit 1; \
			fi; \
		chmod +x /tmp/$$BIN; \
		sudo mv /tmp/$$BIN /usr/local/bin/kind; \
		rm -f /tmp/$$BIN.sha256sum; \
		echo "✓ kind v$(KIND_VERSION) installed"; \
	else \
		echo "✓ kind already installed: $$(kind version)"; \
	fi
	@command -v kubectl >/dev/null 2>&1 || { echo "ERROR: kubectl not found on PATH. Install kubectl and retry."; exit 1; }

kind-create: kind-install ## Create local kind cluster for testing ScheduledMachine
	@if kind get clusters 2>/dev/null | grep -qx $(KIND_CLUSTER_NAME); then \
		echo "✓ kind cluster '$(KIND_CLUSTER_NAME)' already exists"; \
	else \
		echo "Creating kind cluster '$(KIND_CLUSTER_NAME)' using $(KIND_NODE_IMAGE)..."; \
		kind create cluster --name $(KIND_CLUSTER_NAME) --image $(KIND_NODE_IMAGE) --wait 120s; \
		echo "✓ cluster '$(KIND_CLUSTER_NAME)' ready"; \
	fi
	@kubectl --context kind-$(KIND_CLUSTER_NAME) cluster-info

kind-delete: ## Delete the local kind cluster
	@if kind get clusters 2>/dev/null | grep -qx $(KIND_CLUSTER_NAME); then \
		kind delete cluster --name $(KIND_CLUSTER_NAME); \
		echo "✓ cluster '$(KIND_CLUSTER_NAME)' deleted"; \
	else \
		echo "✓ no cluster named '$(KIND_CLUSTER_NAME)' — nothing to delete"; \
	fi

kind-load: ## Build image for host arch (native cross-compile, bypassing cross) and load it into the kind cluster
	@if ! kind get clusters 2>/dev/null | grep -qx $(KIND_CLUSTER_NAME); then \
		echo "ERROR: kind cluster '$(KIND_CLUSTER_NAME)' does not exist. Run: make kind-create"; \
		exit 1; \
	fi
	@HOST_ARCH=$$(uname -m); \
		case "$$HOST_ARCH" in \
			arm64|aarch64) TRIPLE=aarch64-unknown-linux-gnu; DOCKER_ARCH=arm64; LINKER=aarch64-unknown-linux-gnu-gcc ;; \
			x86_64|amd64)  TRIPLE=x86_64-unknown-linux-gnu;  DOCKER_ARCH=amd64; LINKER=x86_64-unknown-linux-gnu-gcc ;; \
			*) echo "ERROR: unsupported host arch: $$HOST_ARCH"; exit 1 ;; \
		esac; \
		echo "Host $$HOST_ARCH -> building linux/$$DOCKER_ARCH image"; \
		if ! command -v $$LINKER >/dev/null 2>&1; then \
			echo "ERROR: linker '$$LINKER' not found on PATH."; \
			echo "  On macOS: brew tap messense/macos-cross-toolchains && brew install $$TRIPLE"; \
			echo "  On Linux: install the matching gcc cross toolchain for your distro."; \
			exit 1; \
		fi; \
		if ! rustup target list --installed | grep -qx "$$TRIPLE"; then \
			echo "Adding rustup target $$TRIPLE..."; \
			rustup target add "$$TRIPLE"; \
		fi; \
		echo "Compiling 5spot for $$TRIPLE..."; \
		cargo build --release --target "$$TRIPLE"; \
		echo "Staging binary at binaries/$$DOCKER_ARCH/5spot..."; \
		mkdir -p "binaries/$$DOCKER_ARCH"; \
		cp "target/$$TRIPLE/release/5spot" "binaries/$$DOCKER_ARCH/5spot"; \
		echo "Building docker image $(KIND_IMAGE) (linux/$$DOCKER_ARCH)..."; \
		$(CONTAINER_TOOL) build \
			--build-arg TARGETARCH=$$DOCKER_ARCH \
			--build-arg VERSION="$(VERSION)" \
			--build-arg GIT_SHA="$(GIT_SHA)" \
			--build-arg BASE_IMAGE="$(BASE_IMAGE)" \
			-t $(KIND_IMAGE) .; \
		echo "Loading $(KIND_IMAGE) into kind cluster '$(KIND_CLUSTER_NAME)'..."; \
		kind load docker-image $(KIND_IMAGE) --name $(KIND_CLUSTER_NAME); \
		echo "✓ image loaded"

kind-deploy: ## Apply CRDs and controller manifests to the kind cluster
	@if ! kind get clusters 2>/dev/null | grep -qx $(KIND_CLUSTER_NAME); then \
		echo "ERROR: kind cluster '$(KIND_CLUSTER_NAME)' does not exist. Run: make kind-create"; \
		exit 1; \
	fi
	@echo "Applying CRDs to kind cluster '$(KIND_CLUSTER_NAME)'..."
	@kubectl --context kind-$(KIND_CLUSTER_NAME) apply -f deploy/crds/
	@echo "Applying namespace first (avoids race with namespace-scoped resources)..."
	@kubectl --context kind-$(KIND_CLUSTER_NAME) apply -f deploy/deployment/namespace.yaml
	@for i in 1 2 3 4 5 6 7 8 9 10; do \
		if kubectl --context kind-$(KIND_CLUSTER_NAME) get namespace 5spot-system >/dev/null 2>&1; then \
			break; \
		fi; \
		echo "  waiting for namespace 5spot-system ($$i/10)..."; \
		sleep 1; \
	done
	@echo "Applying controller manifests (rbac, configmap, deployment, service, ...)..."
	@kubectl --context kind-$(KIND_CLUSTER_NAME) apply -R -f deploy/deployment/
	@echo "Overriding controller image to $(KIND_IMAGE) (locally built)..."
	@kubectl --context kind-$(KIND_CLUSTER_NAME) -n 5spot-system set image deployment/5spot-controller controller=$(KIND_IMAGE)
	@echo "Waiting for the controller Deployment to become available..."
	@kubectl --context kind-$(KIND_CLUSTER_NAME) -n 5spot-system rollout status deployment/5spot-controller --timeout=180s
	@echo "✓ controller deployed"

kind-example: ## Apply the basic ScheduledMachine example to the kind cluster
	@if ! kind get clusters 2>/dev/null | grep -qx $(KIND_CLUSTER_NAME); then \
		echo "ERROR: kind cluster '$(KIND_CLUSTER_NAME)' does not exist. Run: make kind-create"; \
		exit 1; \
	fi
	@kubectl --context kind-$(KIND_CLUSTER_NAME) apply -f examples/scheduledmachine-basic.yaml
	@echo "✓ example ScheduledMachine applied"
	@echo "Inspect with:"
	@echo "  kubectl --context kind-$(KIND_CLUSTER_NAME) get scheduledmachines -A"
	@echo "  kubectl --context kind-$(KIND_CLUSTER_NAME) describe scheduledmachine business-hours-worker"

kind-status: ## Show kind cluster, controller, and ScheduledMachine status
	@echo "=== kind clusters ==="
	@kind get clusters 2>/dev/null || echo "(none)"
	@echo ""
	@echo "=== controller pods (namespace 5spot-system) ==="
	@kubectl --context kind-$(KIND_CLUSTER_NAME) -n 5spot-system get pods 2>/dev/null || echo "(cluster unreachable)"
	@echo ""
	@echo "=== ScheduledMachines (all namespaces) ==="
	@kubectl --context kind-$(KIND_CLUSTER_NAME) get scheduledmachines -A 2>/dev/null || echo "(cluster unreachable)"

kind-setup: kind-create kind-load kind-deploy ## One-shot: create cluster, build+load image, deploy CRDs & controller
	@echo ""
	@echo "✓ kind setup complete"
	@echo ""
	@echo "Next steps:"
	@echo "  make kind-example   # apply the example ScheduledMachine"
	@echo "  make kind-status    # see cluster & controller state"
	@echo "  make kind-delete    # tear down"
