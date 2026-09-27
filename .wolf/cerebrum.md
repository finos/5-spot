---
description: learned preferences, project conventions, and Do-Not-Repeat rules
budget_tokens: 2000
---
# Cerebrum

> OpenWolf's learning memory. Updated automatically as the AI learns from interactions.
> Do not edit manually unless correcting an error.
> Last updated: 2026-09-27

## User Preferences

<!-- How the user likes things done. Code style, tools, patterns, communication. -->

## Key Learnings

- **Project:** five_spot
- **Description:** [![FINOS - Incubating](https://cdn.jsdelivr.net/gh/finos/contrib-toolbox@master/images/badge-incubating.svg)](https://community.finos.org/docs/governance/Software-Projects/stages/incubating)

## Do-Not-Repeat

<!-- Mistakes made and corrected. Each entry prevents the same mistake recurring. -->
<!-- Format: [YYYY-MM-DD] Description of what went wrong and what to do instead. -->

- [2026-09-26] Tracked `.claude/settings.json` carried absolute `/Users/<name>/...` hook paths. Never put home directories, usernames, or any PII in tracked files — use `$CLAUDE_PROJECT_DIR` in hook commands, and put anything machine-local in gitignored `.claude/settings.local.json`. Full rule: `.claude/rules/no-pii.md`.

## Decision Log

<!-- Significant technical decisions with rationale. Why X was chosen over Y. -->
