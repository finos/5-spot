<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0007 — CRD multi-version support with `None` conversion and additive-only evolution

- **Status:** Accepted (amended by ADR [0009](./0009-unify-schedule-as-provider-reference.md): `ScheduledMachine` collapsed to a single served version pre-release; the multi-version machinery remains for post-release evolution)
- **Date:** 2026-06-13
- **Deciders:** Erick Bourgeois, 5-Spot team
- **Supersedes:** —
- **Related:** ADR [0006](./0006-pluggable-spot-schedule-provider-contract.md)
  (introduces the public `spotschedules.5spot.finos.org` contract this policy
  governs); `src/crd.rs` (`#[kube(...)]` derive, source of truth); `crdgen`
  (`src/bin/crdgen.rs`); the `regen-crds` / `regen-api-docs` skills.

## Context

5-Spot's CRDs are served at a single version, `v1alpha1`, today
(`5spot.finos.org/v1alpha1`, kind `ScheduledMachine`). ADR 0006 introduces a
**second** API group, `spotschedules.5spot.finos.org`, whose `status.active`
contract is **implemented by third parties** — it is a public API surface, not
an internal type. Public contracts evolve: fields get added, semantics get
clarified, an alpha graduates to beta to stable. We need a deliberate story for
serving more than one version of a CRD **before** we generate the first one, so
the contract can mature without breaking existing `ScheduledMachine`s or
out-of-tree providers.

The constraints that shape the decision:

- **kube-rs derives one version per struct.** `#[kube(version = "…")]` on a
  `CustomResource` derive emits exactly one version. Serving N versions means N
  Rust structs and a **merged** CRD whose `spec.versions[]` lists all of them
  with exactly **one** `storage: true`.
- **Conversion needs either compatibility or a webhook.** Kubernetes converts
  between served versions one of two ways: `conversion.strategy: None`, which
  works **only** when every served version round-trips losslessly through the
  stored object (the API server just relabels `apiVersion`); or a **conversion
  webhook**, which can transform fields but requires a TLS-served HTTPS endpoint
  plus a cert lifecycle (cert-manager / `caBundle` rotation).
- **5-Spot runs no webhook server today.** The controller exposes only
  `/metrics` and `/healthz`. Standing up an admission/conversion webhook is a
  non-trivial new failure domain (TLS, availability, fail-open/closed posture).

**Options weighed:**

1. **Stay single-version, bump in place.** Rejected: breaks every stored object
   and every out-of-tree provider the moment the contract changes; unacceptable
   for a public API.
2. **Multi-version with a conversion webhook from day one.** Rejected *for now*:
   pays the full webhook/TLS/cert cost before any breaking change exists to
   justify it.
3. **Multi-version with `conversion.strategy: None` + additive-only
   evolution.** **Chosen.** Serve multiple versions that are all structurally
   round-trippable through the storage version; constrain changes to be
   additive (new optional fields, never a rename/retype/removal of a served
   field) so `None` conversion stays correct. The first genuinely *breaking*
   change is the documented trigger to introduce a webhook via a superseding
   ADR.

## Decision

1. **5-Spot CRDs are multi-version-capable.** Each served version of a CRD is a
   distinct Rust struct in `src/crd.rs` (e.g. `ScheduledMachine` at
   `v1alpha1`, a future `ScheduledMachineV1Beta1` at `v1beta1`), and `crdgen`
   emits a single CRD object per kind whose `spec.versions[]` lists every served
   version with exactly **one** marked `storage: true`. This policy applies to
   **both** `5spot.finos.org/ScheduledMachine` and the new
   `spotschedules.5spot.finos.org` provider group from ADR 0006.

2. **Conversion strategy is `None`; evolution is additive-only.** Generated
   CRDs set `spec.conversion.strategy: None`. Because the API server performs
   no field transformation under `None`, every served version **must** be
   round-trippable through the storage schema. We therefore constrain
   cross-version changes to be **additive** — new **optional** fields only;
   never rename, retype, remove, or change the meaning of a field that an older
   served version exposes. Defaulting is handled in the Rust types / reconciler,
   not by conversion.

3. **Resolvers and watchers are version-agnostic.** No code keys off a single
   hardcoded `apiVersion` string. The spot-schedule resolver and dynamic watch
   (ADR 0006 §5) key off **`group` + `kind`** and accept any *served* version of
   a provider; the CEL pin on `spotSchedule.apiVersion` validates the **group**,
   not a specific version.

4. **Generation and CI guards.** `crdgen` produces multi-version YAML; serde
   round-trip tests cover **every served version** of every kind; a CI/test
   guard asserts the **single-`storage: true`** invariant per CRD. CRD shape
   changes still start in `src/crd.rs` and flow through `regen-crds` →
   `regen-api-docs` (LAST), never hand-edited YAML.

5. **The webhook trigger is recorded, not built.** The first change that cannot
   be expressed additively (a true breaking change to a served version) is the
   trigger to introduce a conversion webhook. That is a **new architectural
   decision** — it gets its own ADR superseding the relevant part of this one,
   covering the webhook's TLS/cert lifecycle and fail posture. Until then,
   5-Spot ships **no** webhook.

## Consequences

- **Easier:** the public `spotschedules` contract (and `ScheduledMachine`
  itself) can grow new versions without breaking stored objects or out-of-tree
  providers, and without 5-Spot standing up a webhook server — no TLS, cert, or
  webhook-availability burden while the contract is young.
- **Harder / constraints accepted:** cross-version changes are **disciplined to
  additive-only**; a rename/retype/removal is *not* a routine change — it forces
  the webhook ADR. Reviewers must police this on every CRD edit. Multiple Rust
  structs per kind add some duplication, and round-trip tests multiply by served
  version count.
- **Operational invariant:** exactly one version per CRD is `storage: true`;
  the CI guard fails the build otherwise. Reading/writing always converges on
  the storage version with no lossy transformation.
- **Ruled out (for now):** conversion webhooks; non-additive version changes;
  version-pinned resolver/watch code. Each is reintroduced only via a
  superseding ADR.
- **CALM impact:** **none.** This is a generation/versioning policy for the CRD
  artifacts and the type layer — it changes neither the running system's
  topology (controller, agents, CAPI/provider resources, flows) nor any
  trust boundary. CALM models the system, not the CRD-version-evolution policy,
  so there is no node/relationship/flow to add.
