# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0

# PRODUCTION DOCKERFILE - Uses pre-built binaries
#
# This Dockerfile expects pre-built binaries. Build binaries using:
#
#   # For Linux amd64
#   make prepare-binaries-linux-amd64
#
#   # For macOS ARM64
#   make prepare-binaries-macos-arm64
#
#   # Or auto-detect platform
#   make prepare-binaries
#
# Base image: Google Distroless cc-debian13 (glibc, ~20MB), pinned by digest
# for supply-chain reproducibility. Dependabot (docker ecosystem) will open a
# PR with the new digest when upstream publishes a patched image.

ARG BASE_IMAGE=gcr.io/distroless/cc-debian13:nonroot@sha256:8f960b7fc6a5d6e28bb07f982655925d6206678bd9a6cde2ad00ddb5e2077d78

FROM ${BASE_IMAGE}

ARG VERSION
ARG GIT_SHA
ARG TARGETARCH
ARG BASE_IMAGE

LABEL org.opencontainers.image.source="https://github.com/finos/5-spot" \
      org.opencontainers.image.description="5-Spot Machine Scheduler - Kubernetes Controller for Time-Based Machine Scheduling" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${GIT_SHA}" \
      org.opencontainers.image.base.name="${BASE_IMAGE}"

# Copy the pre-built binary for the target architecture
COPY --chmod=755 binaries/${TARGETARCH}/5spot /5spot

# First-party spot-schedule providers (ADR 0009). Shipped in the same image; their
# deploy manifests select them with `command: ["/spot-schedule-<provider>"]`. Without
# a provider running, ScheduledMachines never see status.active and never activate.
COPY --chmod=755 binaries/${TARGETARCH}/spot-schedule-time-based /spot-schedule-time-based
COPY --chmod=755 binaries/${TARGETARCH}/spot-schedule-capital-markets /spot-schedule-capital-markets

USER nonroot

EXPOSE 8080

ENTRYPOINT ["/5spot"]
