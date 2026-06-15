<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# `.vex/snapshot/` — frozen auto-VEX inputs (sign-off baseline)

This directory holds a **point-in-time freeze** of the inputs the two auto-VEX
generators consume, so that the canonical output in [`../auto/`](../auto/) is a
**pure, byte-reproducible function of committed files**. That reproducibility is
what lets CI enforce the pre-submission gate with a plain `git diff --exit-code`
(see [ADR 0008](../../docs/adr/0008-autovex-presubmission-gate.md) and
[`docs/src/security/vex.md`](../../docs/src/security/vex.md)).

## Files

| File              | What it is                                                              |
| ----------------- | ---------------------------------------------------------------------- |
| `grype.json`      | Frozen Grype scan report (`grype --output json`).                      |
| `sbom-*.json`     | One or more frozen CycloneDX SBOMs (a purl present in ANY counts).     |
| `symbols.txt`     | Frozen `nm -D --undefined-only <release-binary>` output.              |
| `timestamp.txt`   | Canonical RFC-3339 UTC **sign-off timestamp** stamped into the output. |

## The contract

1. A maintainer runs `make vex-auto`, which regenerates `../auto/*.json` from the
   files here using **pinned** `@id`/`author`/`timestamp`.
2. The maintainer **reads the emitted `not_affected` suppressions**, then commits
   both the snapshot and the regenerated output to **sign off**.
3. CI runs `make vex-auto-check` on every PR/push and **hard-fails** if the
   committed `../auto/*.json` no longer matches a fresh regeneration.

## ⚠️ Bootstrap placeholder — replace before relying on this

The committed snapshot is currently a **valid but empty bootstrap**: `grype.json`
has zero matches, the SBOM has zero components, and `symbols.txt` is empty, so
both auto-VEX documents contain zero statements. This makes the gate green and
the mechanism real from day one, but it carries **no real triage signal yet**.

**Maintainers must replace it with a real capture** at the next release:

```sh
# From a CI run (Actions → workflow run → Artifacts), download the real inputs:
#   - the grype-triage JSON report
#   - the docker SBOM(s)            -> sbom-<variant>.json
#   - the auto-vex-reachability-evidence symbol dump
cp <downloaded>/grype.json        .vex/snapshot/grype.json
cp <downloaded>/docker-sbom-*.json .vex/snapshot/      # rename to sbom-*.json
cp <downloaded>/symbols.txt       .vex/snapshot/symbols.txt
date -u +%Y-%m-%dT%H:%M:%SZ     > .vex/snapshot/timestamp.txt

make vex-auto        # regenerate ../auto/*.json
git diff .vex/auto   # review every suppression being signed off
git add .vex/snapshot .vex/auto && git commit
```

## Refreshing

Refresh whenever dependencies, base images, or the binary's symbol imports
change (i.e. whenever a live CI scan would diverge from this freeze). The
byte-exact gate proves the committed output matches the committed **inputs** — it
does **not** detect drift from the *live* vulnerability landscape. Catching that
drift is the job of a future scheduled live-scan job (out of scope for ADR 0008).
