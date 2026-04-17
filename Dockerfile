# Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
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
# Base image: Google Distroless cc-debian12 (glibc, ~20MB)

ARG BASE_IMAGE=gcr.io/distroless/cc-debian13:nonroot

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

USER nonroot

EXPOSE 8080

ENTRYPOINT ["/5spot"]
