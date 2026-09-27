<!--
Copyright (c) 2026 Erick Bourgeois, 5-Spot
SPDX-License-Identifier: Apache-2.0
-->
# 01 — Dependency internalization matrix

> 🔶 In progress. Audited against the tree on **2026-09-23**: of the six
> "remove these first" crates in §1, **four are gone** (`regex`, `lazy_static`,
> `async-trait`, `hyper-util`) and direct runtime deps are down from 31 to 25.
> **`hyper` (still declared with `features = ["full"]`) and `tower` remain** —
> both still have zero non-comment references under `src/`, so §1's reasoning
> stands for them unchanged. `tower-test` is a legitimate dev-dependency and was
> never in scope. The rest of the document is as generated on 2026-06-24 and has
> not been re-verified.

> Analysis of every **direct** dependency in `Cargo.toml`, the actual API surface
> 5-Spot uses from each, and an opinion on whether it could realistically be
> maintained internally instead of depended upon.
>
> Method: ripgrep over production code (`src/**/*.rs`, excluding `*_tests.rs`) for
> every crate's `use` paths, `::` call sites, and macro/derive invocations.
> "Surface" = distinct symbols (types / fns / macros / derives) actually referenced.
> Generated 2026-06-24. No `cargo-udeps`/`cargo-machete` installed, so "unused"
> findings below are grep-confirmed (zero non-comment references) — verify with a
> build before deleting.

---

## TL;DR — the actionable part

**Direct deps declared: 31 runtime + 6 dev. Transitive total: 308 crates.**

1. **6 dependencies appear completely unused in production** and are the highest-value cleanup — likely a `cargo build` away from deletion (see §1).
2. A second tier of **6 thin deps** (1–2 symbols each) are *feasible* to internalize, but most are bad trades — they're tiny precisely because they're well-factored, battle-tested, or carry data you don't want to own (see §3, tier "Thin").
3. The **core 6** (`kube`, `k8s-openapi`, `tokio`, `serde*`, `chrono`) are non-negotiable — internalizing any of them means reimplementing Kubernetes/async/serialization. Don't (see §3, tier "Core").

---

## 1. Remove these first — grep-confirmed unused in production

These have **zero** non-comment references anywhere in `src/`. The only "hits" were
log-filter strings (`"debug,kube=info,hyper=info,tower=info"`) and one comment.

| Crate | Declared as | Evidence | Action |
|---|---|---|---|
| `regex` | runtime dep | 0 references (`Regex`, `regex::` both 0) | **Delete from Cargo.toml** |
| `lazy_static` | runtime dep | 0 references | **Delete** (std `OnceLock`/`LazyLock` exist if ever needed) |
| `async-trait` | runtime dep | 0 references (only line is in Cargo.toml) | **Delete** (modern Rust has native async-fn-in-trait) |
| `hyper` | runtime dep (`features=["full"]`) | 0 real uses — only appears inside a log-filter string | **Delete** (warp pulls its own hyper transitively) |
| `hyper-util` | runtime dep (`features=["full"]`) | 0 references | **Delete** |
| `tower` | runtime dep | 0 real uses — only inside a log-filter string | **Delete** (warp/kube pull tower transitively) |

> Removing `hyper`/`hyper-util` with `features=["full"]` is especially worth it —
> `full` drags in the entire hyper server+client+http1+http2 surface transitively
> for nothing.

