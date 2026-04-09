# 5 Spot Machine Scheduler Contribution and Governance Policies

This document describes the contribution process and governance policies of the FINOS 5 Spot Machine Scheduler project. The project is also governed by the [Linux Foundation Antitrust Policy](https://www.linuxfoundation.org/antitrust-policy/), and the FINOS [IP Policy]( https://community.finos.org/governance-docs/IP-policy.pdf), [Code of Conduct](https://community.finos.org/docs/governance/code-of-conduct), [Collaborative Principles](https://community.finos.org/docs/governance/collaborative-principles/), and [Meeting Procedures](https://community.finos.org/docs/governance/meeting-procedures/).

# Contributing to 5 Spot Machine Scheduler

5 Spot Machine Scheduler is [Apache 2.0 licensed](LICENSE) and accepts contributions via git pull requests. 

## Developer Certificate of Origin (DCO)

All contributions to this project must be accompanied by a **Developer Certificate of Origin** sign-off. This is a FINOS requirement that certifies you have the right to submit the contribution under the project's license. 

>[!IMPORTANT]
>**All commits must be signed with a DCO signature to avoid being flagged by the DCO Bot.**
>The DCO check will fail if even a single commit in your branch is missing the Signed-off-by line.

This sign-off means you agree the commit satisfies the [Developer Certificate of Origin (DCO).](https://developercertificate.org/)

> Pull requests that contain unsigned commits will not be merged.

This means that your commit log message must contain a line that looks like the following one, with your actual name and email address:

```
Signed-off-by: John Doe <john.doe@example.com>
```

### Configuring Git to sign off 

```bash
git config --global user.name  "Your Name"
git config --global user.email "your.email@example.com"
# Then use: git commit -s -m "your message"
```

>[!NOTE]
>The email must match with the email linked to your GitHub profile (and must be set to public, see https://github.com/settings/emails to configure your email and for special configurations if keeping your email private).

Adding the -s flag to your git commit `git commit -s` will add that line automatically. You can also add it manually as part of your commit log message or add it afterwards with git commit --amend -s.

To avoid having to remember the -s flag every time, you can configure Git to sign every commit automatically on your workstation (make sure you configured your name and email correctly):

```
git config --global format.signoff true
```

### How to fix a failing DCO check

If the DCO bot flags your PR, you don't need to start over or re-open the PR. It is likely one or more of the commits inside your PR was not properly signed. You can bulk-sign your previous commits using an interactive rebase:

1. Start the rebase (Replace 'X' with the number of commits in your PR)

  ```git rebase -i HEAD~X --signoff```

2. An editor will open listing your commits. Simply save and close it without making changes.

3. Force push the corrected commits to your branch:

  ```git push --force```

### Helpful DCO Resources
- [Git Tools - Signing Your Work](https://git-scm.com/book/en/v2/Git-Tools-Signing-Your-Work)
- [Signing commits
](https://docs.github.com/en/github/authenticating-to-github/signing-commits)

## Governance

### Roles

The project community consists of Contributors and Maintainers:
* A **Contributor** is anyone who submits a contribution to the project. (Contributions may include code, issues, comments, documentation, media, or any combination of the above.)
* A **Maintainer** is a Contributor who, by virtue of their contribution history, has been given write access to project repositories and may merge approved contributions.
* The **Lead Maintainer** is the project's interface with the FINOS team and Board. They are responsible for approving [quarterly project reports](https://community.finos.org/docs/governance/#project-governing-board-reporting) and communicating on behalf of the project. The Lead Maintainer is elected by a vote of the Maintainers. 

### Contribution Rules

Anyone is welcome to submit a contribution to the project. The rules below apply to all contributions. (The key words "MUST", "SHALL", "SHOULD", "MAY", etc. in this document are to be interpreted as described in [IETF RFC 2119](https://www.ietf.org/rfc/rfc2119.txt).)

* All contributions MUST be submitted as pull requests, including contributions by Maintainers.
* All pull requests SHOULD be reviewed by a Maintainer (other than the Contributor) before being merged.
* Pull requests for non-trivial contributions SHOULD remain open for a review period sufficient to give all Maintainers a sufficient opportunity to review and comment on them.
* After the review period, if no Maintainer has an objection to the pull request, any Maintainer MAY merge it.
* If any Maintainer objects to a pull request, the Maintainers SHOULD try to come to consensus through discussion. If not consensus can be reached, any Maintainer MAY call for a vote on the contribution.

### Maintainer Voting

The Maintainers MAY hold votes only when they are unable to reach consensus on an issue. Any Maintainer MAY call a vote on a contested issue, after which Maintainers SHALL have 36 hours to register their votes. Votes SHALL take the form of "+1" (agree), "-1" (disagree), "+0" (abstain). Issues SHALL be decided by the majority of votes cast. If there is only one Maintainer, they SHALL decide any issue otherwise requiring a Maintainer vote. If a vote is tied, the Lead Maintainer MAY cast an additional tie-breaker vote.

The Maintainers SHALL decide the following matters by consensus or, if necessary, a vote:
* Contested pull requests
* Election and removal of the Lead Maintainer
* Election and removal of Maintainers

All Maintainer votes MUST be carried out transparently, with all discussion and voting occurring in public, either:
* in comments associated with the relevant issue or pull request, if applicable;
* on the project mailing list or other official public communication channel; or
* during a regular, minuted project meeting.

### Maintainer Qualifications

Any Contributor who has made a substantial contribution to the project MAY apply (or be nominated) to become a Maintainer. The existing Maintainers SHALL decide whether to approve the nomination according to the Maintainer Voting process above.

### Changes to this Document

This document MAY be amended by a vote of the Maintainers according to the Maintainer Voting process above.