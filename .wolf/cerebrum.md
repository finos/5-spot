---
description: learned preferences, project conventions, and Do-Not-Repeat rules
budget_tokens: 2000
---
# Cerebrum

> OpenWolf's learning memory. Updated as the AI learns from interactions.
> Format follows ~/dev/banlieue/.wolf/cerebrum.md: every entry is dated, leads
> with the generalizable rule in bold, then gives the concrete evidence, why it
> was invisible, and the fix. Cross-reference buglog ids where one exists.
> Last updated: 2026-10-03

## User Preferences

<!-- How the user likes things done. Code style, tools, patterns, communication. -->

- **[2026-10-03] Build and test on the designated remote build host, not on
  the mac and not on the VM hosts.** `~/dev/CLAUDE.md` names which host that
  is and was changed on 2026-10-02; the copy embedded in a session's system
  prompt can be stale, and Erick had to say "re-read" after I used the wrong
  one. **Re-read `~/dev/CLAUDE.md` from disk before acting on any
  remote-host fact.** Linux-only crates genuinely do not compile on macOS, so
  this is not optional for them. Host names, addresses and kubeconfig paths
  stay out of this file: it is tracked and published (`rules/no-real-infrastructure.md`).
- **[2026-10-03] Package metadata carries Erick's personal identity, not his
  employer's.** `Cargo.toml` and `docs/pyproject.toml` use
  Erick's personal address (the same identity as his git authorship) and
  `repository = "https://github.com/finos/5-spot"`. They previously held an
  employer address and a `github.com/RBC/5-spot` URL, which was the only
  `RBC/` reference against 75 `finos/` ones. The maintainer-identity carve-out
  in `rules/no-pii.md` covers git authorship, `Cargo.toml` and
  `SECURITY.md` only, so the address itself is not repeated here.
  **`MAINTAINERS.md` is different and keeps its employer addresses**, Erick's
  included: Erick said to leave his employer email there (2026-10-04). A
  maintainer contact in a FINOS project's `MAINTAINERS.md` is deliberately
  published identity, which is
  exactly what the carve-out is for, and the other two entries are other
  people's and never ours to change. Settled: do not raise this again.
- **[2026-10-03] Erick commits himself; hand back the exact command.** He asked
  twice for "the git commit commands" rather than having them run. Finish a
  piece of work by printing ready-to-paste `git commit -s -S -m "..."` with a
  real message. Read-only git is free; `checkout -b`, `push`, `rebase` and
  `gh pr create` are handed back. Same rule as banlieue.

## Key Learnings

<!-- Project conventions and surprises that are not obvious from the code. -->

- **Project:** `five_spot`, a FINOS-incubating Kubernetes controller for
  time-based scheduling of physical machines in k0smotron/CAPI clusters.
- **[2026-10-03] Doc comments in `src/crd.rs` ship to users.** They land in the
  generated `deploy/crds/*.yaml` and therefore in `kubectl explain`. A `///`
  comment explaining a Rust trait derive put trait rationale into the public
  API schema of all four CRDs. Implementation rationale on a CRD type belongs
  in a `//` comment; `///` is user-facing API documentation. Noticed only
  because `make crds` dirtied three unrelated CRD files.
- **[2026-10-03] Cargo auto-discovers `src/bin/*.rs` as binaries.** A sibling
  `src/bin/foo_tests.rs` is therefore compiled as its own binary and fails with
  "`main` function not found". That is why `rules/testing.md`'s `_tests.rs`
  convention had never been applied to any bin target. Fixed with
  `autobins = false` in `Cargo.toml`, which is safe because every binary is
  already declared in an explicit `[[bin]]`.
- **[2026-10-03] `kubectl auth can-i` reports a false `no` for a CRD
  subresource.** `kubectl auth can-i patch scheduledcapacities/status` answered
  `no` while the permission was genuinely granted; it cannot resolve the
  `resource/subresource` shorthand for a CRD. A `SubjectAccessReview` posted to
  `/apis/authorization.k8s.io/v1/subjectaccessreviews` with explicit
  `group`/`resource`/`subresource` is authoritative and answered `allowed: true`
  with the granting ClusterRoleBinding named. Use it for any RBAC assertion
  that matters.
- **[2026-10-03] A `;` inside a quoted Mermaid node label is fine.** This
  corrects the stored `project_mermaid_label_escaping` memory, which implied
  `"`, `<`, `>` and `;` all break diagrams. 13 shipped labels in
  `docs/src/architecture/flows.md` contain `;`, and `->` appears throughout.
  Only `"` needs escaping, as `#quot;`. CALM flow *transition descriptions* do
  become Mermaid labels (`t1["1. ..."]`), so they are in scope; node
  `description` fields are not rendered.
