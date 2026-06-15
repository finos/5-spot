<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0008 — Auto-VEX is generated and signed off before submission, enforced by a byte-exact CI gate

- **Status:** Accepted
- **Date:** 2026-06-14
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR-0001 (ADD); `docs/src/security/vex.md`; `build.yaml` (`auto-vex-presence`, `auto-vex-reachability`, `build-vex`, `validate-vex`)

## Context

5-Spot's two auto-VEX generators (`auto-vex-presence`, Phase 2;
`auto-vex-reachability`, Phase 3) currently run **only inside CI**. On every
push/release they consume live inputs — a Grype scan of the freshly built image,
the image CycloneDX SBOMs, and the release binary's `nm -D --undefined-only`
symbol-import table — emit `not_affected` statements, upload them as workflow
artifacts, and merge them unconditionally into the signed release VEX document.

The merged statements are machine-authored **suppressions**: each one tells
downstream Grype/Trivy consumers to stop reporting a CVE. Today a human never
reviews those suppressions before they are produced — the only review is the
Security team's *post-hoc* counter-signature at release time (the two-signature
trust model in `vex.md`). Between emission and that counter-signature, a
suppression the maintainers have never seen is already baked into the release
artifact. There is also no signal on a PR that the auto-VEX *would change* as a
result of the change under review.

We want the inverse: a maintainer runs the generators **before** submitting,
reads the emitted suppressions, signs off by committing them, and CI refuses any
change whose committed auto-VEX no longer matches what the generators produce.
This mirrors the existing `make crds` → `git diff --exit-code` contract used for
generated CRD YAML.

Alternatives weighed:

- **Commit the output only; CI regenerates from a live scan and diffs.**
  Rejected: a developer cannot reproduce CI's live image scan on a laptop (no
  built image, and the vuln DB drifts hourly), so they would be "signing off"
  something they cannot regenerate, and the diff would fire on every DB refresh
  rather than on a reviewable code change.
- **Commit only a checksum of the canonicalized output.** Rejected: the PR
  reviewer would see a changed hash, not the actual statements being signed off
  — defeating the "human reads the suppressions" goal.
- **Enforce at release time only.** Rejected: lets stale auto-VEX sit on `main`
  until a release surfaces it, which is exactly the late-discovery problem we
  are trying to remove.
- **Semantic diff (compare statement sets, ignore `@id`/`timestamp`/`author`).**
  Rejected in favour of byte-exact: the generators are already deterministic
  (CVE-sorted `BTreeMap`, covered by `output_is_sorted_by_cve_id` tests), and the
  three volatile document-level fields are pinned to canonical constants, so a
  plain `git diff --exit-code` is both sufficient and the simplest possible gate
  to reason about.

## Decision

We commit, and require human sign-off on, a **frozen snapshot of the auto-VEX
inputs and its canonical output**, and enforce consistency with a byte-exact CI
gate that hard-fails on every PR and push.

1. **Committed snapshot inputs** live under `.vex/snapshot/`:
   - `grype.json` — frozen Grype scan report,
   - `sbom-*.json` — one or more frozen CycloneDX SBOMs,
   - `symbols.txt` — frozen `nm -D --undefined-only` of the release binary,
   - `timestamp.txt` — the canonical RFC-3339 UTC sign-off timestamp.

2. **Committed canonical output** lives under `.vex/auto/`:
   - `vex.auto-presence.json`, `vex.auto-reachability.json`.

3. **Deterministic regeneration.** `make vex-auto` runs both generators over the
   committed snapshot with **pinned** `--id`, `--author`, and `--timestamp`
   (timestamp read from `timestamp.txt`), making the output a pure, byte-stable
   function of committed files. A developer reproduces it locally, reads the
   emitted suppressions, and commits to sign off.

4. **CI gate.** A new `validate-autovex` job runs `make vex-auto-check`
   (regenerate + `git diff --exit-code -- .vex/auto`) on every `pull_request`
   and `push`. A mismatch hard-fails the build with instructions to run
   `make vex-auto`, review `.vex/auto/*`, and commit.

The existing release-time CI path (live scan → generators → `vexctl merge` →
Cosign/GitHub attest) is unchanged; the Security team's counter-signature stays
the downstream safety net. This ADR adds an *upstream* human checkpoint, it does
not remove the downstream one.

## Consequences

- **Easier:** suppressions are reviewed on the PR that introduces them; the
  auto-VEX is locally reproducible and diffable; the gate is a trivial
  `git diff --exit-code`, with no semantic-diff tooling to maintain.
- **Harder / follow-up:** the snapshot is a point-in-time freeze and must be
  **refreshed** when dependencies, base images, or the binary's symbol imports
  change. Refresh = capture the live `grype.json`/SBOM/`symbols.txt` from a CI
  run (downloadable artifacts), drop them into `.vex/snapshot/`, bump
  `timestamp.txt`, run `make vex-auto`, review, and commit. Staleness vs. the
  *live* vulnerability landscape is **not** caught by this byte-exact gate (it
  only proves the committed output matches the committed inputs); a future
  scheduled live-drift job (open an issue when a fresh scan diverges from the
  snapshot) is the intended complement and is explicitly out of scope here.
- **Bootstrap:** the snapshot is seeded with valid, empty-finding inputs
  (zero Grype matches → empty auto-VEX documents). This makes the gate green and
  the mechanism real from day one; maintainers MUST replace the placeholder
  snapshot with a real CI-captured one at the next release and re-sign off. See
  `.vex/snapshot/README.md`.
- **CALM impact:** none. This is a CI/CD policy and repository convention; it
  does not change the running system's topology (controller, `ScheduledMachine`
  CRD, CAPI/provider resources, or their flows). Per the ADD rule, process/policy
  decisions carry no CALM update.
