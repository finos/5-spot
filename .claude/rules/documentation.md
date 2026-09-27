# Documentation Rules

## Roadmap Document Naming (hard rule)

Roadmap docs live in `.github/community/`, indexed by `ROADMAPS.md` at the repo
root. Their filenames obey three rules, with no exceptions:

1. **Lowercase, hyphens only.** `04-dependency-internalization-matrix.md` —
   never `04-DEPENDENCY-MATRIX.md`, never `04_dependency_matrix.md`. Same rule
   as `docs/adr/NNNN-title.md`. `README.md` in that directory is the sole
   uppercase name. A version in a filename loses its dots:
   `03-release-diff-v0-2-2-to-main.md`, not `v0.2.2`.
2. **Zero-padded two-digit prefix, contiguous from `00`.** No gaps, no decade
   grouping — `00` through `NN` in one run. A number is a position in the
   reading order, not a category.
3. **Renumbering is a whole-repo edit.** Inserting or retiring a roadmap
   renumbers the run after it, so the same commit must also fix:
   - `ROADMAPS.md` rows (label **and** link target) and
     `.github/community/README.md` (the reading-order table, first column
     included);
   - the numbered `# NN — Title` H1 inside the doc itself;
   - cross-links between roadmap docs;
   - every `roadmap NN` prose reference — these reach outside this directory,
     into `docs/adr/`, `docs/src/`, `examples/` and Rust doc comments. A doc
     comment under `src/crd.rs` also lands in `deploy/crds/*.yaml`, so run the
     `regen-crds` skill afterwards.

Refer to a roadmap in prose by its padded number — "roadmap 01", not
"roadmap 1" — so the reference greps against the filename.

Verify with one command; it should return nothing outside the range:

```sh
rg -ioN 'roadmaps? \[?[0-9]{2}' --glob '!target/**' . | sort -u
ls .github/community/ | rg -v '^README\.md$'   # must be 00..NN, all lowercase
```

Past entries in `.claude/CHANGELOG.md` keep the numbers that were true when
they were written — they are a historical record, not an index. A renumbering
commit records the old → new mapping there instead.

## What may not be published

`.github/community/` is public. Two categories stay in the maintainer's private
directory outside the repo (`~/dev/roadmaps/5-spot/`), whatever else the
document contains:

1. **Unremediated security findings.** The published security *posture* is
   `docs/src/security/threat-model.md` — what is defended, from whom, with which
   control in which file. Specific open findings go through private
   vulnerability reporting per `SECURITY.md`. A residual risk already recorded
   in the threat model's §8, with its mitigation named, is posture and may be
   discussed; a live unfixed defect is not.
2. **Real infrastructure identifiers** — hostnames, IP addresses, cluster
   snapshots, internal registry or artifactory paths, or any internal
   organisational reference. This is the internal-references rule in
   `.claude/CLAUDE.md`; it applies to roadmaps exactly as it applies to code.

When a private document's findings are fixed, the fix is recorded the normal
way — an ADR, the CHANGELOG, and the threat model's control tables — not by
importing the private document.

## Status board discipline

`ROADMAPS.md` answers "what state is this project in" on one screen. Update the
status row in the **same commit** that changes an item's state; a roadmap row
that describes intent rather than reality is worse than no row.

When you touch a roadmap detail doc, **audit the rest of it against the tree**
rather than editing only the line you came for. A stale checkbox is a claim the
repository contradicts. Say so explicitly when an item turns out to be
superseded rather than done — the two are different outcomes and the
distinction is the useful part.