- **[2026-10-03] `docs/src/architecture/{flows,system}.md` are gitignored.**
  `make calm-diagrams` regenerates them at docs-build time, so a CALM change
  shows no diff there and needs no commit. Only
  `docs/architecture/calm/architecture.json` is tracked.

## Do-Not-Repeat

<!-- Mistakes made and corrected. Each entry prevents the same mistake recurring. -->
<!-- Format: [YYYY-MM-DD] Description of what went wrong and what to do instead. -->

- **[2026-10-08] Check a design's premise against the other project's code
  before building apparatus to work around it.** ADR 0011 refused to let 5-Spot
  create the capacity object because deleting one "would pull the rug on claim
  holders", and built a validated field path, a confused-deputy trust boundary
  and a merge-patch-over-SSA decision on top of that premise. The consumer had
  already solved it, in code, by recorded decision: it re-parents a claimed
  member's `ownerReferences` to the claim at bind time. One grep of
  `~/dev/banlieue` would have saved the whole apparatus. When an ADR's central
  argument is a claim about a sibling project's behaviour, read that project.

- **[2026-10-08] A `grep` for a Rust field name misses the same field written as
  a JSON literal key.** Searching `owner_references` across banlieue's
  controller found only the member-creation site and I nearly concluded that
  claims do not re-parent. The re-parenting is a `json!({"ownerReferences": ...})`
  merge patch. Search both spellings (`owner_references` **and**
  `ownerReferences`) when asking whether a Kubernetes field is ever written.

- **[2026-10-08] "It is only v1alpha1" is not a licence to drop a field without
  checking whether it shipped.** I wrote that `spec.targetRef` was being
  withdrawn "before it ever shipped". It is in `v0.3.2`, tagged three days
  earlier. `git ls-tree -r --name-only <latest-tag>` answers this in one command.
  State a breaking change as one, with the recreate instruction, rather than
  arguing it away.

- **[2026-10-03] Never `rsync --delete` onto a remote path without checking
  whether it is already occupied.** `~/dev/CLAUDE.md` gives a conventional
  `builds/<repo>/` sync target, and I used it, straight onto Erick's live
  working copy.
  It held an uncommitted edit to `scripts/dev-oidc-k0s.sh` that he was
  iterating on *during* the session. A later push overwrote it (confirmed by
  md5 matching HEAD exactly), and it survived only because I had incidentally
  saved a `git diff` to a patch first. `--delete` also removes
  destination-only files, so anything else that directory held is
  unrecoverable. Use a dedicated directory (`builds/<repo>-<topic>/`), and
  check `ls`/`git status` on the destination before the first sync.
- **[2026-10-03] A controller that logs nothing on a successful reconcile
  cannot be debugged.** My `ScheduledCapacity` reconciler logged only on write
  and only on error, so "nothing needed doing" and "nothing ran" looked
  identical. I concluded from the silence that the watch was dead, told Erick
  so, and spent several steps on the wrong problem: it had been reconciling
  correctly the whole time. `main.rs` logs every outcome and that is why it is
  debuggable. Log every reconcile completion with the object and the next
  action. (bug-005)
- **[2026-10-03] `Condition::new()` stamps `lastTransitionTime` with `now`, so
  an unconditional status patch is an infinite reconcile loop.** The computed
  status never equals the stored one, every reconcile writes, the write
  re-triggers the controller's own watch. Measured at ~50 `resourceVersion`
  bumps/second on an idle object, with a single handback timeout counted 100
  times. The fix needs BOTH halves: preserve each condition's timestamp unless
  its `status` actually changed (matched by `type`, not position), AND compare
  the computed status against the stored one and skip the patch when equal.
  `providers/time_based.rs::compute_status` and
  `scheduled_machine.rs` had both already documented this exact trap; I
  reintroduced it anyway. (bug-005)
- **[2026-10-03] A hand-assembled status JSON must be checked against the type,
  not against memory.** Rewriting `patch_status` silently dropped the
  `targetRef` and `spotSchedule` keys. Because the no-op guard deserialises the
  computed status to compare it, two missing keys made `desired != stored`
  permanently, which **disabled the guard the same commit had just added**, with
  no failing test and no visible symptom (the API server absorbed the identical
  patch as a no-op, so even `resourceVersion` looked stable). The guard is now
  pinned by a test that derives the expected key set from a fully-populated
  `ScheduledCapacityStatus` rather than a hand-written list. (bug-005)
