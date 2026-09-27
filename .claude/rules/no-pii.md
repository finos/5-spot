# Never Commit Home Directories or PII

> **5-Spot is a public repository** (`github.com/finos/5-spot`) developed in a
> **regulated banking environment**. Anything committed here is published,
> indexed, and permanent. This rule is the companion to
> [`no-real-infrastructure.md`](no-real-infrastructure.md): that rule protects
> the *infrastructure*; this one protects the *people*.

A home-directory path is not "just a path". `/Users/<name>/dev/5-spot` names a
real login, reveals the operating system, and hands an attacker one half of a
credential pair for every service where the person reused that username. An
email address or real name in a stray fixture ties a human being to this
codebase forever — a later commit that removes it does not un-publish it.

## The rule

**Never write a user home directory, a real username, or any personally
identifying information into any tracked file.** This covers code, tests,
fixtures, snapshot output, docs, ADRs, roadmaps, examples, scripts, Makefiles,
CI workflows, comments, commit messages, changelog entries, and **tool
configuration** — `.claude/settings.json`, editor configs, hook definitions —
with no exception for "it's only local tooling config".

In scope (this list is **explicitly not exhaustive**):

- **Home directories and personal absolute paths**: `/Users/<name>/…`,
  `/home/<name>/…`, `C:\Users\<name>\…` — including inside pasted terminal
  output, panic backtraces, `cargo test` output, coverage reports, logs, and
  screenshots quoted into docs.
- **Usernames and logins**: system accounts, SSH users, forge handles, chat
  handles — anything that names a real person's account.
- **Personal identity**: real names, personal email addresses, phone numbers,
  physical addresses, photos.
- **Identifiers tied to a person**: employee IDs, customer or account numbers,
  internal ticket references naming individuals, calendar/meeting links.
- **Machine identity**: real IP addresses, hostnames, MAC addresses, serial
  numbers, device names. (IPs and hostnames are already banned by
  [`no-real-infrastructure.md`](no-real-infrastructure.md); they are repeated
  here because they identify people's machines as well as the estate.)

**The list is open-ended by design.** If a value identifies a real person, a
real account, or a real machine — in any way this list did not anticipate — it
is in scope. If you are unsure whether a value is identifying, **assume it is**
and replace it.

**If you believe an identifying value is genuinely necessary, ASK FIRST.**
Same escape hatch as the infrastructure rule, and it is the only one — do not
resolve the question yourself by committing the value.

## Placeholders to use

| Kind | Use | Never use |
| --- | --- | --- |
| Home directory | `~`, `$HOME`, or `/home/user` in prose/examples | any real `/Users/<name>` or `/home/<name>` path |
| Project path in tool config | `$CLAUDE_PROJECT_DIR`, or a repo-relative path | an absolute path under a real home directory |
| Username / login | `admin`, `svc-5spot`, `user` | a real login or handle |
| Person in an example | `Jane Maintainer` | a real name that isn't deliberate maintainer identity (below) |
| Email address | `user@example.com`, `maintainer@example.com` | a personal address |
| IP / hostname | per the table in [`no-real-infrastructure.md`](no-real-infrastructure.md) | any real address or host |

## Paths come from the environment, not from the file

The pattern is the same as the Makefile's empty-default mirrors: a real,
machine-local value is supplied *at runtime*, never baked into a tracked file.

- **`.claude/settings.json` is tracked** — hook commands and any other paths in
  it must use `$CLAUDE_PROJECT_DIR` (Claude Code expands it to the repo root),
  never an absolute path:

  ```json
  // ✅ GOOD — portable, no username in the repo
  { "type": "command", "command": "node \"$CLAUDE_PROJECT_DIR/.wolf/hooks/stop.js\"" }

  // ❌ BAD — a real login is now in the git history, permanently
  { "type": "command", "command": "node \"/Users/<name>/dev/5-spot/.wolf/hooks/stop.js\"" }
  ```

- **Anything that genuinely cannot avoid a machine-local absolute path** goes
  in `.claude/settings.local.json`, which is gitignored and personal. If a
  tracked config file cannot be expressed without a home directory, the fix is
  to *untrack that file*, not to commit the path.
- In shell and Rust, take the value from `$HOME` / `std::env::var` at runtime
  and show a placeholder in the docs.

## Deliberate maintainer identity is the one carve-out

Git commit authorship (from git config), `Cargo.toml` `authors` /
`repository`, and the security contact in `SECURITY.md` are identity the
maintainer chose to publish. They are not violations — and they are not a
precedent. The carve-out covers exactly those locations, only the maintainer's
own deliberate choices: never quietly change them, never extend them, and
never commit anyone *else's* identity anywhere under this heading.

## How PII actually sneaks in

Almost never as a typed literal — it arrives embedded in something copied:

- terminal output pasted into a doc, roadmap, or changelog entry (absolute
  paths in `cargo` output, panics, backtraces);
- snapshot/golden tests that capture full filesystem paths;
- generated artifacts that get committed (coverage HTML, IDE metadata,
  tool state files such as `.wolf/`'s, session logs);
- screenshots showing a terminal prompt, window title, or browser profile;
- kubeconfigs, `kubectl` output, or cluster dumps used as fixtures.

Before quoting any command output into a tracked file, re-read it as an
attacker would.

## Before finishing any task

Grep your own diff — one command, alongside the infrastructure sweep:

```sh
# Home-directory / PII sweep over what you are about to commit.
git diff --cached -U0 \
  | rg -in '/(Users|home)/[A-Za-z0-9._-]+|C:\\Users|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
  | rg -v 'example\.com|/home/user\b|users\.noreply\.github\.com'
```

Widen it to the whole tree when touching docs, examples, or tool config in
bulk:

```sh
rg -in '/(Users|home)/[A-Za-z0-9._-]+|C:\\Users' \
  --glob '!target/**' --glob '!docs/book/**' . \
  | rg -v '/home/user\b'
```

Anything that names a real person, login, or machine is a finding. If you are
unsure whether a value is real, **assume it is** and replace it.
