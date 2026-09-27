# GitHub Workflows & CI/CD Standards

## CRITICAL: Never Replace `firestoned/github-actions` With Direct Action Calls

ALL GitHub Actions workflows MUST use composite actions from the
`firestoned/github-actions` library where one exists. NEVER replace them with
direct action calls, even if the underlying action version is outdated.

**Why:** `firestoned/github-actions` is owned by the maintainer. When an
underlying action needs a version bump, fix it in the `firestoned/github-actions`
repo — NOT by inlining here.

**Fix process:**
1. Update the action version in the `firestoned/github-actions` repository
2. Tag a new release there
3. Bump the version reference in this repo's workflows

```yaml
# ✅ CORRECT
- name: Cache cargo dependencies
  uses: firestoned/github-actions/rust/cache-cargo@v1.3.6

# ❌ WRONG
- name: Cache cargo dependencies
  uses: actions/cache@v5
```

**Action families in use in this repo** (`.github/workflows/`):
- `firestoned/github-actions/rust/cache-cargo` — Cargo dependency caching
- `firestoned/github-actions/rust/security-scan` — cargo audit
- `firestoned/github-actions/docker/setup-docker` — Docker login + buildx
- `firestoned/github-actions/security/cosign-sign` — image signing
- `firestoned/github-actions/security/license-check` — SPDX header verification
- `firestoned/github-actions/security/verify-signed-commits` — commit signature verification

Direct `uses:` of third-party actions that remain (checkout, upload-artifact,
CodeQL, deploy-pages, …) are pinned by SHA and re-pinned by Dependabot — keep
them SHA-pinned; do not loosen a pin to a floating tag.

---

## CRITICAL: All Workflows Must Be Makefile-Driven

Workflows MUST only: install tools, set env vars, and call Makefile targets.
Business logic lives in the Makefile, so CI runs exactly what a contributor
runs locally.

```yaml
# ✅ GOOD
- name: Validate CALM model
  run: make calm-validate

# ❌ BAD
- name: Validate CALM model
  run: |
    npm install -g @finos/calm-cli
    calm validate -a docs/architecture/calm/architecture.json ...
    # ... a script CI has that no contributor can run ...
```

**Rules:**
- No multi-line bash scripts in `run:` (except trivial tool setup, or a step
  whose *job* is reading workflow context — e.g. `build.yaml`'s
  `Resolve base image reference` step, which reads the `FROM` digest via
  `env:` exactly the way the Makefile's `BASE_IMAGE_REF` does)
- `run:` commands call Makefile targets (`make crds`, not the crdgen
  invocation inline)
- Makefile targets MUST work identically locally and in CI
- Document targets with `##` comments for `make help`

---

## CRITICAL: Workflows Must Be Reusable and Composable

A new workflow MUST support both `workflow_call` (called by other workflows)
and a standalone trigger:

```yaml
on:
  workflow_call:
    inputs:
      image_tag:
        required: true
        type: string
  workflow_dispatch:
    inputs:
      image_tag:
        required: true
        type: string
```

**Checklist before adding a new workflow:**
- [ ] Can this be a job in an existing workflow?
- [ ] Is it reusable via `workflow_call`?
- [ ] Does it duplicate existing logic?
- [ ] Can it be a composite action (in `firestoned/github-actions`)?

---

## Dependabot discipline

- Base-image digests live on literal `FROM` lines (ADR 0010) so the docker
  ecosystem can re-pin them; never move a digest into an `ARG` default.
- Grouped/bundled Dependabot PRs and the auto-merge release flow are
  configured in `.github/dependabot.yml` and
  `.github/workflows/dependabot-auto-merge.yaml`; an approval releases a held
  PR. Keep `base.name` accuracy when touching the release path.
- A CI failure on `cargo-deny`/`cargo audit` in a deps PR usually means a
  transitive crate needs `cargo update -p <crate>` to the advisory's fix
  version — fix the lockfile in the same PR and log it per
  `rules/openwolf.md`.
