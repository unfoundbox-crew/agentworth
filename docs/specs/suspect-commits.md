# Suspect commits

Status: **built, PR #84**, measured 2026-09-02.

Shipped as the MCP tool `suspect_commits` and the CLI `agentworth suspect`, with
`agentworth suspect --hook` printing the pre-push script. Two things changed
between this spec and what shipped; both are recorded in "What shipped that this
spec did not say" at the end.

## The one-line version

Before you push, list which commits on this branch were written by a session
that never proved anything — and hand the list to the agent, never a patch.

## The problem, stated by the person who has it

I merge work I did not watch. Subagents write most of it, and by the time I
look, the diff reads fine and the session that produced it is 20,000 events I
will never read.

I do not want another reviewer bot. Five of those exist and they all read the
diff, which is the part I can already read myself. I want to know which commits
came out of a session that was going badly, so I know where to look twice.

## Why this is the one autofix idea worth building

`market-autofix.md` surveyed roughly twenty observe-and-fix products and found
that every one keys off a dependency feed, a static rule, a PR diff, a CI
failure, or a production error. None reads how the code was written.

It also reached the honest conclusion: trajectory data should **triage, not
fix**. A trajectory says the session was sloppy. It does not say what the
sloppy code does wrong — a stack trace still writes the better patch. So the
output here is a list of session ids and a copyable prompt. Never a diff.

## The measurement

Against 93 commits on `origin/main` of this repo, joined to the local index's
`file_modifications` table (25,206 rows, 1,184 sessions, 8,310 distinct files).

The join: for each commit, take its changed paths, find blame rows whose
`file_path` ends with that path and whose `occurred_at` is within 24 hours
before the commit, then look at that session's `primary_outcome`.

| | |
| :--- | ---: |
| Commits scanned | 93 |
| Attributed to at least one indexed session | 51 |
| Flagged — an authoring session below rung 3 | 17 |
| Flag rate of attributed commits | 33.3% |

**Then the false positives.** A sample of the first 10 flagged commits, checked
by hand against the blame path that triggered each flag:

| commit | matched on | evidence path | real |
| :--- | :--- | :--- | :--- |
| bd84c85a | `docs/HANDOFF.md` | `/Users/saurabh/code/unfoundbox/agentworth/docs/HANDOFF.md` | yes |
| ca746787 | `.gitignore` | `.gitignore` | no |
| ddf62cb4 | `README.md` | `web/README.md` | no |
| 80d9d22a | `README.md` | `web/README.md` | no |
| 6f1c0478 | `AGENTS.md` | `AGENTS.md` | no |
| 1185ce4b | `README.md` | `web/README.md` | no |
| 97e9c6eb | `apps/cli/src/main.rs` | `apps/cli/src/main.rs` | no |
| c245897e | `Cargo.lock` | `Cargo.lock` | no |
| 0dcd2d80 | `.gitignore` | `.gitignore` | no |
| 5b35e4e7 | `Cargo.lock` | `Cargo.lock` | no |

**Nine of ten are false.** The cause is one line of the schema:
`file_modifications.file_path` is not always absolute.

```sql
SELECT s.adapter, COUNT(*) rows,
       SUM(fm.file_path LIKE '/%') absolute,
       SUM(fm.file_path NOT LIKE '/%') relative
FROM file_modifications fm JOIN sessions s ON s.session_id = fm.session_id
GROUP BY 1;
```

| adapter | rows | absolute | relative |
| :--- | ---: | ---: | ---: |
| claude_code | 24,841 | 24,812 | 29 |
| antigravity | 238 | 238 | 0 |
| opencode | 127 | 0 | 127 |

156 relative rows out of 25,206 — 0.6% of the table. Those 0.6% produced 16 of
the 17 flags, because a bare `Cargo.lock` suffix-matches every Rust repo on the
disk. Re-running the same join with the repo name required in the evidence
path:

| | naive | repo-anchored |
| :--- | ---: | ---: |
| Attributed | 51 | 39 |
| Flagged | 17 | 1 |
| Flag rate | 33.3% | 2.6% |

**The honest headline is 2.6%, not 33%.** One commit in 39 on this repo's main
was written by a session that never got past `artifact_changed`. That is a
believable number and a useful one. The 33% was a path bug wearing a finding's
clothes, and shipping it would have burned the feature's credibility on its
first run.

So the first rule of this tool: a blame row with a relative `file_path` is
unusable as evidence. Drop it, and say how many were dropped.

## The three signals

Only one of the three is available today.

