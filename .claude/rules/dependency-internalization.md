# Dependency Internalization

> **Prefer owning small code to tracking an upstream release cadence for it.**
> Where the surface we actually use is small, stable and fully understood,
> internalize it and drop the dependency. Where the dependency encodes a
> *specification*, a *cryptographic primitive*, *reference data* or a
> *compiler plugin*, keep it, however small our surface looks.

The motivation is upgrade churn and upstream latency: a one-symbol dependency
still obliges us to track its releases, triage its advisories, and wait on a
maintainer for a fix we could have made ourselves in an afternoon. That logic
is sound for glue. It inverts for anything whose *implementation* is large or
whose correctness is a moving target.

## The test

A dependency is a candidate when **all** of these hold:

1. **The replacement is 500 lines or fewer**, measured as the code we would
   actually write (excluding comments and blanks), not the size of the upstream
   crate.
2. **It is not in an excluded category** (below).
3. **We can test it at least as well as upstream does** for the surface we use.
   If the behaviour we depend on is hard to pin with tests, we do not
   understand it well enough to own it.
4. **Dropping it removes the crate from the production graph.** A crate that
   other dependencies pull in anyway is still compiled, still in the SBOM, and
   still ours to upgrade, so internalizing it buys nothing but duplicate code.

   Verify with an **explicit production target**, not the dev machine's:

   ```sh
   cargo tree -e normal --target x86_64-unknown-linux-gnu | grep -c '<crate> v'
   ```

   `cargo tree -e normal` alone reports the host target, so on macOS it hides
   every `[target.'cfg(target_os = "linux")'.dependencies]` entry. `nix` reads
   as absent from the production graph that way and is actually in it: 225
   crates on linux-gnu against 217 on macOS. A measurement taken on the wrong
   target is worse than no measurement, because it looks like a finding.

Measure, do not estimate. Record the numbers in the ADR.

## Excluded categories, and why

Do **not** internalize these, no matter how few symbols we touch:

| Category | Examples here | Why |
| --- | --- | --- |
| **Specifications and wire formats** | `serde_json`, `serde_yaml`, `toml`, `prometheus` | Our surface is often one call (`to_string`); the implementation is a conformant parser or exposition format. Owning it means owning its parsing CVEs and its spec drift. YAML in particular is far harder than it looks. |
| **Cryptographic primitives** | `sha2` | Never hand-roll. Also an audit and FIPS question in a regulated environment, not merely a correctness one. |
| **Reference data** | `chrono-tz` | `Tz` is one symbol and *is* the IANA timezone database. DST rules change several times a year by government decree. Hand-maintaining them inside a scheduler is the worst possible place for that risk. |
| **Date and time arithmetic** | `chrono` | Leap years, leap seconds, DST transitions, ambiguous local times. A correctness minefield with no upside to owning. |
| **Procedural macros** | `serde`, `schemars`, `clap`, `thiserror` | The used "surface" is one or two derive names, but the implementation is a compiler plugin. There is no 500 lines to bring over: the alternative is hand-written impls at every call site, forever. `schemars` additionally feeds CRD generation. |
| **The platform** | `kube`, `k8s-openapi`, `tokio`, `futures`, `tracing` | Internalizing these means reimplementing Kubernetes or an async runtime. Their surface is wide because the project genuinely uses them. |

A thin surface over a thick, moving specification is the trap this table
exists to name.

## Requirements when we do internalize

1. **Write the behavioural contract down, then test it.** The module doc states
   every property the replacement must honour, and `_tests.rs` pins each one.
   Upstream's edge cases are the specification; losing them silently is the
   failure mode.
2. **Reimplement, do not vendor, where practical.** Writing against the
   underlying API avoids carrying a third-party licence and notice into an
   Apache-2.0 FINOS repository. If code genuinely must be copied, preserve the
   original licence header and record it in the module doc and the ADR.
3. **State the provenance.** Name the crate and version replaced, so the next
   reader can diff against upstream if a bug appears.
4. **Note the supply-chain effect.** Internalized code leaves the SBOM, so
   `cargo audit`, `cargo deny` and Grype stop reporting on it: a defect in it
   is now found by review, not by a scanner. That is an accepted trade for
   small, well-tested code, and a reason not to make it for anything large.
5. **An ADR per internalization**, with the measured numbers (our LOC, upstream
   LOC, the symbols used, the `cargo tree -e normal` before/after). The policy
   itself is [ADR 0015](../../docs/adr/0015-dependency-internalization-policy.md).

## Current state

The per-dependency analysis lives in roadmap
[04](../../.github/community/04-dependency-internalization-matrix.md) §2, which
records the exact symbols used by every direct dependency. Keep it current when
a dependency is added or dropped.
