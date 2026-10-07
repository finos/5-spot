<!--
Copyright (c) 2025 Erick Bourgeois, firestoned
SPDX-License-Identifier: Apache-2.0
-->

# Security Policy

5-Spot is a FINOS-incubating project. Its security posture is described
in detail under [`docs/src/security/`](./docs/src/security/); this
document gives the short version: how to report a vulnerability and what
to expect back.

## Reporting a vulnerability

**Do not open a public GitHub issue for security problems.** Please use
one of the private channels below so we can triage and coordinate
disclosure:

1. **GitHub private vulnerability report** (preferred):
   <https://github.com/finos/5-spot/security/advisories/new>
2. **Email**: `security@finos.org` — for general FINOS-coordinated
   disclosure. Include `5-spot` in the subject line.
3. **Direct maintainer**: `erick.bourgeois@gmail.com` — PGP-signed
   messages welcome.

Please include, where possible:

- A description of the vulnerability and its impact.
- The version / commit SHA / container image digest you tested against.
- Steps to reproduce or a minimal proof-of-concept.
- Any suggested mitigation or patch.

## What to expect

| Phase                  | Target                                                              |
| ---------------------- | ------------------------------------------------------------------- |
| Acknowledgement        | 3 business days                                                     |
| Initial triage         | 7 business days                                                     |
| Fix / mitigation ETA   | Shared after triage; depends on severity (CVSS) and blast radius    |
| Public disclosure      | After a fix is released, coordinated with the reporter              |

We follow standard coordinated-disclosure practice: the reporter is
credited in the resulting advisory unless they prefer to remain
anonymous.

## Supported versions

5-Spot follows a rolling-release model until `v1.0.0`. Only the latest
release on `main` receives security fixes during the incubation phase.

## Scope

In scope:

- The 5-Spot controller binary and its container images
  (`ghcr.io/finos/5-spot`, `ghcr.io/finos/5-spot-distroless`).
- The Custom Resource Definitions and admission policies shipped under
  [`deploy/`](./deploy).
- The build and release pipeline
  ([`.github/workflows/`](./.github/workflows)) insofar as a compromise
  would affect published artifacts.

Out of scope:

- Vulnerabilities in upstream dependencies that are already addressed
  in our [`.vex/`](./.vex) directory as `not_affected` — these have a
  documented, reviewed justification. If you disagree with one, please
  still report it so we can re-evaluate.
- Findings that require the reporter to already have cluster-admin or
  equivalent privileges.
- Denial-of-service against a single 5-Spot replica (the operator is
  designed to be run with leader election and horizontal replicas; see
  [`docs/src/operations/multi-instance.md`](./docs/src/operations/multi-instance.md)).

## CRA Escalation (For Maintainers)

CRA stewardship: This project is supported under the Linux Foundation CRA stewardship framework. Security vulnerabilities should be reported through the mechanisms described in this file, which we will coordinate with our CRA steward. For actively exploited vulnerabilities and severe incidents that may require CRA escalation, please use the project's emergency security reporting mechanisms as appropriate. Read more at https://www.linuxfoundation.org/security .

**Project maintainers MUST escalate** the issue to the LF steward at [steward@linuxfoundation.org](mailto:steward@linuxfoundation.org) if the project experiences either of the following:

- **Actively exploited vulnerabilities:** a security vulnerability where the project has reliable evidence that a malicious actor has exploited it.
- **Severe incident:** a security compromise of the project’s own IT infrastructure.

Ordinary vulnerabilities with no evidence of exploitation are not CRA escalation events. Escalate those that are actively exploited.  

## Related documents

- [Threat model](./docs/src/security/threat-model.md)
- [VEX publication process](./docs/src/security/vex.md)
- [Admission policies & validation](./docs/src/security/admission-validation.md)
- [FINOS security policy](https://www.finos.org/hubfs/finos/FINOS%20Vulnerability%20Disclosure%20Policy.pdf)