| Signal | Where it lives | State |
| :--- | :--- | :--- |
| No exit-0 test run | `sessions.primary_outcome` below `test_or_build_passed` | **available** |
| A done-claim the verifier demoted | `verify::VerificationNote`, computed at load, never stored | **new work** |
| A loop the sentinel caught | `LoopSentinelAlert` in `watch.rs`, computed live, never stored | **new work** |
| Recoveries | `RecoveryDetector`, computed at load, never stored | **new work** |

`crates/outcomes/src/verify.rs` already returns a note for every claim whose
kind or confidence reality contradicted, and `detect_outcomes` throws them
away — `detect_outcomes_with_verification` is the variant that keeps them, and
nothing calls it outside tests. Persisting those notes is the highest-value
piece of new work in this spec, because "the agent said the tests passed and
they did not" is a far sharper flag than "the outcome rung is low".

Storage needs one new table:

```sql
CREATE TABLE session_risk (
  session_id TEXT PRIMARY KEY REFERENCES sessions(session_id) ON DELETE CASCADE,
  demoted_claims INTEGER NOT NULL DEFAULT 0,
  loop_alerts INTEGER NOT NULL DEFAULT 0,
  unresolved_loops INTEGER NOT NULL DEFAULT 0,
  recoveries INTEGER NOT NULL DEFAULT 0
);
```

Written by the scanner from the detectors that already run there. No new
parsing, no new pass over the logs.

## The MCP tool

    suspect_commits(repo, branch?, since?, base?, window_hours?)

| Param | Type | Default |
| :--- | :--- | :--- |
| `repo` | absolute path to a checkout | required |
| `branch` | string | current HEAD |
| `base` | ref to diff against | the branch's upstream, else `origin/main` |
| `since` | RFC 3339 | ignored when `base` resolves |
| `window_hours` | integer | 24 |

Returns:

```json
{"repo": "unfoundbox/agentworth", "branch": "feat/x", "base": "origin/main",
 "commits_scanned": 93, "attributed": 39, "unattributed": 54,
 "relative_path_rows_dropped": 127,
 "suspect": [
   {"sha": "bd84c85a", "subject": "docs: …",
    "sessions": [{"session_id": "33122482-…", "adapter": "claude_code",
                  "model": "claude-sonnet-5", "outcome": "artifact_changed",
                  "reasons": ["no_test_run"],
                  "evidence_path": "/Users/…/docs/HANDOFF.md",
                  "occurred_at": "2026-09-01T14:02:11Z"}]}],
 "prompt": "3 commits on feat/x came from sessions that never …",
 "receipt": {"counted_at": "…", "index_last_session_at": "…",
             "db_path": "~/.agentworth/agentworth.db"}}
```

`unattributed` is load-bearing. 54 of 93 commits here matched no indexed
session at all — written before the index existed, written on another machine,
or written by hand. The tool must never let those read as clean. A commit with
no authoring session is **unknown**, and the return says so in its own field
rather than silently omitting it.

The `prompt` field is the copyable block:

    3 commits on feat/x came from sessions that never ran a test.

      · bd84c85a  docs: …          [session 33122482 · claude-sonnet-5]
      · 97e9c6eb  fix: …           [session ses_fb3a · opencode]

    Check these before pushing. Session ids are queryable with session_get.

That is the whole output. No patch, no diff, no PR.

## The pre-push hook

    #!/bin/sh
    agentworth suspect-commits --repo . --quiet || true

Prints the same list and exits 0 always. It is a note on the way out, not a
gate. A hook that blocks a push on a heuristic with a measured 2.6% hit rate
would be turned off within a day, and then it protects nothing.

## Deliberately not built

- **No patch, no PR, no commit.** `market-autofix.md`'s conclusion, kept.
- **No blocking hook.** Exit 0, always.
- **No blame on the human.** The output names the session and the model. It
  does not name a branch owner or a reviewer.
- **No GitHub integration.** Local git only. Everything else in this product is
  local, and a token is a different consent question.

## Sequencing

1. Absolute-path filtering, plus a count of what was dropped. Without it the
   tool ships a 33% flag rate that is 90% wrong.
2. `session_risk`, written by the scanner from detectors that already run.
3. The MCP tool, with `no_test_run` as its only reason code.
4. Add `demoted_claim` and `loop` reasons once `session_risk` has data.
5. The pre-push hook, last — it is the same query with a terminal renderer.

## Open questions

- Is 24 hours the right attribution window? A commit can land days after the
  session that wrote it, and a rebase moves the timestamp.
- Should `opencode`'s relative paths be repaired at scan time instead of
  dropped at query time? The adapter knows the cwd; the schema loses it.
