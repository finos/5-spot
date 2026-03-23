# Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
# SPDX-License-Identifier: MIT

# PRODUCTION DOCKERFILE - Uses pre-built binaries
#
# This Dockerfile expects pre-built binaries. Build binaries using:
#
#   # On Linux (native)
#   cargo build --release --target x86_64-unknown-linux-gnu
#   mkdir -p binaries/amd64 && cp target/x86_64-unknown-linux-gnu/release/5spot binaries/amd64/
#
#   # Or use Makefile
#   make build-release
#
# Base image: Google Distroless cc-debian12 (glibc, ~20MB)

ARG BASE_IMAGE=gcr.io/distroless/cc-debian12:nonroot

FROM ${BASE_IMAGE}

ARG VERSION
ARG GIT_SHA

LABEL org.opencontainers.image.source="https://github.com/RBC/5-spot" \
      org.opencontainers.image.description="5-Spot Machine Scheduler - Kubernetes Controller for Time-Based Machine Scheduling" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${GIT_SHA}" \
      org.opencontainers.image.base.name="${BASE_IMAGE}"

# Copy the pre-built amd64 binary
COPY --chmod=755 binaries/amd64/5spot /5spot

USER nonroot

EXPOSE 8080

ENTRYPOINT ["/5spot"]
