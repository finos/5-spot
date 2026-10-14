---
description: chronological action log per session, consolidated weekly
---
# Memory

> Chronological action log. Hooks and AI append to this file automatically.
> Old sessions are consolidated by the daemon weekly.

## Session: 2026-09-27 22:53

| Time | Action | File(s) | Outcome | ~Tokens |
|------|--------|---------|---------|--------|
| 22:57 | Created .claude/rules/no-pii.md | — | ~1694 |
| 22:57 | Created .claude/settings.local.json | — | ~2279 |
| 22:57 | Created .claude/settings.json | — | ~88 |
| 22:57 | Created .claude/settings.json | — | ~88 |
| 22:58 | Edited .claude/CLAUDE.md | expanded (+17 lines) | ~313 |
| 22:58 | Edited .claude/CHANGELOG.md | expanded (+45 lines) | ~624 |
| 22:59 | Edited .claude/CHANGELOG.md | inline fix | ~4 |
| 23:05 | Added no-PII rule (home dirs, usernames, IPs, emails — open-ended); moved OpenWolf hooks to settings.local.json with $CLAUDE_PROJECT_DIR; scrubbed /Users paths from old changelog entries; wired rule into CLAUDE.md + changelog + cerebrum | .claude/rules/no-pii.md, .claude/settings{,.local}.json, .claude/CLAUDE.md, .claude/CHANGELOG.md, .wolf/cerebrum.md | tracked settings.json back to HEAD; tree sweep clean | ~5000 |
| 23:45 | Edited .github/community/03-release-diff-v0-2-2-to-main.md | 00 → 03 | ~11 |
| 23:45 | Edited .github/community/04-dependency-internalization-matrix.md | 01 → 04 | ~11 |
| 23:46 | Created .github/community/00-overview.md | — | ~2012 |
| 23:46 | Created .github/community/01-decisions.md | — | ~2128 |
| 23:47 | Created .github/community/02-conventions.md | — | ~1478 |
| 23:47 | Edited .github/community/README.md | 4→7 lines | ~300 |
| 23:47 | Edited ROADMAPS.md | 4→7 lines | ~566 |
| 23:47 | Edited .claude/rules/documentation.md | 5→5 lines | ~91 |
| 23:48 | Edited .claude/CHANGELOG.md | expanded (+49 lines) | ~676 |
| 23:25 | Created bootstrapping roadmaps 00-overview/01-decisions/02-conventions (banlieue pattern); renumbered release-diff 00→03 and dep-matrix 01→04 incl. H1s; rebuilt ROADMAPS.md + community README tables; updated documentation.md filename examples; changelog entry records old→new mapping | .github/community/*, ROADMAPS.md, .claude/rules/documentation.md, .claude/CHANGELOG.md | numbering 00–04 contiguous; link-check + PII/infra sweeps clean | ~9000 |
<<<<<<< HEAD
=======
| 00:13 | Edited .claude/CHANGELOG.md | modified 0285() | ~256 |
| 00:15 | Diagnosed PR #164 CI failure (RUSTSEC-2026-0285, rustls 0.23.44 via hyper-rustls); cargo update -p rustls → 0.23.45; cargo deny advisories ok; changelog + buglog entries | Cargo.lock, .claude/CHANGELOG.md, .wolf/buglog.json | fix ready to commit on chore/bundle-dependabot-2026-09-26 | ~3000 |
>>>>>>> 3b3348d (chore(deps): bundle six open Dependabot updates)
| 14:17 | Created .claude/rules/testing.md | — | ~1440 |
| 14:17 | Created .claude/rules/threat-modeling.md | — | ~1490 |
| 14:18 | Created .claude/rules/rust-style.md | — | ~1566 |
| 14:18 | Created .claude/rules/github-workflows.md | — | ~1035 |
| 14:20 | Edited .claude/rules/architecture-driven-development.md | 3→3 lines | ~19 |
| 14:20 | Edited .claude/rules/architecture-driven-development.md | expanded (+9 lines) | ~190 |
| 14:20 | Edited .claude/rules/architecture-driven-development.md | 2→4 lines | ~70 |
| 14:20 | Edited .claude/rules/architecture-driven-development.md | 3→4 lines | ~79 |
| 14:21 | Edited .claude/CHANGELOG.md | expanded (+52 lines) | ~730 |
| 14:21 | Edited .claude/rules/testing.md | 2→2 lines | ~35 |
| 14:21 | Edited .claude/CHANGELOG.md | 2→2 lines | ~39 |
| 01:10 | Imported banlieue's four missing rules adapted to 5-spot (testing w/ #[path] idiom + tier principles, threat-modeling w/ §-remap + v1.1 motivation, rust-style w/ ScheduledMachine examples, github-workflows w/ 6 in-use firestoned actions); ADD cycle extended to end in threat-model pass; changelog entry | .claude/rules/{testing,threat-modeling,rust-style,github-workflows}.md, .claude/rules/architecture-driven-development.md, .claude/CHANGELOG.md | skills/commands were already identical across repos; sweep clean | ~12000 |
| 14:28 | Edited Cargo.toml | 7→4 lines | ~66 |
| 14:28 | Edited Cargo.toml | 6→10 lines | ~89 |
| 14:31 | Edited .github/community/04-dependency-internalization-matrix.md | expanded (+7 lines) | ~263 |
| 14:31 | Edited ROADMAPS.md | "regex" → "hyper" | ~114 |
| 14:31 | Edited .github/community/README.md | inline fix | ~83 |
| 14:31 | Edited .github/community/01-decisions.md | inline fix | ~64 |
| 14:31 | Edited .claude/CHANGELOG.md | expanded (+35 lines) | ~454 |
| 01:35 | Closed roadmap 04: removed hyper+tower from Cargo.toml, moved http-body-util to dev-deps (runtime deps 25→22); verified check/fmt/clippy/test (708 passed); flipped 04 header + ROADMAPS.md + community README rows to ✅; struck O-001 in 01-decisions | Cargo.toml, Cargo.lock, .github/community/{04-dependency-internalization-matrix,01-decisions,README}.md, ROADMAPS.md, .claude/CHANGELOG.md | hyper/tower now kube-client transitives only | ~6000 |
| 22:15 | Edited .github/dependabot.yml | expanded (+7 lines) | ~174 |
| 22:16 | Edited .claude/CHANGELOG.md | expanded (+30 lines) | ~344 |
| 09:45 | Fixed dependabot PRs: diagnosed CodeQL init/analyze 4.38.0-vs-4.38.1 mismatch on PRs 173/174 (172 passed, unpaired); re-pinned all 6 codeql-action refs to 4.38.1 across 4 workflows; added github/codeql-action/* to actions-routine group (bare pattern never matched sub-actions); changelog entry | .github/workflows/{codeql,sast,scorecard,build}.yaml, .github/dependabot.yml, .claude/CHANGELOG.md | PRs 170/171 green, leave for auto-merge; 172-174 to be superseded | ~5000 |
| 06:55 | Edited .github/scripts/admission-deny.bats | 4→9 lines | ~125 |
| 06:56 | Edited .claude/CHANGELOG.md | expanded (+27 lines) | ~314 |
| 12:15 | PR 175 kind job round four: awaited policy denial aborted errexit'd setup_file at the bare out=$(patch_annotation) assignment; added || true (load-bearing comment); bats parses 5 tests | .github/scripts/admission-deny.bats, .claude/CHANGELOG.md, .wolf/buglog.json | previous fix (stderr/can-i) confirmed working — setup now reaches the probe loop | ~2500 |
| 07:55 | Edited .github/scripts/admission-deny.bats | modified patch_annotation() | ~191 |
| 07:55 | Edited .claude/CHANGELOG.md | expanded (+30 lines) | ~358 |
| 13:05 | PR 175 round five: patch_annotation broke its own -p JSON on quoted values (kubectl parse error, no request sent) — 4/5 tests failed while quote-free values passed; escape quotes in helper; validated escaped/unescaped with json.load; bats parses 5 | .github/scripts/admission-deny.bats, .claude/CHANGELOG.md, .wolf/buglog.json | setup fully green since round four; this was the first run where tests executed | ~3000 |

## Session: 2026-09-30 16:05

| Time | Action | File(s) | Outcome | ~Tokens |
|------|--------|---------|---------|--------|
| 16:06 | Session end: 43 writes across 21 files (no-pii.md, settings.local.json, settings.json, CLAUDE.md, CHANGELOG.md) | 1 reads | ~25474 tok |

## Session: 2026-09-30 16:06

| Time | Action | File(s) | Outcome | ~Tokens |
|------|--------|---------|---------|--------|
| 20:03 | Edited docs/adr/0011-schedule-gated-capacity-separate-controller.md | 2→3 lines | ~35 |
| 20:04 | Edited docs/adr/0011-schedule-gated-capacity-separate-controller.md | expanded (+43 lines) | ~678 |
| 20:04 | Edited docs/adr/0011-schedule-gated-capacity-separate-controller.md | expanded (+23 lines) | ~455 |
| 20:04 | Edited docs/adr/0011-schedule-gated-capacity-separate-controller.md | expanded (+17 lines) | ~307 |
| 20:04 | Edited docs/adr/0011-schedule-gated-capacity-separate-controller.md | expanded (+8 lines) | ~142 |
| 20:05 | Edited docs/adr/0011-schedule-gated-capacity-separate-controller.md | expanded (+12 lines) | ~282 |
| 20:16 | Created examples/scheduledcapacity.yaml | — | ~1543 |
| 11:31 | Created src/leader.rs | — | ~1558 |
| 11:31 | Created src/leader_tests.rs | — | ~1157 |
| 11:35 | Created src/reconcilers/capacity_path_tests.rs | — | ~3066 |
| 11:36 | Created src/reconcilers/capacity_path.rs | — | ~2375 |
| 11:38 | Created src/reconcilers/capacity_decision.rs | — | ~3527 |
| 11:39 | Created src/reconcilers/capacity_decision_tests.rs | — | ~5819 |
| 11:41 | Created src/reconcilers/scheduled_capacity.rs | — | ~7146 |
| 11:43 | Created src/reconcilers/scheduled_capacity_tests.rs | — | ~4216 |
| 11:44 | Created src/bin/scheduled_capacity_controller.rs | — | ~1451 |
| 11:44 | Created src/bin/scheduled_capacity_controller_tests.rs | — | ~1015 |
| 23:01 | Session end: 17 writes across 12 files (0011-schedule-gated-capacity-separate-controller.md, scheduledcapacity.yaml, leader.rs, leader_tests.rs, capacity_path_tests.rs) | 29 reads | ~46021 tok |

## Session: 2026-10-09 23:01

| Time | Action | File(s) | Outcome | ~Tokens |
|------|--------|---------|---------|--------|
| 01:22 | Created docs/adr/0017-machine-reference-shape-follows-capi-contract.md | — | ~1742 |
| 01:23 | Created docs/src/operations/worker-resource-confinement.md | — | ~3144 |
| 01:24 | Created .github/community/06-k0smotron-e2e-and-worker-confinement.md | — | ~2433 |
| 08:37 | Created .cargo/config.toml | — | ~415 |
| 08:49 | Created docs/src/operations/worker-resource-confinement.md | — | ~6594 |
| 10:25 | Closed F-3: removed the cargo linker pins, moved linker choice into the Makefile (verified native + cross); expanded the cgroup confinement guide to the full control surface | .cargo/config.toml, Makefile, docs/src/operations/worker-resource-confinement.md, docs/src/development/building.md, .github/community/06-*.md, ROADMAPS.md, .claude/CHANGELOG.md | gates green: fmt/clippy/860 tests, crds+crddoc byte-identical, mkdocs --strict clean | ~9000 |
| 23:38 | Created docs/src/guides/confining-a-worker-with-cgroups.md | — | ~7038 |
| 15:22 | Created src/yaml.rs | — | ~781 |
| 15:23 | Created src/yaml_tests.rs | — | ~1357 |
| 15:34 | Created docs/adr/0018-replace-serde-yaml-with-serde-saphyr.md | — | ~1538 |
| 20:26 | Created docs/adr/0019-k0smotron-end-to-end-tier-in-kind.md | — | ~1920 |
| 20:28 | Created .github/scripts/e2e-k0smotron.sh | — | ~2843 |
| 20:28 | Created .github/scripts/k0smotron-e2e.bats | — | ~1973 |
| 20:30 | Created .github/scripts/validate-examples.py | — | ~2590 |
| 21:03 | Created .github/workflows/e2e.yaml | — | ~949 |
| 16:54 | Created docs/adr/0020-one-binary-one-image-with-subcommands.md | — | ~1693 |
| 16:59 | Created src/cmd/nsenter.rs | — | ~2030 |
| 17:10 | Created docs/src/reference/cli.md | — | ~2260 |
