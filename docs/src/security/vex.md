<!--
Copyright (c) 2025 Erick Bourgeois, firestoned
SPDX-License-Identifier: Apache-2.0
-->

# VEX (Vulnerability Exploitability eXchange)

5-Spot publishes a signed
[OpenVEX](https://github.com/openvex/spec/blob/main/OPENVEX-SPEC.md)
document with every GitHub Release. The document records, per CVE, whether
the finding is `not_affected`, `affected`, `fixed`, or
`under_investigation` in **this specific release** of 5-Spot, plus an
OpenVEX-spec justification when we claim non-exploitability.

VEX closes the gap between "Grype flagged a CVE" and "is this CVE
actually reachable in 5-Spot?". The 5-Spot CI pipeline feeds the VEX
document into [Grype](https://github.com/anchore/grype) (`grype --vex
...`) to suppress pre-triaged findings before they reach GitHub Code
Scanning; downstream consumers can do the same with Grype or with
[Trivy](https://aquasecurity.github.io/trivy/latest/docs/supply-chain/vex/),
so the triage burden does not land on every downstream team
independently.

---

## What is published, and how

On every release of 5-Spot the CI pipeline performs the following steps:

1. **Parse** every file under `.vex/*.json` (each is a single-statement
   native OpenVEX document) via `vexctl merge`. Malformed input fails
   the merge — there is no separate validator to keep in sync.
2. **Generate presence-based auto-VEX** (roadmap Phase 2): the
   `auto-vex-presence` job runs a Grype triage scan on each image
   variant without VEX suppression, then emits a `not_affected +
   component_not_present` statement for every finding whose affected
   package URL is absent from the image SBOM and not already covered
   by a hand-authored statement. The result is uploaded as the
   `vex-auto-presence` workflow artifact.
3. **Generate symbol-import reachability auto-VEX** (roadmap Phase 3):
   the `auto-vex-reachability` job inspects the release binary's
   dynamic symbol-import table (`nm -D --undefined-only`) and emits a
   `not_affected + vulnerable_code_not_in_execute_path` statement for
   every Grype finding whose CVE id is present in the curated
   `.vex/.affected-functions.json` mapping **and** whose listed
   library functions are *all absent* from the imports. The result
   is uploaded as the `vex-auto-reachability` artifact, with the raw
   symbol-import dump alongside as `vex-auto-reachability-evidence`.
4. **Assemble** a single OpenVEX document (`vex.openvex.json`) with
   `vexctl merge`, stamped with a canonical
   `@id = https://github.com/<owner>/<repo>/releases/tag/<tag>/vex`
   and the release actor as the document-level author. Both
   auto-* documents are included in the merge on every build —
   there is no feature-flag gate.
5. **Cosign-attest** the document to *both* image digests (Chainguard
   and Distroless). The attestation lands in the Sigstore transparency
   log and is pushed to the registry alongside the image.
6. **GitHub attest** the document with `actions/attest-build-provenance`
   so `gh attestation verify` works for downstream pulls.
7. **Attach** `vex.openvex.json` and its `.bundle` to the GitHub
   Release as assets and register them in `checksums.sha256`.

No new GitHub secrets are required — all signing is keyless via the
GitHub OIDC token and Sigstore Fulcio.

---

## Consuming the VEX document

### With Grype

```sh
grype --vex vex.openvex.json \
    ghcr.io/<owner>/5-spot-chainguard@<digest>
```

Grype will suppress every CVE the document marks `not_affected` or
`fixed` for the scanned image, with the OpenVEX statement as the audit
record.

### With Trivy

```sh
trivy image \
    --vex file:vex.openvex.json \
    ghcr.io/<owner>/5-spot-chainguard@<digest>
```

### Verifying the Cosign attestation end-to-end

```sh
cosign verify-attestation \
    --type openvex \
    --certificate-identity-regexp '^https://github.com/<owner>/5-spot' \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    ghcr.io/<owner>/5-spot-chainguard@<digest>
```

### Verifying the GitHub attestation

```sh
gh attestation verify vex.openvex.json --repo <owner>/5-spot
```

Replace `<owner>` with the GitHub organization hosting your 5-Spot
release (for example, `finos`).

---

## How a 5-Spot maintainer adds a statement

When a new CVE surfaces on a release artifact, open a PR adding a
single file to [`.vex/`](https://github.com/finos/5-spot/tree/main/.vex).
Each file is a native OpenVEX v0.2.0 single-statement document:

```json
{
  "@context": "https://openvex.dev/ns/v0.2.0",
  "@id": "https://github.com/finos/5-spot/.vex/CVE-2025-12345",
  "author": "maintainer@example",
  "timestamp": "2026-04-19T00:00:00Z",
  "version": 1,
  "statements": [
    {
      "vulnerability": {"name": "CVE-2025-12345"},
      "products": [{"@id": "pkg:oci/5-spot"}],
      "status": "not_affected",
      "justification": "vulnerable_code_not_in_execute_path",
      "impact_statement": "5-Spot does not parse untrusted XML; the affected libxml2 code path is never invoked.",
      "timestamp": "2026-04-19T00:00:00Z"
    }
  ]
}
```

The document-level `@id`, `author`, and `timestamp` fields are replaced
by CI at release time with a canonical release-tag `@id`, the release
actor, and the release timestamp. Only the statement contents (inside
`statements[]`) carry forward into the merged release document.

The PR gate on `build.yaml` (`validate-vex` job) runs `vexctl merge`
over every `.vex/*.json` file; any malformed file fails the merge and
blocks the PR. The same tool runs again on release for
belt-and-suspenders.

### Required fields per status (statement level)

| `status`              | Extra required field | Notes                                                              |
| --------------------- | -------------------- | ------------------------------------------------------------------ |
| `not_affected`        | `justification`      | OpenVEX enum. `impact_statement` is strongly recommended.          |
| `affected`            | `action_statement`   | What users should do until fixed (upgrade path, mitigation, etc.). |
| `under_investigation` | `action_statement`   | Same — give consumers something actionable.                        |
| `fixed`               | —                    | Just declares the CVE no longer applies to this release.           |

All four statuses additionally require `vulnerability.name`, `products`,
and `timestamp` (RFC-3339 UTC).

Validate locally with `make vex-validate`.

---

## What we automate, and what stays human

The VEX document is a trust claim, not a compliance artifact. If 5-Spot
automatically emitted `not_affected` for every Grype finding, the
document would be worthless the moment Grype missed a true positive.
That constraint rules out "auto-triage everything" but not all
automation — specifically, the `component_not_present` justification
has a purely mechanical definition ("the vulnerable component is not
in the product"), and the SBOM is the authoritative definition of
what's in the product.

The split is therefore:

- **Automated — presence** (roadmap Phase 2, active on every build):
  if Grype flags a CVE on a package whose `purl` is not in any image
  SBOM, and the CVE isn't already triaged in `.vex/`, the
  `auto-vex-presence` job emits a `not_affected +
  component_not_present` statement. The SBOM digest is the evidence
  backing the claim.
- **Automated — symbol-import reachability** (roadmap Phase 3, active
  on every build): if Grype flags a CVE that's listed in the curated
  `.vex/.affected-functions.json` mapping, and **none** of the listed
  public-API function names appear in the release binary's dynamic
  symbol-import table, the `auto-vex-reachability` job emits a
  `not_affected + vulnerable_code_not_in_execute_path` statement.
  The raw `nm -D --undefined-only` output is uploaded as the
  `vex-auto-reachability-evidence` artifact and is the evidence
  backing the claim.
- **Hand-authored** (everything else): `not_affected` with any
  justification other than `component_not_present` /
  `vulnerable_code_not_in_execute_path`, plus all `affected`,
  `fixed`, and `under_investigation` statements, stay in `.vex/*.json`
  and go through PR review. Grype findings drive maintainers to write
  statements; the statements themselves are deliberate human
  decisions.

Both automated paths are merged unconditionally into the signed VEX
document — there is no feature-flag gate. The Security team's
downstream re-verification (see "Trust model" below) is the safety
net.

### Why symbol-imports, not LLVM-IR call graphs

The original Phase 3 plan targeted Rust LLVM-IR call-graph
reachability against RustSec advisories carrying `affected.functions`.
At implementation time, `cargo audit` reported zero open Rust-level
vulnerabilities for this codebase — every CVE in `.vex/` is a
base-image glibc/zlib finding from Grype scanning the Docker image,
none of which RustSec tracks. The Rust call-graph approach addressed
zero current findings.

The mechanical equivalent for our actual data is symbol-import
absence: if the Rust binary doesn't link against `glob` / `scanf` /
`iconv` / etc., the affected glibc code paths cannot be reached
through their documented entry points. The `nm -D --undefined-only`
output is auditable and the Security team re-derives the same
conclusion by running `nm` against the release-attested binary
themselves.

---

## Trust model

5-Spot operates a **two-signature** trust model for VEX:

1. **CI emits and Cosign-attests** the merged VEX document
   (hand-authored + auto-presence) against both image digests on every
   push + release. The attestation is keyless via GitHub OIDC and lands
   in the Sigstore transparency log alongside the SBOM attestations,
   SLSA provenance, and GitHub build-provenance bundle.
2. **The Security team independently verifies and counter-signs**. On
   each release they:
   - Re-run `vexctl merge` over the committed `.vex/*.json` and the
     uploaded `vex-auto-presence` artifact, diffing against the signed
     `vex.openvex.json` attached to the release.
   - For each auto-generated `component_not_present` statement,
     re-derive presence by inspecting the signed SBOM attestations
     (`cosign download attestation --predicate-type cyclonedx.json`).
   - For each hand-authored statement, review the `impact_statement`
     against the release source tree at the tagged commit.
   - Apply their own Cosign attestation to the same image digests if
     they agree (`cosign attest --type openvex` under the Security
     team's OIDC identity), adding a second signature to the
     transparency log.

Downstream consumers can require both attestations via
`cosign verify-attestation` with two `--certificate-identity-regexp`
invocations (one matching the CI identity, one matching the Security
team's). The design keeps the VEX document auditable even when the
5-Spot team automates its generation aggressively: if the CI emits a
claim the Security team cannot substantiate, the image ships with
only one signature on the VEX — which is a discoverable condition for
any downstream gate.

This is why it is safe for 5-Spot CI to auto-generate
`component_not_present` statements by default: incorrect suppressions
are caught at verification time, not at emission time.

---

## References

- [OpenVEX specification](https://github.com/openvex/spec)
- [`vexctl`](https://github.com/openvex/vexctl)
- [Cosign OpenVEX attestation docs](https://docs.sigstore.dev/cosign/verifying/attestation/)
- [Grype `--vex` flag](https://github.com/anchore/grype#supply-chain-security-with-vex)
- Repository convention: [`.vex/README.md`](https://github.com/finos/5-spot/blob/main/.vex/README.md)
