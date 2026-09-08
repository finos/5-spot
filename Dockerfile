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
# for supply-chain reproducibility.
#
# The digest MUST live on a real `FROM` line: Dependabot's docker ecosystem
# only parses `FROM` instructions, so a digest hidden in an `ARG` default
# (`FROM ${BASE_IMAGE}`) is invisible to it and never gets re-pinned. With the
# pin below, Dependabot opens a PR rewriting the digest whenever Google
# publishes a patched cc-debian13:nonroot. See ADR 0010.
#
# BASE_IMAGE remains overridable for air-gapped / mirrored builds (README →
# "Air-Gapped Builds"). It defaults to the `pinned-base` stage, so an ordinary
# build always resolves to the digest pinned here:
#   make docker-build-amd64 BASE_IMAGE=<mirror>/distroless/cc-debian13:nonroot
# An override deliberately bypasses the digest pin — the mirror is then the
# trusted source.
ARG BASE_IMAGE=pinned-base

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:8f960b7fc6a5d6e28bb07f982655925d6206678bd9a6cde2ad00ddb5e2077d78 AS pinned-base

FROM ${BASE_IMAGE}

ARG VERSION
ARG GIT_SHA
ARG TARGETARCH

# Reference recorded in org.opencontainers.image.base.name. Supplied by the
# Makefile, which resolves it to whatever the build actually used: the
# BASE_IMAGE override when set, otherwise the pinned `FROM` read out of this
# file. Never hardcode the upstream registry here — an air-gapped build from an
# internal mirror would then ship a label naming a registry it never contacted.
# `BASE_IMAGE` itself is unusable for this: it holds the stage name by default.
ARG BASE_IMAGE_REF

LABEL org.opencontainers.image.source="https://github.com/finos/5-spot" \
      org.opencontainers.image.description="5-Spot Machine Scheduler - Kubernetes Controller for Time-Based Machine Scheduling" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${GIT_SHA}" \
      org.opencontainers.image.base.name="${BASE_IMAGE_REF}"

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
