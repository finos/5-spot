<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# Architecture Decision Records

This directory holds 5-Spot's **Architecture Decision Records (ADRs)** — the
durable log of *why* the system is shaped the way it is.

5-Spot follows **Architecture Driven Development (ADD)**: for any
architecturally significant change, the decision is recorded here **before** code
is written, then modeled in CALM, then implemented test-first. See
[`.claude/rules/architecture-driven-development.md`](../../.claude/rules/architecture-driven-development.md)
for the full methodology.

## Conventions

- **Filename:** `NNNN-title.md` — zero-padded sequential number, lowercase, hyphenated.
- **One decision per ADR.** Copy [`template.md`](./template.md) to start.
- **Status lifecycle:** `Proposed` → `Accepted` → (`Superseded by ADR-NNNN`).
  Never delete or rewrite a decision — supersede it and link forward.
- **In the repo.** ADRs are version-controlled here. (Roadmaps and phase plans
  are *not* — those live outside the repo at `~/dev/roadmaps/`.)

## When to write one

Write a full ADR (and update the CALM model) for: new CRDs/CRD-field contract
changes, controllers/reconcilers/binaries, changes to the CAPI interaction
(Machine / bootstrap / infrastructure contract, allowed API groups), deploy /
admission / GitOps topology, and cross-cutting concerns (security boundaries,
RBAC posture, scheduling semantics). Process/policy decisions get an ADR with
**no CALM impact**. Trivial changes (typos, isolated bugfixes, mechanical
refactors) need neither. When unsure, **write the ADR.**

## Index

| ADR | Title | Status |
|----:|-------|--------|
| [0001](./0001-adopt-architecture-driven-development.md) | Adopt Architecture Driven Development (ADD) | Accepted |
