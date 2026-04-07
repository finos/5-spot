# Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
# SPDX-License-Identifier: MIT

.PHONY: help install build build-debug build-linux-amd64 build-linux-arm64 build-all-platforms prepare-binaries prepare-binaries-native test test-lib lint format clean crds crddoc docs docs-serve docs-clean docs-rustdoc run-local docker-build docker-build-chainguard docker-push docker-buildx docker-buildx-chainguard gitleaks gitleaks-install install-git-hooks security-scan-local

# Image configuration
REGISTRY ?= ghcr.io
IMAGE_NAME ?= 5spot
IMAGE_TAG ?= latest-dev
NAMESPACE ?= 5-spot-system

# Platform configuration for multi-arch builds
PLATFORM ?= linux/amd64
BUILD_PLATFORMS ?= linux/arm64,linux/amd64

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
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

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

build-linux-amd64: ## Build for Linux x86_64 (requires cross or Linux)
	@if command -v cross >/dev/null 2>&1; then \
		MALLOC_CONF=abort_conf:false cross build --release --target x86_64-unknown-linux-gnu; \
	elif [ "$$(uname -s)" = "Linux" ] && [ "$$(uname -m)" = "x86_64" ]; then \
		cargo build --release --target x86_64-unknown-linux-gnu; \
	else \
		echo "ERROR: Cross-compilation requires 'cross' tool. Install with: cargo install cross"; \
		echo "       Or run on Linux x86_64, or use GitHub Actions CI."; \
		exit 1; \
	fi

build-linux-arm64: ## Build for Linux ARM64 (requires cross or Linux ARM64)
	@if command -v cross >/dev/null 2>&1; then \
		MALLOC_CONF=abort_conf:false cross build --release --target aarch64-unknown-linux-gnu; \
	elif [ "$$(uname -s)" = "Linux" ] && [ "$$(uname -m)" = "aarch64" ]; then \
		cargo build --release --target aarch64-unknown-linux-gnu; \
	else \
		echo "ERROR: Cross-compilation requires 'cross' tool. Install with: cargo install cross"; \
		echo "       Or run on Linux ARM64, or use GitHub Actions CI."; \
		exit 1; \
	fi

build-all-platforms: build-linux-amd64 build-linux-arm64 ## Build for all Linux platforms

prepare-binaries: build-all-platforms ## Build binaries and prepare for Docker
	@echo "Preparing binaries for Docker build..."
	@mkdir -p binaries/amd64 binaries/arm64
	@cp target/x86_64-unknown-linux-gnu/release/5spot binaries/amd64/
	@cp target/aarch64-unknown-linux-gnu/release/5spot binaries/arm64/
	@echo "Binaries ready in binaries/"
	@ls -lh binaries/amd64/5spot binaries/arm64/5spot

prepare-binaries-native: build ## Prepare native binary for single-platform Docker build
	@echo "Preparing native binary for Docker build..."
	@mkdir -p binaries/amd64 binaries/arm64
	@if [ "$$(uname -m)" = "x86_64" ] || [ "$$(uname -m)" = "amd64" ]; then \
		cp target/release/5spot binaries/amd64/; \
		echo "Binary ready in binaries/amd64/"; \
	elif [ "$$(uname -m)" = "aarch64" ] || [ "$$(uname -m)" = "arm64" ]; then \
		cp target/release/5spot binaries/arm64/; \
		echo "Binary ready in binaries/arm64/"; \
	fi

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
	@cargo run --bin crdgen > deploy/crds/scheduledmachine.yaml
	@echo "✓ CRD YAML file generated: deploy/crds/scheduledmachine.yaml"

crddoc: ## Generate API documentation from CRD types
	@echo "Generating API documentation..."
	@cargo run --bin crddoc > docs/src/reference/api.md
	@echo "✓ API documentation generated: docs/src/reference/api.md"

# ============================================================
# Documentation
# ============================================================

docs: export PATH := $(HOME)/.local/bin:$(HOME)/.cargo/bin:$(PATH)
docs: ## Build all documentation (MkDocs + rustdoc + CRD API reference)
	@echo "Building all documentation..."
	@echo "Checking Poetry installation..."
	@command -v poetry >/dev/null 2>&1 || { echo "Error: Poetry not found. Install with: curl -sSL https://install.python-poetry.org | python3 -"; exit 1; }
	$(configure_poetry_index)
	@echo "Ensuring documentation dependencies are installed..."
	@cd docs && poetry install --no-interaction --quiet
	@echo "Generating CRD API reference documentation..."
	@cargo run --bin crddoc > docs/src/reference/api.md
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


