<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0010 — Base-image digests are pinned on the `FROM` line so Dependabot re-pins them

- **Status:** Accepted
- **Date:** 2026-09-07
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0008](./0008-autovex-presubmission-gate.md) (supply-chain gates), `.github/dependabot.yml`, `Dockerfile`, `Dockerfile.chainguard`, `SECURITY.md`

## Context

5-Spot ships two runtime images built from two root Dockerfiles:

| File | Base |
|------|------|
| `Dockerfile` | `gcr.io/distroless/cc-debian13:nonroot` |
| `Dockerfile.chainguard` | `cgr.dev/chainguard/glibc-dynamic:latest` |

Both bases are rolling tags: the tag is stable, the content is rebuilt (daily,
in Chainguard's case) to absorb CVE fixes. Both were therefore pinned by
digest, and `.github/dependabot.yml` declared a `docker` ecosystem entry so
Dependabot would open a PR each time the tag moved.

**That never worked.** The digests were written as

```dockerfile
ARG BASE_IMAGE=gcr.io/distroless/cc-debian13:nonroot@sha256:8f96…
FROM ${BASE_IMAGE}
```

so the `BASE_IMAGE` build-arg stayed overridable for air-gapped builds against
an Artifactory mirror (README → "Air-Gapped Builds"). But Dependabot's docker
parser reads `FROM` instructions only — it does not expand `ARG`
([dependabot-core#2691], open since 2020). `FROM ${BASE_IMAGE}` does not match
its image regex, so the line was silently skipped, the updater found zero
dependencies, and the entry produced no PRs. Three comments in the tree
(`Dockerfile`, `Dockerfile.chainguard`, `dependabot.yml`) asserted that this
was working. The digests had not moved since they were first set.

Two second-order problems came with it. The `Makefile` defaulted
`BASE_IMAGE ?= gcr.io/distroless/cc-debian12:nonroot` — an *unpinned* tag, and
a different Debian generation from the `Dockerfile` default — so every local
`make docker-build-*` built against a base CI never builds. And CI passes no
`BASE_IMAGE` at all, so the two paths had silently diverged.

Options weighed:

1. **Keep `ARG` + `FROM ${BASE_IMAGE}`, re-pin by hand on a calendar.** Rejected:
   this is exactly the manual toil the `docker` ecosystem entry exists to remove,
   and the last two years show it does not happen.
2. **Hard-pin `FROM`, delete the `BASE_IMAGE` override.** Simplest, and it makes
   Dependabot work — but it breaks the documented air-gapped/mirrored build
   path, which matters most in precisely the regulated environments 5-Spot
   targets.
3. **Hard-pin `FROM` in a named stage, and default the override to that stage.**
   Dependabot sees a real `FROM image:tag@sha256:…` line and rewrites the
   digest; the build still resolves through `${BASE_IMAGE}`, whose default is
   the stage name. Chosen.

## Decision

Base-image digests live on a real `FROM` instruction, in a named `pinned-base`
stage, and the override defaults to that stage:

```dockerfile
ARG BASE_IMAGE=pinned-base
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:8f96… AS pinned-base
FROM ${BASE_IMAGE}
```

- A digest pin is **never** hidden in an `ARG` default. Any new Dockerfile
  follows the same three-line shape.
- `ARG BASE_IMAGE` is declared before the first `FROM` (a global build arg) so
  it is usable in `FROM`; `pinned-base` is a forward reference, resolved when
  the second `FROM` is evaluated.
- The `Makefile`'s `BASE_IMAGE` / `CHAINGUARD_BASE_IMAGE` variables default to
  **empty** and expand to a `--build-arg` only when explicitly set, so an
  ordinary `make docker-build-*` builds the same pinned digest CI builds. Setting
  one is an explicit, documented decision to trust a mirror instead of the pin.
- `org.opencontainers.image.base.name` records what the build **actually**
  pulled, via a `BASE_IMAGE_REF` build-arg. It is never a hardcoded literal: an
  air-gapped build against an internal mirror would then ship a label naming an
  upstream registry it never contacted. `BASE_IMAGE` itself cannot serve — it
  holds the *stage name* by default — so every caller resolves the reference and
  passes it:
    - the `Makefile` (`BASE_IMAGE_REF` / `CHAINGUARD_BASE_IMAGE_REF`) → the
      override when set, otherwise the first `FROM` read out of the matching
      Dockerfile;
    - `.github/workflows/build.yaml` → a `Resolve base image reference` step
      doing the same first-`FROM` read on the variant's Dockerfile. CI builds
      every *published* image and never overrides `BASE_IMAGE`, so this must be
      passed there too or the shipped label is empty.
- `.github/dependabot.yml` records what the `docker` entry does and does not
  cover, and why base images are reviewed one PR at a time rather than grouped.

## Consequences

**Easier.** A moved base-image tag now produces a real PR with the new digest,
through the same review + auto-merge gate as every other dependency. Local and
CI builds resolve to the same base by default. The air-gapped override survives
unchanged.

**Harder / ruled out.**

- The `pinned-base` stage is dead code when `BASE_IMAGE` is overridden. BuildKit
  (Docker's default builder since v23) and Podman/Buildah with
  `--skip-unused-stages` elide it. A *classic* (`DOCKER_BUILDKIT=0`) builder
  walks every stage and would still try to resolve the upstream pin — an
  air-gapped classic build must therefore use BuildKit, which the documented
  `docker buildx` flow already does.
- `base.name` is derived, not written, so it never drifts from the `FROM` line —
  but the derivation (`awk '$1 == "FROM" { print $2; exit }'`) now exists twice,
  in the `Makefile` and in `build.yaml`. That duplication is deliberate: a Make
  indirection just to feed a CI step costs more than the one-liner. Any *third*
  way of building these images has to resolve `BASE_IMAGE_REF` as well, or it
  publishes an image whose `base.name` is the empty string.
- Not everything is reachable by Dependabot: workflow `container:` images
  (`semgrep/semgrep` in `sast.yaml`, digest-pinned by hand) and `kindest/node`
  tags in the `Makefile` / `examples/` are parsed by no ecosystem and stay
  manual — they are re-pinned alongside the SHA-pinned actions. The
  deploy manifests' own `ghcr.io/finos/5-spot*` tags are deliberately excluded —
  `make set-image-version` owns them.
- The 7-day `cooldown` on the docker ecosystem means we intentionally run a
  base-image digest up to a week behind the freshest rebuild. That is a
  bake-in-versus-CVE-latency trade-off, now written down at the setting.

**CALM impact:** none (process/build-policy decision; no change to the running
system's nodes, interfaces, or flows).

[dependabot-core#2691]: https://github.com/dependabot/dependabot-core/issues/2691