**Misclassified (move, don't delete):**

| Crate | Issue | Action |
|---|---|---|
| `http-body-util` | runtime dep, but only used in `helpers_tests.rs` (`BodyExt`) | **Move to `[dev-dependencies]`** |

> `tower-test`, `http`, `base64`, `tokio-test`, `mockall`, `test-log`, `tempfile`
> are already correctly under `[dev-dependencies]`.

**Estimated impact of §1:** removes ~7 direct deps and a meaningful chunk of the
308-crate transitive graph (hyper `full` + tower + regex's `aho-corasick`/`regex-syntax`).

---

## 2. The matrix — sorted by surface area (lowest first)

Surface = distinct symbols used in production. Lower = thinner coupling = easier to
own. "Internalize?" is the engineering call, not just feasibility.

| # | Crate | Surface | Symbols actually used | Files | Internalize? |
|---|---|---:|---|---:|---|
| — | `regex` | **0** | *(none)* | 0 | ✅ delete |
| — | `lazy_static` | **0** | *(none)* | 0 | ✅ delete |
| — | `async-trait` | **0** | *(none)* | 0 | ✅ delete |
| — | `hyper` | **0** | *(none)* | 0 | ✅ delete |
| — | `hyper-util` | **0** | *(none)* | 0 | ✅ delete |
| — | `tower` | **0** | *(none)* | 0 | ✅ delete |
| 1 | `schemars` | 1 | `JsonSchema` (derive) | 1 | ❌ no — CRD schema gen, deep kube/k8s-openapi integration |
| 2 | `chrono-tz` | 1 | `Tz` | 3 | ❌ **no** — that one symbol *is* the entire IANA tz database |
| 3 | `serde_yaml` | 1 | `to_string` | 1 | ⚠️ maybe — but YAML is a deceptively hard format |
| 4 | `tokio-stream` | 1 | `wrappers::ReceiverStream::new` | 1 | ✅ **yes** — ~15 lines, a `Stream` impl over `mpsc::Receiver` |
| 5 | `thiserror` | 1 | `Error` (derive) | 5 | ⚠️ feasible but high-churn — hand-rolled `impl Error` × N error enums |
| 6 | `sha2` | 2 | `Digest`, `Sha256` | 1 | ❌ no — crypto primitive, never hand-roll |
| 7 | `serde` | 2 | `Serialize`, `Deserialize` (derives) | 3 | ❌ no — the derive macros are the entire point |
| 8 | `kube-lease-manager` | 2 | `LeaseManager`, `LeaseManagerBuilder` | 2 | ⚠️ feasible — leader-election over a Lease; ~150–250 lines + edge cases |
| 9 | `clap` | 2 | `Parser`, `ValueEnum` (derives) | 8 | ⚠️ feasible if args stay trivial; derive ergonomics are the value |
| 10 | `futures` | 2 | `StreamExt`, `TryStreamExt` | 6 | ❌ no — core async combinators, used pervasively |
| 11 | `chrono` | ~7 | `DateTime`, `Utc`, `Duration`, `Datelike`, `Timelike`, `TimeZone` | 9 | ❌ no — date/time math is a correctness minefield |
| 12 | `prometheus` | ~9 | `register_counter_vec!`(15), `register_counter!`(7), `register_gauge*`, `HistogramVec`, `gather`, `Encoder`, `TextEncoder` | 1+ | ❌ no — exposition format + registry; owning it is all downside |
| 13 | `warp` | 5 | `Filter`, `serve`, `path`, `reply`, `http` | 2 | ⚠️ **yes-ish** — only 2 tiny HTTP servers (health + metrics); see §4 |
| 14 | `toml` | 5 | `Value`, `from_str`, `to_string`, `de`, `map` | 2 | ❌ no — config format parser, non-trivial |
| 15 | `tracing-subscriber` | 4 | `fmt`, `layer`, `registry`, `EnvFilter` | 5 | ❌ no — log routing/filtering plumbing |
| 16 | `nix` | 7 | `socket::{bind,recv,send,MsgFlags,NetlinkAddr}` (Linux-only) | 1 | ⚠️ feasible — already mostly raw; you parse netlink bytes by hand anyway |
| 17 | `tracing` | 5 | `debug!`, `info!`, `warn!`, `error!`, `Instrument` | 15 | ❌ no — structured logging facade, used everywhere |
| 18 | `tokio` | 10 | `main`, `select!`, `spawn`, `spawn_blocking`, `sync::{mpsc,watch}`, `task::JoinHandle`, `time::{sleep,timeout}` | 11 | ❌ **never** — the async runtime |
| 19 | `anyhow` | 4 | `Result`, `anyhow!`, `bail!`, `Context`, `Error` | 10 | ⚠️ trivially feasible (it's a boxed-error helper) but no reason to |
| 20 | `serde_json` | 13 | `json!`, `Value`, `Map`, `from_str/slice/value`, `to_string`, `to_string_pretty`, `Error` | 13 | ❌ no — JSON is the k8s wire format |
| 21 | `k8s-openapi` | ~6 types | `ConfigMap`, `Node`, `Secret`, `Pod`, `ObjectMeta`, `ObjectReference`, `Taint`, authz v1 | 8 | ❌ **never** — generated k8s API types |
| 22 | `kube` | 25 | `Api`, `Client`, `CustomResource`, `runtime::{Controller,watcher,reflector}`, `controller::Action`, `core::{DynamicObject,GVK}`, `discovery::pinned_kind`, … | 18 | ❌ **never** — this is the whole controller framework |

---

## 3. Three tiers, plainly

**Core — do not touch (internalizing = rewriting Kubernetes/async):**
`kube`, `k8s-openapi`, `tokio`, `serde`, `serde_json`, `chrono`, `futures`,
`tracing`. These are load-bearing and their surface is wide *because the project
genuinely uses them*. Owning any of these is a multi-month liability, not a saving.

**Thin but bad trades (small surface, still keep):**
- `chrono-tz` — 1 symbol, but `Tz` embeds the IANA timezone database. You do **not**
  want to hand-maintain DST rules in a banking scheduler. Keep.
- `sha2`, `serde`, `serde_yaml`, `toml`, `schemars`, `prometheus` — small touch
  points but each encodes a *format/spec/crypto-primitive* that's expensive to get
  right and dangerous to get wrong. Keep.
- `anyhow` — you could replace with `Box<dyn Error>` in an afternoon, but it buys
  nothing; it's a binary-level convenience. Keep.

**Genuinely reasonable to internalize (if you want fewer deps):**
- `tokio-stream` (#4) — **best candidate.** Single use: wrap an `mpsc::Receiver` as
  a `Stream`. ~15 lines implementing `Stream` for a newtype. Drops a dep cleanly.
- `nix` (#16) — you already do the netlink byte-parsing by hand in
  `netlink_proc.rs`; `nix` only provides the `bind/recv/send` syscall wrappers
  (Linux-only). Replacing with raw `libc` is lateral (swaps one dep for another),
  but replacing with direct `syscall`/`std::os::fd` is plausible. Marginal.
- `warp` (#13) — see §4. Two trivial servers; a small hyper-based handler removes
  warp **and** its filter machinery. Medium effort, real transitive savings.
- `kube-lease-manager` (#8), `thiserror` (#5), `clap` (#9) — feasible, see notes,
  but each trades a maintained crate for code you now own forever. Only worth it if
  dependency count is a hard compliance constraint.

---

## 4. The one structural opportunity: `warp`

`warp` is used in exactly two places — `src/health.rs` (healthz/readyz) and
`src/metrics.rs` (the `/metrics` endpoint) — using only `Filter`, `path`, `serve`,
`reply`, `http::StatusCode`. These are three static routes returning strings.

If you delete the unused `hyper`/`hyper-util` from §1, you may reconsider: warp
itself rides on hyper+tower. A ~60–80 line hand-written hyper service could serve
all three routes and let you drop `warp` (and lean on the hyper you'd otherwise
delete). This is the only case where "internalize" both removes a dep *and*
simplifies the transitive graph meaningfully. Worth an ADR if pursued
(architecturally significant: HTTP-serving topology).

---

## 5. Recommended sequence

1. **Now (zero-risk):** delete `regex`, `lazy_static`, `async-trait`, `hyper`,
   `hyper-util`, `tower`; move `http-body-util` to `[dev-dependencies]`. Run
   `cargo build --all-targets` + `cargo-quality`. → ~7 fewer direct deps.
2. **Easy win:** internalize `tokio-stream` (~15 lines) if you want one fewer dep.
3. **Optional, ADR-gated:** collapse `warp` → small hyper service (§4).
4. **Leave everything else.** The remaining deps are either core or thin-but-correct;
   internalizing them adds maintenance liability in a regulated codebase for no
   real benefit.

> Net: the dependency *count* problem is mostly **dead weight (§1), not over-reliance**.
> The crates you actually lean on are the right ones.
