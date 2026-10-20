---
title: "The evidence check"
description: "AgentWorth / Evidence on a pull request: what CI re-derived, what your machine signed, and what was only claimed. Transcripts never leave your laptop."
---

A reviewer sees a diff. They do not see that the session behind it never ran
the tests, or that it said "all tests pass" with no exit code anywhere. That
evidence is on the author's laptop, in the index, and it should stay there.

The evidence check moves only what a reviewer can use: a small signed **Work
Receipt** per push, and a check on the pull request that says which parts of
it this runner verified for itself.

## What travels

When you push, the pre-push hook runs `archie repo receipt create --push`. It
attributes the commits you are pushing to the sessions that wrote them (the same
anchored-blame join `repo suspect` uses), takes each session's rungs, signs the
result with a local ed25519 key, and pushes it as a git note on
`refs/notes/agentworth`.

A receipt holds hashes, enums, counts, and timestamps. No prompts, no
transcripts, no file contents, no absolute paths. Every string goes through the
publication redactor, and the build fails closed if one would leak. Nothing is
signed for a repository until you run `archie repo receipt init --write` there,
which records your consent; until then `create` only prints a preview.

## What the check shows

The Action fetches the notes, checks the signature against the author's GitHub
signing keys, and sorts every rung into three columns:

| Column | Means |
| :-- | :-- |
| Verified here | This runner re-derived it: the commit is in this PR (rung 4), the claimed files are in its diff (rung 2), CI is green on the head commit (rung 5). |
| Attested locally (signed) | The author's machine says a test or build passed (rung 3), and signed it. Not re-runnable here; agent-chosen tests are flagged. |
| Claimed only | A "done" with nothing behind it, an invalid signature, or a commit outside the PR. |

A commit with no receipt says "no receipt covers this commit". Unknown, never
clean.

The conclusion is `neutral` by default. It is `success` only when every commit
carries a receipt signed by a key linked to GitHub and head CI is green, and
`failure` only when the repository opts in with `fail-on`. The job itself never
fails.

## Set it up

```bash
npx -y agentworth@latest repo receipt init --write
git add .github/workflows/agentworth.yml && git commit -m "Add AgentWorth evidence check" && git push
```

Then open a pull request. To have the check say "signed by @you", run
`archie repo receipt key --github` and paste the `gh ssh-key add --type signing`
line it prints.

On a fork pull request GitHub hands the workflow a read-only token, so the
Action writes the table to the job summary instead of a check. A `workflow_run`
workflow, which runs after CI in the base repository's context, gets the check
on forks too; the README has the YAML.

Before pushing, an agent can call the `repo_receipt` MCP tool to see the receipt
the next push would sign and the rungs the check would show, and run the tests
first.
