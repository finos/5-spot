<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 0017: The Machine's bootstrap and infrastructure references are serialized in the shape the discovered CAPI contract requires

- **Status:** Accepted
- **Date:** 2026-10-09
- **Deciders:** Erick Bourgeois
- **Supersedes:** (none)
- **Related:** ADR [0007](./0007-crd-multi-version-and-conversion.md) (CRD multi-version), roadmap [06](../../.github/community/06-k0smotron-e2e-and-worker-confinement.md), `src/reconcilers/helpers.rs`, `src/constants.rs`

## Context

5-Spot already refuses to hardcode a CAPI API version. `main` resolves the
served `cluster.x-k8s.io` Machine version at startup through mandatory
discovery (`resolve_capi_machine_version`), records it once, and
`machine_api_version_for` lets a user's `bootstrapSpec.apiVersion` pass that
version through to the created Machine. The module documentation states the
goal plainly: "One binary works against CAPI lines serving `v1beta1` (<= v1.10)
or `v1beta2` (v1.11+ / modern k0smotron) with no config."

Only half of that was implemented. Discovery returned the version; the Machine
body was still built in exactly one shape:

```rust
"configRef": { "apiVersion": …, "kind": …, "name": …, "namespace": … }
```

That is the `v1beta1` shape, a core `ObjectReference`. In `v1beta2` Cluster API
replaced both `spec.bootstrap.configRef` and `spec.infrastructureRef` with a
`ContractVersionedObjectReference`, whose only fields are `apiGroup`, `kind`
and `name`. `apiVersion` and `namespace` are gone, and the reference is
same-namespace by definition.

The removed fields are **not** pruned. The Machine type decodes strictly, so
the surviving half of the feature failed loudly rather than quietly. Verified
against Cluster API v1.14.3 with k0smotron v2.2.0:

```
Error from server (BadRequest): Machine in version "v1beta2" cannot be handled
as a Machine: strict decoding error:
  unknown field "spec.bootstrap.configRef.apiVersion",
  unknown field "spec.bootstrap.configRef.namespace",
  unknown field "spec.infrastructureRef.apiVersion",
  unknown field "spec.infrastructureRef.namespace"
```

The blast radius was narrower than "5-Spot does not work on CAPI v1.11+",
which is why it survived review. `machine_api_version_for` takes the version
from the user's `bootstrapSpec.apiVersion`, so a `ScheduledMachine` declaring
`bootstrap.cluster.x-k8s.io/v1beta1` created its Machine at
`cluster.x-k8s.io/v1beta1`, where the emitted shape was correct and the
conversion webhook stored it as `v1beta2`. Two paths were broken:

1. a `ScheduledMachine` declaring a `v1beta2` bootstrap or infrastructure spec,
   which is the forward path as `v1beta1` is deprecated and will be removed;
2. the discovery fallback, used whenever `bootstrapSpec` carries no parsable
   version, which resolves to the cluster's *recommended* version and is
   therefore `v1beta2` on any current cluster.

Alternatives weighed:

- **Pin to `v1beta1` and emit only the legacy shape.** Rejected: it discards
  the runtime discovery the controller already performs and dies with the
  `v1beta1` removal, converting a current bug into a scheduled outage.
- **Emit both field sets and let the server ignore the surplus.** Rejected:
  strict decoding makes "ignore the surplus" false. It is rejected outright at
  both versions.
- **Require the operator to pin the shape via an env var.** Rejected: the
  controller can already determine the answer, and
  `CAPI_MACHINE_API_VERSION` remains available for the case where discovery
  must be overridden.

## Decision

The Machine's `spec.bootstrap.configRef` and `spec.infrastructureRef` are
serialized in the shape the target contract version requires, by a single
function keyed on the Machine API version the controller is creating at:

- for the contract versions listed in `CAPI_LEGACY_OBJECT_REFERENCE_VERSIONS`
  (`v1alpha3`, `v1alpha4`, `v1beta1`), the legacy `ObjectReference`:
  `apiVersion` + `kind` + `name` + `namespace`;
- for every other version, the `ContractVersionedObjectReference`:
  `apiGroup` + `kind` + `name`, with the group derived from the referenced
  object's `apiVersion` and the version segment dropped.

An unrecognised version resolves to the **modern** shape. The legacy shape is
the one being removed, so a CAPI line newer than this list converges on the
contract that replaced it rather than on the one that is going away. The list
is an explicit allowlist of known-legacy versions, not a comparison, because
CAPI versions are not ordered strings.

## Consequences

**Easier.** One binary spans CAPI v1.9 through v1.14 and whatever follows, with
no configuration, which is what the discovery code always intended. A
`ScheduledMachine` may now declare `v1beta2` bootstrap and infrastructure specs
and migrate off the deprecated `v1beta1` on its own schedule.

**Harder.** There are now two serializations to keep in mind when reading or
changing Machine construction. The drift risk is contained by
`machine_contract_ref` being the only place either shape is written, and by six
unit tests in `src/reconcilers/helpers_tests.rs` that pin both shapes, the
group derivation, and the unknown-version default.

**Ruled out.** Writing a Machine reference anywhere other than through
`machine_contract_ref`. A second call site reintroduces exactly the divergence
this ADR exists to close.

**Follow-up.** `CAPI_LEGACY_OBJECT_REFERENCE_VERSIONS` is a fixed list. If CAPI
ever ships a *third* reference shape, the keying function generalises to a
match rather than a boolean, and this ADR is superseded rather than amended.

**Verification.** Confirmed end to end on a three-node k0s management cluster
running Cluster API v1.14.3 and k0smotron v2.2.0: the controller logged
`Discovered CAPI Machine API version: v1beta2`, and the stored Machine carried

```yaml
bootstrap:
  configRef: {apiGroup: bootstrap.cluster.x-k8s.io, kind: K0sWorkerConfig, name: spot-worker-01}
infrastructureRef: {apiGroup: infrastructure.cluster.x-k8s.io, kind: RemoteMachine, name: spot-worker-01}
```

reaching Machine phase `Running` with the worker Node `Ready`. Detail in
roadmap [06](../../.github/community/06-k0smotron-e2e-and-worker-confinement.md).

**CALM impact:** none. The nodes, relationships and flows are unchanged: the
same controller creates the same Machine against the same CAPI core service
over the same interface. Only the serialization of a field inside an existing
flow changes, and the contract version was never modelled as topology.
