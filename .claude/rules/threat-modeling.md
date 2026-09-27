# Threat Modeling

> **After implementing an ADR, do a full pass over the threat model.**
> `docs/src/security/threat-model.md` is a living document under Architecture
> Driven Development. It is the **last step of the ADD cycle**, after docs —
> and an ADR is not "done" until it has been done.

## The rule

When the implementation of an ADR is complete (code written, tests green,
CHANGELOG and `docs/src/` updated), make a **full pass** over
`docs/src/security/threat-model.md` before declaring the task finished.

Full pass means walking **every** section, not appending a row to the one
table that obviously changed. An ADR that adds a controller also adds an
actor, probably an identity, possibly a trust boundary, and may invalidate a
residual risk recorded four sections away.

## Why

The threat model states a **posture**: what is defended, from whom, with which
control in which file. Its header asserts a specific claim:

```
**Version:** 1.1
**Date:** 2026-09-23
**Covers:** ADR-0001 … ADR-0010
```

Every merged ADR that isn't reflected makes that claim false. A stale threat
model is worse than no threat model: an absent one prompts analysis, while a
stale one asserts a posture nobody has actually checked and is trusted anyway.
**This repo has already paid for this once**: the pre-v1.1 model was last
checked 2026-04-08, four ADRs went in after it, and the most security-relevant
component in the tree — the privileged kata-config agent that writes the host
filesystem and restarts a host systemd unit via `nsenter -t 1` — appeared in
it exactly once, incidentally, inside another threat's mitigation text. The
v1.1 rewrite is what this rule exists to make unnecessary.

## Trigger questions

Run the ADR against each of these. Any **yes** means that section changes
(section numbers per `docs/src/security/threat-model.md`):

| Question | Section to revisit |
| --- | --- |
| New binary, controller, agent, provider, or DaemonSet? | §2 System Overview, §5 Threat Actors |
| New CRD, contract, or field carrying user-controlled data? | §3 Assets, §6 STRIDE |
| New credential, Secret read, kubeconfig, or token path? | §3 Assets, §4 Trust Boundaries |
| New identity, ServiceAccount, RBAC grant, or admission policy (VAP)? | §6, §7 Mitigations |
| Anything that touches a host: filesystem write, `nsenter`, `hostPID`, privilege? | §4, §6, §8 |
| New call out to a child cluster, registry, or external API? | §4 Trust Boundaries |
| New external dependency in the build or boot path? | §3 Assets (supply chain) |
| Does data now cross a boundary it didn't before — or is there a **new** boundary? | §4 (incl. any diagram), §6 |
| Does it weaken, strengthen, or invalidate a recorded residual risk? | §8 Residual Risks |
| Does it break an assumption in §9? | §9 Security Assumptions |

## Requirements for the pass

1. **Every new or changed threat maps to a concrete control** that exists in
   `deploy/` or `src/` — cite the actual file, the way the existing tables
   do. A threat with no control is not a table row: it is either
   - an entry in **§8 Residual Risks** with an explicit *revisit when*, or
   - a finding to fix **before** the ADR counts as implemented.

   Never write a control that does not exist yet as though it does.

2. **Classify with STRIDE**, per boundary, matching the existing §6 format.

3. **Keep §2/§4 structurally true** — a new component missing from the system
   overview or the boundary description is a missed pass, not a cosmetic
   omission.

4. **Bump the header stamp** — **Version**, **Date**, and the **Covers** ADR
   range (`ADR-0001 … ADR-NNNN`). **This is the deliverable.** An unchanged
   stamp means the pass did not happen, regardless of what else was edited.

5. **"No change" is a valid outcome** — but it is a *conclusion*, not a skip.
   Bump the stamp anyway and record it in `.claude/CHANGELOG.md`
   ("threat model pass: no boundary changes; Covers advanced to ADR-NNNN").

6. **Never record a specific unremediated vulnerability here.** This page is
   public and ships in the docs site. Findings go through
   [private vulnerability reporting](https://github.com/finos/5-spot/security/advisories/new),
   per the document's own header and `SECURITY.md`.

7. **No real infrastructure identifiers and no PII** —
   `rules/no-real-infrastructure.md` and `rules/no-pii.md` apply here like
   everywhere else. Threat models attract concrete hostnames; use the
   placeholder tables.

## Scope

**Full pass required** for any ADR that reached implementation — the same set
of changes that required an ADR in the first place
(`rules/architecture-driven-development.md`). A Proposed ADR with no
implementation yet needs no pass; the pass lands with the implementation.

**Not required** for TDD-only changes (typos, isolated bugfixes, mechanical
refactors) — those never had an ADR. But if a "trivial" fix turns out to
change who can reach what, it wasn't trivial: write the ADR, then do the pass.

## Checklist

- [ ] All 10 sections of `docs/src/security/threat-model.md` walked, not just the obvious one
- [ ] Every trigger question above answered against this ADR
- [ ] New/changed threats classified with STRIDE and mapped to a real file in `deploy/` or `src/`
- [ ] Uncontrolled threats either fixed or recorded in §8 with a *revisit when*
- [ ] §2 overview and §4 boundaries match reality
- [ ] Header stamp bumped: Version, Date, **and** Covers ADR range
- [ ] `.claude/CHANGELOG.md` records the pass (with `**Author:**`)
- [ ] No unremediated finding, no real infrastructure identifier, no PII committed
