<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0021: The reclaim agent polls `/proc`, and holds no `NET_ADMIN`

- **Status:** Accepted
- **Date:** 2026-10-14
- **Deciders:** Erick Bourgeois
- **Supersedes:** ADR [0004](./0004-agent-pod-security-exception-boundary-vap.md) in part (decision 5's `NET_ADMIN` exception for the reclaim agent; every other agent exception stands)
- **Related:** ADR [0019](./0019-k0smotron-end-to-end-tier-in-kind.md), ADR [0020](./0020-one-binary-one-image-with-subcommands.md), `src/cmd/reclaim_agent.rs`, `src/netlink_proc.rs`, `deploy/node-agent/daemonset.yaml`, `deploy/admission/agent-pod-security-policy.yaml`

## Context

The reclaim agent detects a matching process in one of two ways: polling
`/proc` (rung 1), or subscribing to the kernel's process-events connector over
netlink (rung 2, sub-10 ms). Its default, `--detector=auto`, picks netlink on
Linux, and the DaemonSet is granted `NET_ADMIN` for it, the one capability
ADR 0004 lets any agent add.

**The netlink connector only delivers to the host's initial network
namespace.** Measured on a Linux host as root, with `CAP_NET_ADMIN`, using the
repository's own live test (`tests/integration_netlink_proc.rs`):

| Network namespace | `Subscriber::new()` | Result |
| --- | --- | --- |
| host (initial) | succeeds | both tests pass |
| a fresh one (`unshare -n`) | `Connection refused (os error 111)` | both tests fail |

A pod without `hostNetwork` is in the second row, and the agent pod-security
policy (ADR 0004 decision 2) denies `hostNetwork` to every agent. So, as
shipped, the agent's default detector can never open its socket on Linux:
`Subscriber::new()` fails, the agent exits, and the DaemonSet crash-loops. Emergency
reclaim has not worked out of the box, and the `NET_ADMIN` it holds is
unusable. No test caught it: the netlink test runs only by hand, as root, in
the host namespace, which is exactly the case that works.

Options weighed:

- **Admit `hostNetwork` for the reclaim agent.** Makes netlink work as
  designed. Rejected: the agent would then share the node's network stack
  while holding `NET_ADMIN`, which is able to rewrite the node's routes and
  firewall. That is a far larger grant than the latency gain (under 10 ms
  against a 250 ms poll) is worth for an eject that drains the node anyway.
- **Only fall back to polling.** Stops the crash loop, but leaves a privilege
  granted that can never be used, which is the opposite of least privilege.

## Decision

1. **`auto` falls back to polling.** On Linux it tries the netlink connector
   once at start-up; if the socket cannot be opened it logs a warning naming
   the reason and polls `/proc`. An explicit `--detector=netlink` still fails
   loudly: an operator who asked for netlink is told it is unavailable.
2. **The shipped DaemonSet polls and adds no capability.**
   `deploy/node-agent/daemonset.yaml` sets `RECLAIM_DETECTOR=poll` and drops
   `NET_ADMIN`: `capabilities.drop: [ALL]` with nothing added.
3. **The agent pod-security policy admits no capability adds at all.** Rule 7
   of `deploy/admission/agent-pod-security-policy.yaml` no longer carves out
   `NET_ADMIN` for the reclaim agent. Netlink remains in the code for anyone who
   deliberately runs the agent in the host network namespace outside this
   policy, but 5-Spot's own posture no longer grants what it cannot use.

## Consequences

**Easier.** Emergency reclaim works on Linux as shipped. The reclaim agent's
privilege drops to `hostPID` plus read-only host mounts; no 5-Spot pod adds a
capability. The threat model's T13 (misuse of `NET_ADMIN`) is closed rather
than accepted.

**Harder.** Detection latency is the poll interval (250 ms by default) instead
of under 10 ms. For a trigger whose next step is a drain, that is not
material. Polling also sees only processes that survive a tick; a process that
starts and exits inside one interval is missed, as it always was in poll mode.

**Verification.** The k0smotron e2e tier (ADR 0019) now deploys the reclaim
agent from the shipped manifest and drives a real match on the worker through
to the emergency removal.

**CALM impact:** the reclaim agent's controls drop the `NET_ADMIN` grant; no
topology change.