docker-build: prepare-binaries ## Build Docker image (distroless, single platform)
	$(CONTAINER_TOOL) build --platform $(PLATFORM) -t $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG) \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(BASE_IMAGE)" \
		.


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

docker-buildx: prepare-binaries ## Build and push multi-arch Docker image (distroless)
	- $(CONTAINER_TOOL) buildx create --name fivespot-builder
	$(CONTAINER_TOOL) buildx use fivespot-builder
	- $(CONTAINER_TOOL) buildx build --push --platform=$(BUILD_PLATFORMS) -t $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG) \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(BASE_IMAGE)" \
		.
	- $(CONTAINER_TOOL) buildx rm fivespot-builder

docker-buildx-chainguard: prepare-binaries ## Build and push multi-arch Chainguard image
	- $(CONTAINER_TOOL) buildx create --name 5-spot-chainguard-builder
	$(CONTAINER_TOOL) buildx use 5-spot-chainguard-builder
	- $(CONTAINER_TOOL) buildx build --push --platform=$(BUILD_PLATFORMS) -f Dockerfile.chainguard -t $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)-chainguard \
		--build-arg VERSION="$(VERSION)" \
		--build-arg GIT_SHA="$(GIT_SHA)" \
		--build-arg BASE_IMAGE="$(CHAINGUARD_BASE_IMAGE)" \
		.
	- $(CONTAINER_TOOL) buildx rm 5-spot-chainguard-builder

# ============================================================
# Deployment
# ============================================================

deploy-crds: ## Deploy CRDs to cluster
	kubectl apply -f deploy/crds/

deploy: deploy-crds ## Deploy operator (CRDs + deployment)
	kubectl create namespace $(NAMESPACE) --dry-run=client -o yaml | kubectl apply -f -
	kubectl apply -f deploy/deployment/ -n $(NAMESPACE)

undeploy: ## Remove operator from cluster
	kubectl delete -f deploy/deployment/ -n $(NAMESPACE) || true
	kubectl delete -f deploy/crds/ || true

# ============================================================
# Security Scanning
# ============================================================

gitleaks-install: ## Install gitleaks from GitHub with checksum verification
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

gitleaks: gitleaks-install ## Scan for hardcoded secrets and credentials
	@echo "Scanning for secrets with gitleaks..."
	@gitleaks detect --source . --verbose --redact

install-git-hooks: gitleaks-install ## Install git hooks for pre-commit secret scanning
	@echo "Installing git hooks..."
	@mkdir -p .git/hooks
	@echo '#!/bin/sh' > .git/hooks/pre-commit
	@echo '# Pre-commit hook to scan for secrets' >> .git/hooks/pre-commit
	@echo '' >> .git/hooks/pre-commit
	@echo 'echo "Running gitleaks pre-commit scan..."' >> .git/hooks/pre-commit
	@echo 'gitleaks protect --staged --verbose --redact' >> .git/hooks/pre-commit
	@echo 'if [ $$? -ne 0 ]; then' >> .git/hooks/pre-commit
	@echo '    echo ""' >> .git/hooks/pre-commit
	@echo '    echo "ERROR: Secrets detected in staged changes!"' >> .git/hooks/pre-commit
	@echo '    echo "Please remove secrets before committing."' >> .git/hooks/pre-commit
	@echo '    echo "If this is a false positive, add to .gitleaks.toml allowlist."' >> .git/hooks/pre-commit
	@echo '    exit 1' >> .git/hooks/pre-commit
	@echo 'fi' >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "✓ Pre-commit hook installed"
	@echo "  Hook location: .git/hooks/pre-commit"
	@echo "  Gitleaks will scan staged changes before each commit"

security-scan-local: gitleaks ## Run local security scans (gitleaks)
	@echo "Running local security scans..."
	@echo ""
	@echo "=== Gitleaks (Secret Scanning) ==="
	@gitleaks detect --source . --verbose --redact || true
	@echo ""
	@echo "✓ Security scan complete"