- **[2026-10-03] A JSON merge patch that OMITS a key cannot clear a field.**
  The stored value survives, so an optional status field set once is set
  forever: a completed handback would keep advertising the deadline it had
  already met. Emit every key the controller owns on every write, with an
  explicit `null` for the ones that should be absent.
- **[2026-10-03] A metric that means "this happened" must count transitions,
  not reconciles.** `handback_timeouts_total` was incremented once per
  reconcile while the object sat in the timed-out phase, so one timeout read as
  100. A counter that is trusted and wrong is worse than no counter. Gate on
  "did this reconcile *enter* the phase".
- **[2026-10-03] Do not assert on a regex pattern's own metacharacters.** I
  wrote a test requiring `CAPACITY_PATH_PATTERN` to contain no `[`, `]` or `*`,
  which fails immediately because those are the character-class and quantifier
  syntax. Behavioural rejection needs a regex engine; without one, assert the
  properties that are checkable (anchoring, no unescaped `.`) and prove the
  rejections against the real API server instead.
- **[2026-10-03] A test that early-returns when its fixture is missing reports
  success while proving nothing.** I wrote `let Ok(client) = ... else { return }`
  for a test needing a cluster. `rules/testing.md` names this exact
  anti-pattern. The pre-existing tests in the same file use a `tower_test` mock
  client, which cannot skip. Use the mock, or fail loudly naming what is
  missing.
- **[2026-10-03] A ValidatingAdmissionPolicy cannot correlate across objects.**
  ADR 0011 was written specifying a VAP to reject a host governed by both
  `ScheduledMachine` and `ScheduledCapacity`. A VAP evaluates one request
  against its own object and its bound `paramRef` only, so it can never answer
  "is any other object already governing this node?". Before specifying
  admission enforcement, check the policy can actually see what the rule needs.
- **[2026-09-26] Tracked `.claude/settings.json` carried absolute
  `/Users/<name>/...` hook paths.** Never put home directories, usernames, or
  any PII in tracked files: use `$CLAUDE_PROJECT_DIR` in hook commands, and put
  anything machine-local in gitignored `.claude/settings.local.json`. Full
  rule: `.claude/rules/no-pii.md`.

## Decision Log

<!-- Significant technical decisions with rationale. Why X was chosen over Y. -->

- **[2026-10-03] ADR 0011 handback: on timeout, hold and report; never force.**
  Erick chose this over forcing capacity to zero or a per-object
  `onTimeout: Hold|Force` knob. A missed handover is visible and recoverable
  while a killed agent task is neither, and ADR-0007's additive-only rule would
  make the enum permanent. Zero is still written *first*, immediately on
  deactivation: the gated field is a warm-pool target, so zero stops
  replenishment while claimed work finishes on its own, and holding the field
  above zero until the consumer reported drained would be circular. "Hold"
  therefore means "do not escalate further", not "do not write zero". My
  original question to Erick illustrated this wrongly (it showed the value
  staying at 10) and the ADR now states it explicitly.
- **[2026-10-03] ADR 0011 decision 7 is controller-side, not a VAP.** Optional
  `spec.nodeName`, compared against every `ScheduledMachine.status.nodeRef.name`
  in the namespace, failing closed with a `HostGovernanceConflict` condition.
  Chosen because a VAP cannot do the cross-object lookup the rule needs. Known
  gap, recorded and still open: a conflict appearing *after* a value was
  written leaves that value in place, because "refuse to write" also refuses to
  write zero.
- **[2026-10-03] Actuation is a JSON merge patch, not server-side apply.** SSA
  would make 5-Spot a permanent co-owner of the consumer's capacity field and
  fight the consumer's own GitOps field manager on every reconcile. A merge
  patch writes the value and leaves ownership alone. (Contrast banlieue
  ADR-0087, where SSA with a *per-writer* field manager is the right answer
  because the writer genuinely owns its row.)
- **[2026-10-03] The segment cap lives in the regex, not in a CEL rule.**
  `self.split('.').size() <= 8` depends on CEL's extended strings library,
  while a structural-schema `pattern` is enforced by any API server that can
  serve the CRD at all. `CAPACITY_PATH_MAX_SEGMENTS` and the pattern are tied
  together by a drift-guard test, since the reconciler enforces one and
  admission the other.
- **[2026-10-03] Three commits, not the five ADD phases.**
  `src/reconcilers/mod.rs` and `Cargo.toml` each carry changes from several
  phases, and splitting them needs `git add -p`, which is interactive and
  unavailable. Five commits would include two that do not compile; three each
  build and test green.
