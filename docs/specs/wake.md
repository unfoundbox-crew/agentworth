# Wake

Status: built, PR #TBD (2026-09-05). Shipped as the `session_wake` MCP tool and
`archie session wake` on the CLI. Measured before the build, on this machine's
own index; the numbers below are from that measurement.

## The one-line version

One tool call that tells a cold agent what it was doing, in under 30 lines,
from the index and the checkout it is standing in.

## The problem

An agent that starts cold, or comes back from a compaction, spends 15k to 30k
tokens answering four questions before it can work: what was I doing, what
state is the checkout in, what passed and what failed, what is still open. It
answers them by listing directories, grepping transcripts, reading handoff
markdown and parsing `git diff`. Every one of those reads is a fact the index
already holds or a `git` command that costs nothing.

`session_handoff` and `session_carry_forward` exist and were meant to be that
first call. Measured on 2026-09-05 against the `unfoundbox/agentworth` key,
they are not:

| What | Measured |
| :--- | :--- |
| Sessions indexed under the key | 131 |
| Of those, subagent transcripts (`.../subagents/agent-*.jsonl`) | 127 |
| `session_carry_forward(n=3)` returned | three subagents of one 2026-09-04 session; the parent (1,826 events, 106M tokens) never appears |
| `session_handoff()` with no id returned | the newest subagent |
| `session_list(repo=…, limit=8)` returned | nothing, `truncated: true` — it over-fetches 4x by start date and 32 rows across every repo held none of this one |
| Handoff "Ran" section for that subagent | 68 commands, 38 of them shown, every line a `cd … && …` prefix |

Two defects and one shape problem. The defects: subagent transcripts win
"newest" because they start after their parent, and a repo filter that
over-fetches by a fixed factor across every repo finds nothing for a repo
that has been quiet for a day. The shape problem: a handoff is an inventory,
and an inventory of 68 shell commands is not what a waking agent needs. It
needs the last thing that passed, the last thing that failed, and whether the
failure was ever retried.

## What the tool returns

    session_wake(workspace?, repo?, include_raw?)

| Param | Type | Default |
| :--- | :--- | :--- |
| `workspace` | path | the server's working directory |
| `repo` | string, the same key `session_carry_forward` takes | derived from `workspace` |
| `include_raw` | boolean | false; redacted is the default everywhere |

Returns `{ "markdown": "…", "report": {…}, "gaps": […] }`. The markdown is
at most 30 lines. This one, rendered from a fixture shaped like the
2026-09-04 session:

    # Wake · unfoundbox/agentworth · 2026-09-05 15:04Z
    Checkout ~/code/unfoundbox/agentworth/.claude/worktrees/plugin · branch claude/landing-product-page · HEAD a25b9cd "site: the landing page sells the product" · 3 files dirty · 2 ahead of origin
    Index scanned 2026-09-05 12:42Z · source unchanged since scan

    ## Last session b214830c · 2026-09-04 05:32–16:17 · 106M tokens · 1,826 events · 2 compactions
    **Task** AgentWorth as a Claude Code plugin: plugin.json, own marketplace, MCP over npx …
    **Last asked** Rebuild the landing page terminals at 2x and push …
    **Ran in** ~/code/unfoundbox/agentworth/.claude/worktrees/plugin on claude/landing-product-page
    **Outcome** rung 4, commit_observed
    **Proof** last passed `npm run build` 16:14 · last failed `node capture-landing.mjs` 16:12, not re-run
    **Changed** 5 files · Terminal.tsx (3) · LandingPage.tsx (3) · index.css (1)
    **Loose ends** (3)
    - "I'll re-run the export once the schema lands"  [seq 8441]
    - "Then I'll delete the stale worktree"  [seq 9102]
    **Said it decided** "We're going with a symlink for the root SKILL.md"  [seq 3310]
    **Forgotten** 12 decisions dropped by compaction — `session_forgotten` has them

    ## Before that
    - 09-03 09:20 · rung 4 · "the cockpit: a bare archie opens the grammar with a cursor"
    - 09-02 14:26 · rung 3 · "archie stats ladder"

    ## Next
    Blocker `node capture-landing.mjs` failed at 16:12 and was not run again.
    Next "Then I'll delete the stale worktree"  [seq 9102]
    Not here: PR and CI state, open decisions. `gh pr list` for the first.

    ---
    session b214830c-ff21-4c23-8521-55f1e59d60a4 · claude_code · generated 2026-09-05T15:04Z · redacted

Every line is a fact from a row, a `git` read, or a stat call. No line is
written by a model.

### Where each line comes from

