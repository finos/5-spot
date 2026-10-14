<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0020: One `5spot` binary with subcommands, shipped in one image

- **Status:** Accepted
- **Date:** 2026-10-14
- **Deciders:** Erick Bourgeois
- **Supersedes:** ADR [0011](./0011-schedule-gated-capacity-separate-controller.md) decision 3 in part (the capacity controller's *binary*; its Deployment, ServiceAccount and RBAC stay separate); ADR [0003](./0003-in-pod-host-service-restart-via-nsenter.md) in part (the restart no longer execs an external `nsenter`; the privileged, `hostPID` stance is unchanged)
- **Related:** ADR [0004](./0004-agent-pod-security-exception-boundary-vap.md), ADR [0009](./0009-unify-schedule-as-provider-reference.md), `src/cli.rs`, `src/cmd/`, `Dockerfile`, `Dockerfile.chainguard`, `deploy/`

## Context

5-Spot builds six runtime binaries: the controller (`5spot`), the capacity
controller, two first-party spot-schedule providers, the reclaim agent and the
Kata-config agent. Only three of them were ever in an image.

| Component | Manifest runs | Shipped in a published image? |
| --- | --- | --- |
| controller | `ghcr.io/finos/5-spot` entrypoint | yes |
| time-based provider | `/spot-schedule-time-based` | yes |
| capital-markets provider | `/spot-schedule-capital-markets` | yes |
| capacity controller | `/5spot-capacity-controller` from `ghcr.io/finos/5-spot` | **no**: the Dockerfiles never copy it |
| reclaim agent | `ghcr.io/finos/5-spot-reclaim-agent` | **no**: no workflow builds that image |
| Kata-config agent | `ghcr.io/finos/5-spot-kata-config-agent` | **no**: no workflow builds that image |

So half the components in `deploy/` cannot start from any release. Each needs a
binary in the right image under the right path, and each of those is a separate
thing to keep in step across `Cargo.toml`, two Dockerfiles, the Makefile, the
release workflow and the manifests. The drift is the predictable result.

The Kata-config agent adds a second constraint. It restarts the host's k0s
service by executing `nsenter -t 1 -m -u -i -n -p -- systemctl restart`
(ADR 0003), so its image needs an `nsenter` binary. Neither base image
(Distroless `cc-debian13`, Chainguard `glibc-dynamic`) has one, and putting one
in the shared image would hand a namespace-entering tool to every component,
including the network-facing controller.

Options weighed:

- **Build the missing images.** Three more images, two of them needing their
  own base (one with `nsenter`), triples the release, signing, SBOM and scanning
  matrix. Rejected: it multiplies exactly the surface that already drifted.
- **One image, add `nsenter` to it.** Rejected: weakens the no-tools base for
  every component to serve one.
- **One image, keep the Kata agent separate.** Rejected: leaves one image that
  has never been built and the same drift for it.

## Decision

1. **One binary, `5spot`, with a subcommand per component:**

   | Subcommand | Component |
   | --- | --- |
   | `5spot controller` | the `ScheduledMachine` controller |
   | `5spot capacity-controller` | the `ScheduledCapacity` controller (ADR 0011) |
   | `5spot provider time-based` | the `TimeBasedSpotSchedule` provider |
   | `5spot provider capital-markets` | the `CapitalMarketsSchedule` provider |
   | `5spot agent reclaim` | the node-side reclaim agent |
   | `5spot agent kata-config` | the node-side Kata-config agent |

   Every flag and environment variable keeps its name and default; each is
   scoped to its subcommand. **Bare `5spot`, with no subcommand, runs the
   controller**, so existing controller Deployments keep working unchanged.

2. **One image per variant, containing only `/5spot`.** Every manifest uses
   `ghcr.io/finos/5-spot` and selects its component with `args`. The
   `5-spot-reclaim-agent` and `5-spot-kata-config-agent` image names are retired.

3. **The Kata agent enters host namespaces itself, in safe Rust.** It re-executes
   its own binary (`/proc/self/exe`) as a hidden `5spot nsenter` subcommand that
   implements the subset of `nsenter` the restart uses: `-t <pid>` and
   `-m -u -i -n -p`. That process is single-threaded, which is what `setns(2)`
   requires for the mount namespace. It opens every
   `/proc/<pid>/ns/<type>` first, enters each with `nix::sched::setns`, then
   runs the command (the host's `systemctl`, resolved in the host's mount
   namespace) and exits with its status. No `unsafe` code and no external
   binary. `privileged: true` and `hostPID: true` remain required, exactly as
   ADR 0003 and ADR 0004 record.

4. **Isolation stays where it actually lives: in identity.** Each component keeps
   its own Deployment or DaemonSet, ServiceAccount, RBAC and pod security
   context. ADR 0011's guarantee holds: the capacity controller's ServiceAccount
   cannot delete a `Machine`, and the controller's `ClusterRole` is not extended.
   Code in an image grants no permission.

5. **Build-time tools stay separate binaries** (`crdgen`, `crddoc`,
   `auto-vex-presence`, `auto-vex-reachability`). They are never in an image.

## Consequences

**Easier.** Every component in `deploy/` runs from a release, three of them for
the first time. One build, one image per variant, one SBOM, one signature, one
scan. A new component is a subcommand, not a new image.

**Harder.** Every pod now carries every component's code. That code is
unreachable unless invoked, and invoking it inside a pod already requires `exec`
into that pod, but it does put the controller's network-facing dependencies into
the privileged agent pods' image, and vulnerability reports for a component now
cover the union. The privileged restart path is rewritten rather than reused;
`nsenter`'s behaviour for the flags used is the specification, pinned in tests.

**Breaking, for anyone using the old paths.** `/spot-schedule-time-based`,
`/spot-schedule-capital-markets` and `/5spot-capacity-controller` no longer exist
in the image, and the two agent images are retired. The manifests in `deploy/`
and the release bundle are updated in the same change; hand-written manifests
must switch to `args`. A bare `5spot` controller is unaffected.

**Verification.** 880 library and 47 binary tests pass (the binary's tests now
cover every subcommand's arguments, backward-compatible bare `5spot`, and the
`nsenter` contract). The restart path was run for real on a Linux host inside
the actual Distroless base image, privileged with the host PID namespace: with
no namespace entered `systemctl` does not exist; with `-t 1 -m -u -i -n -p` the
host's `systemctl` reports `running`; `-u` alone switches the hostname from the
container's to the host's (and without it does not); a failing `systemctl`'s
exit status (4) passes through; a unit beginning with `-` reaches `systemctl` as
a unit; and without privileges the subcommand refuses ("Permission denied") and
runs nothing.

**CALM impact:** none to topology. Components, identities, relationships and
flows are unchanged; the Kata-config agent's control text changes from
"`nsenter -t 1`" to in-process namespace entry.