- What is the right treatment for a commit with several authoring sessions,
  one clean and one suspect? Flagged here if any session is suspect, which will
  over-fire on long-running files.
- Does the flag correlate with anything real — a later revert, a bug fix
  touching the same lines? Assumed here, unmeasured, and the only thing that
  would prove the tool works.

## What shipped that this spec did not say

### Git worktrees, found while measuring

A linked git worktree normally lives *inside* the main checkout
(`<repo>/.claude/worktrees/<name>`). Anchoring against the main root alone turns
an edit made in one into `.claude/worktrees/<name>/crates/a.rs` — a path no
commit will ever name — so the session attributes to nothing and the commit
reads as unattributed. Measured on this repository's own index: **58 of 308**
in-repo blame rows were written from a worktree, and every one of them was lost.

`Storage::blame_for_repo` now takes the linked worktree roots alongside the main
one, longest first, and `agentworth suspect` collects them with
`git worktree list --porcelain`. On the last 60 commits of `origin/main` this
doubled attribution, from 10 commits to 20.

### `no_outcome_detected` is its own reason code

The spec's single signal was "`primary_outcome` below `test_or_build_passed`",
which folds a null outcome in with a low one. `docs/capability-matrix.md` says
why that would mislead: several adapters extract no outcomes at all, so a null
can mean "this session proved nothing" or "this adapter cannot tell us." Those
are different claims and the reader has to be able to see which one they have —
the same distinction `outcome_rate` already draws with its `no_outcome_detection`
reason. Both still flag a commit.

## The measurement, re-run against what shipped

Replicated in SQL plus a python port of the shipped anchoring rule, against a
copy of the real index (`~/.agentworth/agentworth.db`, 25,206 blame rows). Not
the binary itself: it was measured on a Mac that cannot run the Linux build.
Index copy's newest session is 2026-09-01T19:04Z, so commits after that point
have no session history to check yet.

**Last 60 commits of `origin/main`:**

| | naive suffix | anchored |
| :--- | ---: | ---: |
| Attributed | 26 | 20 |
| Flagged | 0 | 0 |

Zero flags either way. Every session behind those 20 commits reached
`test_or_build_passed` or higher — a true and boring answer, and the right one.

**All 104 commits of `origin/main`,** which reaches back into the period the
original measurement covered:

| | naive suffix | anchored |
| :--- | ---: | ---: |
| Attributed | 59 | 44 |
| Flagged | 17 | **1** |
| Flag rate of attributed | 28.8% | **2.3%** |
| Unanchored rows dropped | — | 111 |

The spec predicted 2.6%; the shipped rule measures 2.3% on a slightly different
range. The 26-point gap is the bug this feature exists to avoid.

**The sample.** Only one commit survives anchoring, so all 1 was checked rather
than 10: `bd84c85a` matched `docs/HANDOFF.md` against
`/Users/…/agentworth/docs/HANDOFF.md`, written by session `33122482` (claude_code,
claude-sonnet-5, 1,672 events, 173 tool calls, `artifact_changed`). A real
absolute path inside this checkout, a real session, a real unproven outcome.
Correct — and the same commit the original measurement's table marked "yes".

So the 10-flag sample was taken from the other side, which is the more useful
one: **the first 10 flags anchoring removes.** All ten are false, all ten for the
same reason — a relative path from an `opencode` session whose own repository
resolves to `Users/saurabh`, not `unfoundbox/agentworth`:

| commit | matched | evidence path | session repo |
| :--- | :--- | :--- | :--- |
| ca746787 | `.gitignore` | `.gitignore` | `Users/saurabh` |
| ddf62cb4 | `README.md` | `web/README.md` | `Users/saurabh` |
| 80d9d22a | `README.md` | `web/README.md` | `Users/saurabh` |
| 6f1c0478 | `AGENTS.md` | `AGENTS.md` | `Users/saurabh` |
| 1185ce4b | `README.md` | `web/README.md` | `Users/saurabh` |
| 97e9c6eb | `apps/cli/src/main.rs` | `apps/cli/src/main.rs` | `Users/saurabh` |
| c245897e | `Cargo.lock` | `Cargo.lock` | `Users/saurabh` |
| 0dcd2d80 | `.gitignore` | `.gitignore` | `Users/saurabh` |
| 5b35e4e7 | `Cargo.lock` | `Cargo.lock` | `Users/saurabh` |
| 947f854a | `crates/adapters/src/claude.rs` | `crates/adapters/src/claude.rs` | `Users/saurabh` |

10 of 10 removed flags false, 1 of 1 kept flag right.