| Line | Source | When absent |
| :--- | :--- | :--- |
| Checkout, branch, HEAD, dirty, ahead | `git` run read-only in `workspace`, two-second timeout per call | `git_unavailable` or `not_a_git_checkout` in `gaps`; the line says which |
| Index scanned, source changed | `MAX(scanned_at)`; the session's `sources.mtime` against the file's mtime now | `source_unreadable` |
| Last session | newest **primary** session for `repo` by last activity (`COALESCE(ended_at, started_at)`), subagent transcripts excluded | `no_session_for_repo` and the document stops after the checkout block |
| Task | `sessions.prompt_preview` | `prompt_preview_empty` |
| Last asked | the last user message in the trace that is not a compaction summary | `no_user_message` |
| Ran in | the `cwd` and `gitBranch` the adapter recorded from the transcript's own records | line omitted; adapters other than Claude Code do not carry it |
| Outcome | the same strongest-rung logic `session_handoff` uses | `no_outcome_detected` |
| Proof | newest verification-shaped command that passed, newest that failed, and whether the failed one's exact command string ran again later and passed | `no_commands_recorded` |
| Changed | files written or edited, three most recent by name, with the total | `no_file_modifications` |
| Loose ends | `agentworth_outcomes::find_loose_ends`, newest three | `no_loose_ends` |
| Said it decided | `handoff::find_decisions`, newest one | omitted |
| Forgotten | count only; the statements are one call away | omitted when never compacted |
| Before that | the next two primary sessions for the repo, one line each | omitted |
| Blocker | the failed verification command from Proof when nothing re-ran it | "none recorded" |
| Next | the newest loose end | "none recorded" |

"Ran in" is the one line that needs adapter work. Claude Code writes `cwd`
and `gitBranch` on every record of a transcript; the adapter never kept them.
It now records the last non-empty value of each in the trace's `metadata`
(`workspace.cwd`, `workspace.git_branch`), which is where every other
adapter would put the same fact. The wake tool reads the generic key and
says nothing when it is absent. `parser_version` is not bumped for this:
wake re-parses the transcript it reports on, so the value is live, and a
bump would re-parse every session on every machine for one optional line.

### Why the checkout comes from `git` and not the index

`docs/specs/handoff.md` chose not to read git state, and that was right for a
handoff: a document about a past session should not carry the current
branch as if the session had seen it. Wake is the other direction. It is
about now, and a waking agent's first three commands are `pwd`, `git branch`
and `git status`. Running them read-only inside the tool costs nothing and
saves three round trips. The two facts are kept apart in the output: the
"Checkout" line is the present, the "Ran in" line is what the last session
recorded.

### Why it is not `session_carry_forward` with a smaller budget

Carry-forward lists handoffs and refuses to merge them, because merging two
sessions' facts needs judgment about which is current. Wake does not merge
either: it reports one session in full and the ones before it as one line
each. The difference is which facts it keeps. A handoff keeps the inventory
so a person can audit it; wake keeps the last proof, the last failure, and
the loose ends, because that is what the agent acts on. Both exist and both
are correct; they answer different questions.

## Also in this change

- **Subagent transcripts no longer answer for their parent.**
  `session_carry_forward` and the no-id default of `session_handoff` and
  `session_forgotten` now resolve to primary sessions.
  `session_carry_forward` takes `include_subagents` (default false) for the
  old behaviour. `Storage::list_sessions_for_repo` takes the same switch and
  orders by last activity, so a parent session that started at 05:32 and
  ended at 16:17 is newer than a subagent it spawned at 14:03.
- `agentworth_schema::is_subagent_transcript` names the rule in one place,
  beside `extract_repository_or_workspace`, which already knows the same
  path shape.

## Deliberately not built

- **No scan.** The server never scans on its own; that rule stands. The
  "Index scanned" line says how stale the answer is and the "source changed
  since scan" clause says whether the session it reports on has grown since.
  An agent that needs its own current session indexed runs `archie scan`.
- **No PR or CI state.** Still not in the index. The output names the gap
  and the one command that fills it.
- **No fix to `session_list`'s repo over-fetch.** It is a real defect
  (measured above) and it is a different change: that tool takes eight
  other filters, and the bounded scan `list_sessions_for_repo` uses does
  not compose with them. Recorded here so it is not re-discovered.
- **No model.** Nothing here summarises. A line the index cannot supply is a
  gap, never a guess.

## Open questions

- Should wake take a session id, to wake into a specific past session rather
  than the newest? Not yet: `session_handoff` does that, and the two would
  drift. Revisit if the newest-primary rule picks the wrong session in
  practice.
- The `~300 token` target. Measured on the fixture the markdown is under 30
  lines; the token count depends on path lengths and is reported in the PR,
  not promised here.
