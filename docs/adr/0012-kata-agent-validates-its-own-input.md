<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0012 — The kata-config agent validates its own input, and the restart argv terminates options

- **Status:** Accepted
- **Date:** 2026-09-27
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR [0002](./0002-kata-config-delivery-via-spec-kata.md) (config
  delivery via `spec.kata`), ADR
  [0003](./0003-in-pod-host-service-restart-via-nsenter.md) (the `nsenter`
  restart), ADR [0005](./0005-remove-kata-destpath-fixed-host-path.md) (which
  closed the *file* half of this same threat), ADR
  [0004](./0004-agent-pod-security-exception-boundary-vap.md);
  [Threat Model](../src/security/threat-model.md) §6.5 rows K2, K4 and K5.

## Context

ADR-0005 closed one half of a two-halved threat. A `ScheduledMachine` author
could name the host path the privileged kata-config agent wrote to, so the field
was removed outright: the destination is a fixed constant, and
`confine_dest_path` canonicalizes and re-checks it against `/etc/k0s/` before
every write and unlink, fail closed. Nothing a CR or an annotation says can
redirect the write.

**The command half never got the same treatment.** The agent also restarts a
host systemd unit, and the unit name travels the same path the host path used to:

1. `ScheduledMachine.spec.kata.restartService`, validated by the CRD schema at
   the **management cluster's** admission boundary;
2. the controller writes it into the `5spot.finos.org/kata-config-ref`
   annotation on a Node in the **workload cluster**
   (`build_kata_config_ref_annotation_patch`);
3. the agent deserializes that annotation with a bare
   `serde_json::from_str` and passes the value to
   `nsenter -t 1 -m -u -i -n -p -- systemctl restart <value>`.

Two defects follow, and they compound.

**The argv does not terminate systemctl's options.** The `--` in that command
line belongs to `nsenter`; it separates nsenter's flags from the command to run.
There is no second `--`, so `systemctl` parses a value beginning with `-` as an
option rather than a unit — `-H` connects to a remote host over SSH, `-M`
targets a container, `--root=` retargets the filesystem. This is reachable
**through the CRD**, not only through the annotation: the schema pattern
`^[A-Za-z0-9@._-]+\.service$` includes `-` in the character class with nothing
anchoring first position, so a value like `-Hbar.example.com.service` satisfies
it.

**The agent's real input is not the CRD.** It is a Node annotation in the
workload cluster, which no schema gates. Anything holding `patch nodes` there
can write any string into it — and per the threat model's §8 residual, that
includes both agents' own ServiceAccounts, because RBAC cannot express "the Node
this pod runs on". The agent cannot distinguish an annotation the controller
wrote from one anything else wrote. Validating only at the management cluster
puts the check on the wrong side of the boundary that matters.

Options considered:

1. **Validate only in the CRD** (status quo). Rejected: the schema does not run
   where the value is consumed, and K5 already records the annotation as an
   untrusted input.
2. **Validate only in the agent.** Better, but it lets a bad value be admitted
   and stored, failing later on a node instead of at `kubectl apply`.
3. **Both, plus an argv that cannot be misread.** Chosen.

## Decision

**The agent treats its Node annotation as untrusted input and validates every
field it deserializes, and the restart command line terminates option parsing
before the unit name.**

1. **`systemctl restart -- <unit>`.** A second `--` is added between the
   subcommand and the unit. After it, `systemctl` cannot interpret the value as
   an option, whatever it contains. This is the load-bearing fix: it holds even
   if every other check is bypassed or removed.

2. **`parse_kata_ref` validates, and returns a typed error.** Every field is
   checked against the same constraint the CRD enforces for its counterpart —
   `restartService` against the unit pattern, `namespace` as an RFC-1123 label,
   `name` as a DNS subdomain, `key` as a ConfigMap/Secret key — and a failure is
   a rejected annotation, not a sanitized one. The agent logs it, sets the
   condition, and does nothing else: no write, no restart.

3. **The CRD pattern is anchored against a leading hyphen**:
   `^[A-Za-z0-9@._][A-Za-z0-9@._-]*\.service$`. A hyphen remains legal inside a
   unit name, where systemd allows it, and illegal in first position, where it
   only ever means an option.

4. **The three are defence in depth, and each is load-bearing.** Decision 1 makes
   a hostile value inert; decision 2 rejects it earlier, on the node, with a
   clear reason; decision 3 stops it being admitted at all. Removing any one is
   a regression, and the tests name which defect each covers so a future reader
   cannot mistake one for redundancy.

## Consequences

**Easier.** The command half of the threat now matches the file half: nothing a
CR or an annotation says can make the agent run something other than
`systemctl restart` on a syntactically valid unit name. A malformed annotation
fails visibly on one node instead of silently doing something else.

**Harder / accepted costs.**

- A valid-but-unwanted unit is still restartable. `sshd.service` matches the
  pattern and always will; the pattern constrains *syntax*, not *policy*. Who
  may set `spec.kata` remains the control, and §7 of the threat model already
  states that grant is node-root-equivalent. An allowlist of units was
  considered and rejected: it would have to be configured per distribution, and
  a wrong allowlist fails closed on a legitimate restart during an incident.
- The agent now rejects annotations a previous version accepted. That is the
  point, but it means an operator who hand-edited an annotation to something
  outside the pattern sees a rejection after upgrade rather than silence.
- **K5 is narrowed, not closed.** The agent no longer trusts the annotation's
  *shape*; it still trusts its *authority*. Anything with `patch nodes` can
  still point a node at a different ConfigMap or Secret within the pattern. That
  residual stays open until a `ValidatingAdmissionPolicy` restricts who may write
  `5spot.finos.org/kata-config-*` — recorded in §8 with that recommendation.

**CALM impact: yes.** `architecture.json` carries controls on nodes and
relationships, and this adds one to the agent's annotation-read path — the
existing nodes, relationships and flows are unchanged, so it is a control edit
rather than a topology edit. `make calm-validate` and `make calm-diagrams` gate
it.

**Threat model impact.** §6.5 K2 (command injection) gains the `--` as a second,
independent control; K4 (restart of an unintended unit) moves from *partially
mitigated* to mitigated for the injection case, with the policy case named as
out of scope; K5 is narrowed as described above. A pass is due with the
implementation.
