# Never Commit Real Infrastructure or Internal References

> **5-Spot is a public repository** (`github.com/finos/5-spot`) developed in a
> **regulated banking environment**. Anything committed here is published,
> indexed, and permanent — a later commit that removes it does not un-publish
> it, and neither does a force-push once a fork or a mirror has seen it.

This is not a style preference. A real hostname in a public repo is a free
reconnaissance gift: it names a host, implies what runs on it, and usually
reveals the naming scheme for every other host beside it. An internal
organisational reference is worse — it also tells a reader *whose* estate they
are looking at.

## The rule

**Never write a real hostname, IP address, registry path, username, account
identifier, or internal organisational reference into any tracked file.** This
covers code, tests, docs, ADRs, roadmaps, examples, scripts, Makefiles,
comments, commit messages, and changelog entries — with no exception for "it's
only a doc comment" or "it's an example in an `#[ignore]`d test".

Specifically, never commit:

- Secrets, tokens, credentials, or kubeconfigs — even as examples.
- Real hostnames, IP addresses, or cluster/datacentre names.
- Internal organisational references: employer-specific org names, corporate
  domains, internal GitHub/GitLab orgs, Artifactory or other private registry
  paths, internal wiki or ticket URLs.
- Customer or transaction data in any form.

**If you believe an internal reference is genuinely necessary, ASK FIRST.**
That escape hatch is deliberate and it is the only one — do not resolve the
question yourself by committing the value.

## Placeholders to use

Use what the tree already uses. 5-Spot's convention is the RFC 2606
`example.com` family, not invented domains:

| Kind | Use | Never use |
| --- | --- | --- |
| Hostname / domain | `example.com`, `workshop.example.com`, `evil.example.com` (for the hostile party in a security example) | any real host anyone operates |
| Private registry / mirror | `artifactory.example.com`, `oss-docker-gcr.artifactory.example.com`, `mirror.example.com` | a real private registry host or repo path |
| Published image | `ghcr.io/finos/5-spot` | any internal mirror of it |
| Documentation IPv4 | `192.0.2.x`, `198.51.100.x`, `203.0.113.x` (RFC 5737) | any real routable address |
| Documentation IPv6 | `2001:db8::/32` (RFC 3849) | any real routable address |
| Private / cluster IPv4 | `10.0.0.x`, `192.168.x.x` (RFC 1918) — correct when the example is *semantically* a private network, as in a `Machine`'s `spec.address` | a real private address actually in use |
| Public DNS resolver | `192.0.2.53` | `1.1.1.1`, `8.8.8.8` — real third-party services |
| Username | `admin`, `svc-5spot` | a real login |
| Cluster name | `5spot-dev`, `workload-cluster` | a real cluster's name |

The RFC 5737 ranges are the right answer to "make up an IP": they are reserved
for documentation and guaranteed never routable. A *genuinely* random address is
worse than a reserved one — it probably belongs to somebody.

`host.docker.internal` and `localhost` / `127.0.0.1` / `0.0.0.0` are tooling
conventions, not infrastructure identifiers. They are fine.

## Getting a real value in without committing it

Real environments still need testing and air-gapped builds still need a mirror.
Take the value from the environment at runtime and document it with a
placeholder. The Makefile already works this way and is the pattern to copy —
`BASE_IMAGE`, `CHAINGUARD_BASE_IMAGE`, `PYPI_INDEX_URL` and `PUSH` all default
to **empty**, and the docs show an `example.com` value:

```make
# ✅ GOOD — empty default, the caller supplies the real mirror
#   make docker-build-amd64 BASE_IMAGE=<your-mirror>/distroless/cc-debian13:nonroot
BASE_IMAGE ?=

# ❌ BAD — a real registry is now in the git history, permanently
BASE_IMAGE ?= <a real internal registry host>/distroless/cc-debian13:nonroot
```

The same shape applies in Rust (`std::env::var`, with the `expect` message
naming a placeholder) and in shell scripts (default empty and require the
caller, or derive at runtime). Never bake a real value in as a default.

## Package metadata is the one grey area

`authors` and `repository` in `Cargo.toml` are maintainer-chosen identity
metadata rather than infrastructure — nothing there names a host you could
connect to. They are still **published**, including to crates.io if the crate is
ever released, so they are in scope for this rule's spirit: if either field
carries a corporate address or an internal forge URL, that is an internal
reference in a public repository and it needs an explicit decision, not a
default. Review them; do not quietly change someone's authorship.

## Before finishing any task

Grep your own diff. It costs one command:

```sh
# Real-infrastructure / internal-reference sweep over what you are about to commit.
# No look-around: ripgrep's default engine has none, so filter the allowed
# org with a second pass rather than a negative lookahead.
git diff --cached -U0 \
  | rg -in 'artifactory|\.corp\b|\b(?:\d{1,3}\.){3}\d{1,3}\b|github\.com/[A-Za-z0-9_-]+' \
  | rg -v 'github\.com/finos/|example\.com|127\.0\.0\.1|0\.0\.0\.0'
```

Anything that is not in the placeholder table above is a finding. Widen it to
the whole tree when touching docs or examples in bulk:

```sh
rg -in 'artifactory|\.corp\b|\b(?:\d{1,3}\.){3}\d{1,3}\b' \
  --glob '!target/**' --glob '!docs/book/**' --glob '!Cargo.lock' . \
  | rg -v 'example\.com|127\.0\.0\.1|0\.0\.0\.0'
```

If you are unsure whether a value is real, **assume it is** and replace it.
