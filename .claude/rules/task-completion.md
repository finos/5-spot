# Task Completion Gates

> **Two gates run at the end of every task, before reporting it complete:
> `cargo-quality` and `sync-docs`.** Not at the end of a session, not before a
> commit only: at the end of each task, including the small ones.

**This rule is the authority; the `Stop` hook in `.claude/settings.json` is a
nudge that points at it.** Both exist deliberately, because each fails in a way
the other does not: a hook fires reliably but into a message that is easy to
scroll past, and a rule can say *why* but only works if it is read.

That is not a theoretical balance. On 2026-10-08 the gates were made a rule and
the hook was removed as redundant; the session that did it had already gone the
whole way without running `sync-docs`, with the obligation stated twice in
`.claude/CLAUDE.md` the entire time. The prose mandate alone demonstrably did
not fire. The hook went back the same day.

## Gate 1: `cargo-quality`

Required whenever the task touched **any** `.rs` file.

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

All three exit 0. A task with a failing test or a clippy warning is not
complete, it is in progress. Do not report it as done, and do not defer the
fix to "a follow-up" without saying so explicitly in the response.

## Gate 2: `sync-docs`

Required for **every** task, including ones that touched no Rust.

`src/crd.rs` is the source of truth. Check that every YAML snippet and field
table in `docs/src/` still matches it, per the `sync-docs` skill in
`.claude/SKILL.md`. The usual divergences are a renamed field, a `snake_case`
name that was never converted to `camelCase`, a field removed from the Rust
type but surviving in an example, and an enum whose variants grew.

Generated files are regenerated, never hand-edited:

```sh
make crds      # deploy/crds/*.yaml      from src/crd.rs
make crddoc    # docs/src/reference/api.md
```

Both must be idempotent: run them, then confirm `git diff` is empty for their
outputs. A dirty diff after a regeneration means the committed artifact had
drifted.

## Why both, every time

They fail in opposite directions, which is why one does not substitute for the
other.

`cargo-quality` catches what the compiler and the test suite can see.
`sync-docs` catches what they cannot: a doc comment in `src/crd.rs` lands in
the generated CRD and in `kubectl explain`, so prose is a published API surface
here, and nothing in the build will tell you it went stale. Documentation drift
is also silent in exactly the way a test failure is not, so it accumulates
until someone follows an example that no longer works.

## When the gates do not apply

- A task that changed nothing (a question answered, a file read) has nothing to
  gate.
- A task interrupted or redirected mid-way: say plainly that the gates have not
  run yet and what state the tree is in. An ungated tree reported as finished
  is the failure this rule exists to prevent.
- If a gate cannot run (no toolchain, a Linux-only crate on macOS), say so and
  name the host it needs, rather than quietly skipping it.

## Relationship to the other rules

This is the last step of ordinary work, and it sits **inside** step 4 of the
ADD cycle (`rules/architecture-driven-development.md`). ADD adds further
obligations on top for architecturally significant changes: the roadmap and
`ROADMAPS.md` row in the same commit, and the full threat-model pass with its
stamp bumped. Passing these two gates does not mean an ADR is done; see
`rules/threat-modeling.md`.
