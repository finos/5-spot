<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0005 — Remove `spec.kata.destPath`; fix the host path to `/etc/k0s/containerd.d/kata.toml`

- **Status:** Accepted
- **Date:** 2026-06-10
- **Deciders:** Erick Bourgeois
- **Supersedes:** —
- **Related:** ADR-0002 (kata delivery contract), ADR-0003 (privileged in-pod
  host write + `nsenter` restart), CALM node `service-kata-config-agent`,
  security roadmap H-1 gate (`~/dev/roadmaps/5spot-security-fix-roadmap-2026-06-09.md`)

## Context

ADR-0002/0003 shipped kata config delivery with a user-configurable
`spec.kata.destPath` whose CRD schema accepted **any** absolute path
(`^/[^\0]*$`). Combined with the agent's privileged posture (root, hostPath
mount, `nsenter` into host PID 1 — ADR-0003), this is a latent High (the H-1
gate): anyone permitted to create/patch a `ScheduledMachine` and supply a
source object can direct a root-owned write of arbitrary content to **any
host path** (`/etc/cron.d/…`, `/root/.ssh/authorized_keys`, …) — effectively
node root via the CRD. The agent's tear-down path has the same exposure
through the `kata-config-applied` annotation's recorded `destPath` (root
unlink of an arbitrary host file), and that annotation is forgeable by anyone
holding `patch nodes`.

The configurability existed to serve non-k0s layouts (kairos, vanilla
containerd, full-file `/etc/kata-containers/configuration.toml` override). In
practice the deployment target is k0s, whose containerd imports drop-ins from
exactly one place — `/etc/k0s/containerd.d/`
([k0s runtime docs](https://docs.k0sproject.io/stable/runtime/)) — so the
field buys risk without a user.

Two corrections fall out of checking the official docs: the previous default
`/etc/k0s/container.d/kata-containers.toml` used a directory (`container.d`)
that **does not exist in k0s** — the real import dir is `containerd.d` — so
the default was silently undeliverable on a stock k0s node.

Alternatives weighed:

- **Schema allowlist on a kept field** (`^/etc/k0s/[A-Za-z0-9._/-]+\.toml$` +
  agent-side canonicalized containment). Viable and was prototyped, but keeps
  a path-shaped attacker input flowing CRD → annotation → privileged write,
  with RE2's no-lookahead limitation pushing `..`-traversal rejection into
  agent code. Rejected: for a field with no current user, removing the input
  beats validating it.
- **Configurable agent-side allowlist (env/flag).** More machinery for the
  same residual input. Rejected.
- **Do nothing.** Carries node-root-via-CRD into a regulated environment.
  Rejected.

## Decision

`spec.kata.destPath` is **removed from the CRD**. The destination is the
compile-time constant `KATA_CONFIG_DEST_PATH = /etc/k0s/containerd.d/kata.toml`
— the k0s containerd import directory and conventional drop-in name.

Supporting changes that follow from "no path travels through the contract":

1. The controller's `kata-config-ref` Node annotation no longer carries
   `destPath`; the agent never reads a path from the API.
2. The agent's `kata-config-applied` annotation simplifies from the
   `{destPath, hash}` record back to the **bare content hash** (or `absent`):
   with a fixed path, tear-down no longer needs a recorded location, and a
   forged annotation can no longer steer a root unlink.
3. Defense-in-depth stays: every write **and** unlink still resolves through
   `confine_dest_path` (`/etc/k0s/` prefix + `.toml` suffix, lexical `..`/`.`
   rejection, canonicalized parent must stay under the canonicalized base) —
   now guarding the constant against host-side symlink games rather than
   validating user input. Fail closed.
4. The DaemonSet's hostPath mount narrows from `/` to `/etc/k0s` — with no
   configurable path there is no reason for the agent to see the whole host
   filesystem. (`nsenter` for the service restart is unaffected; that is
   namespace entry, not filesystem mount.)

## Consequences

- The arbitrary-host-write / arbitrary-unlink vector through `spec.kata` is
  **eliminated**, not validated: no CRD field, no annotation field, no agent
  input carries a host path. The H-1 gate is closed.
- The agent's writable host surface shrinks to `/etc/k0s` via the narrowed
  hostPath mount.
- **Ruled out:** per-SM destination paths, full-file
  `/etc/kata-containers/configuration.toml` override, and non-k0s layouts.
  Re-introducing any of these requires a superseding ADR and a deliberate
  re-opening of the input-validation question.
- One drop-in file per node (`kata.toml`). Multiple `ScheduledMachine`s
  binding the same node would contend for one file — unchanged from the
  previous default behaviour, now explicit.
- **Breaking (pre-release):** any `ScheduledMachine` setting `kata.destPath`
  fails admission once the regenerated CRD (with
  `deny_unknown_fields` semantics on `KataConfig`) is applied. The feature has
  not shipped in a release; no migration is provided.
- A file written under the old contract to a non-standard path is invisible
  to the new tear-down (which only manages the constant path) and must be
  removed manually.
- CALM updated: `host-path-containment` control on
  `service-kata-config-agent` rewritten for the fixed-path design; node
  description and hostPath wording updated.
