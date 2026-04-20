<!--
Copyright (c) 2025 Erick Bourgeois, firestoned
SPDX-License-Identifier: Apache-2.0
-->

# `.vex/` — Per-CVE Triage Source of Truth

This directory is the **human-authored source of truth** for 5-Spot's
[VEX (Vulnerability Exploitability eXchange)](https://github.com/openvex/spec)
statements. CI reads every `.vex/<cve-id>.toml` at release time and assembles
a single signed
[OpenVEX](https://github.com/openvex/spec/blob/main/OPENVEX-SPEC.md)
document that is:

- attached to the GitHub Release as an asset,
- recorded in `checksums.sha256`,
- Cosign-attested against every published image digest (Chainguard +
  Distroless), and
- GitHub-attested via `actions/attest-build-provenance`.

Downstream scanners (Grype, Trivy, Harbor) consume the OpenVEX document
and suppress findings we have already triaged as not applicable.

## When to add a statement

When a scanner (Trivy in CI, or a downstream consumer) flags a CVE on a
5-Spot release artifact, open a PR adding **one file per CVE** in this
directory. Merging is gated by:

1. `tools/validate-vex.sh` (schema + enum + uniqueness checks).
2. Human review of the impact statement.

No automated "everything is `not_affected`" statements are written. Every
statement is explicitly authored and reviewed.

## File format

One TOML file per advisory, named `<identifier>.toml` (case-insensitive match
on the `cve` field; file name is informational). Accepted identifier shapes:

- `CVE-YYYY-NNNN+` — MITRE CVE (the common case).
- `GHSA-xxxx-xxxx-xxxx` — GitHub Security Advisory. Use this when the advisory
  has no assigned CVE yet (e.g. `GHSA-cq8v-f236-94qc`).
- `RUSTSEC-YYYY-NNNN` — RustSec advisory DB.

The TOML field is still named `cve` for backward compatibility with the
original file format; the value may be any of the above.

```toml
cve = "CVE-2025-12345"
status = "not_affected"
justification = "vulnerable_code_not_in_execute_path"
impact_statement = "5-Spot does not parse untrusted XML; the affected libxml2 code path is never invoked."
products = [
    "pkg:oci/5-spot-chainguard",
    "pkg:oci/5-spot-distroless",
]
author = "erick.bourgeois@gmail.com"
timestamp = "2026-04-19T00:00:00Z"
```

### Field reference

| Field              | Required                                                        | Notes                                                                                                        |
| ------------------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `cve`              | yes                                                             | Canonical CVE identifier, `CVE-YYYY-NNNNN+`.                                                                 |
| `status`           | yes                                                             | One of: `not_affected`, `affected`, `fixed`, `under_investigation`.                                          |
| `justification`    | required iff `status = "not_affected"`                          | OpenVEX enum, see below.                                                                                     |
| `impact_statement` | recommended for `not_affected`                                  | Free-form explanation of why the CVE is non-exploitable in 5-Spot.                                           |
| `action_statement` | required iff `status = "affected"` or `"under_investigation"`   | What a consumer should do until a fix is available (e.g. upgrade path, mitigation).                          |
| `products`         | yes, non-empty                                                  | List of product identifiers the statement applies to. Use package URLs (`pkg:oci/...`) or image references.  |
| `author`           | yes                                                             | Email or GitHub handle of the author of the triage.                                                          |
| `timestamp`        | yes                                                             | RFC-3339 UTC timestamp.                                                                                      |

### Allowed `justification` values

Per the OpenVEX spec:

- `component_not_present`
- `vulnerable_code_not_present`
- `vulnerable_code_not_in_execute_path`
- `vulnerable_code_cannot_be_controlled_by_adversary`
- `inline_mitigations_already_exist`

## Local validation

```bash
./tools/validate-vex.sh
```

The validator is the same one CI runs on every PR.

## References

- [OpenVEX specification](https://github.com/openvex/spec)
- [vexctl](https://github.com/openvex/vexctl)
- [Grype `--vex` flag](https://github.com/anchore/grype#supply-chain-security-with-vex)
