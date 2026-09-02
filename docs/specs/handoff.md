# Handoff

Status: built, PR #TBD (2026-09-02). Shipped as the `session_handoff` and
`carry_forward` MCP tools, `agentworth handoff` and `agentworth loose-ends` on
the CLI, and `agentworth_outcomes::loose_ends` — the TypeScript detector ported
to Rust. What was designed below and what actually shipped differ in four
places; they are marked **Shipped:** in place rather than rewritten, so the
design record still reads as it was written.

The rest of this document is the design as measured on 2026-09-02.

## The one-line version

The handoff written by the machine that did the work — under 60 lines, every
claim carrying the session id it came from.

## The problem, stated by the person who has it

I write a handoff file by hand at the end of most working days, because the
next session starts blank and will otherwise redo what the last one finished.

**338 files matching `*handoff*.md` sit under `~/code`. 78 of them were
written in the last eight days.** Every one is me re-typing what a transcript
already knows.

The parts I re-type are mechanical: what changed, what ran, what passed, what
was promised and not done. I do not mind writing the judgment. I mind writing
the inventory.

## What the data can already answer

Measured against a copy of the local index. 2,960 non-stub sessions
(`total_events > 1 AND total_tokens > 0`).

```sql
SELECT COUNT(*) nonstub,
  SUM(EXISTS(SELECT 1 FROM file_modifications f
             WHERE f.session_id = s.session_id))            with_files,
  SUM(primary_outcome IS NOT NULL)                          with_outcome,
  SUM(primary_outcome IN ('test_or_build_passed',
      'commit_observed','ci_or_deployment_verified'))       test_ran,
  SUM(tool_calls_count > 0)                                 with_tools,
  SUM(prompt_preview IS NOT NULL)                           with_prompt
FROM sessions s WHERE total_events > 1 AND total_tokens > 0;
```

| Handoff line | Source | Sessions that have it |
| :--- | :--- | ---: |
| Files touched | `file_modifications` | 1,152 of 2,960 |
| Tools and shell commands run | `tool_calls_count` | 2,763 |
| Outcome rung reached | `sessions.primary_outcome` | 1,464 |
| Proof a test or build passed | rung ≥ 3 | 1,002 |
| Loose ends | `looseEnds.ts`, browser-side | any loaded trace |
| **What the task was** | `sessions.prompt_preview` | **0** |

**The single most important line of a handoff is the one field that is empty
in every row.** `prompt_preview` was added for exactly this and has never been
filled — `docs/specs/README.md` already names it as item 2. A handoff that
opens with "session `agent-af702e89` touched 14 files" is not a handoff. Fill
that field first, or this tool ships a title-less document.

> **Corrected during the build.** The *code* has filled `prompt_preview` since
> #47; every one of those 2,960 rows was written before it and nothing
> rescanned them. `agentworth scan --force` fills them. See "Shipped" under
> New work.

Loose ends are further from ready than the sequencing table suggests. #44
shipped `apps/dashboard/src/utils/looseEnds.ts` — a browser-side detector that
runs on a trace already loaded into the dashboard. There is no
`agentworth loose-ends` subcommand, no `/api` route, and no MCP tool. The
regex and its three gating filters are good and measured (120 of 212 stated
intents across five sessions are gated, not dropped); they are in TypeScript,
in the wrong process. Porting that logic to Rust is real work in this spec, not
a call into something that exists.

## What a hand-written handoff has that the data cannot supply

Read from a real one — a 95-line daily handoff written at 99% weekly quota:

| In the file | Can the index produce it |
| :--- | :--- |
| Files changed, tests run, suite result | yes |
| Which PRs merged, what is open and unreviewed | no — no git or GitHub in the index |
| "Trust `git worktree list` over any doc, including this one" | no — that is an instruction about trust |
| "Two divergent drafts, one is likely the keeper" | no — a judgment about which of two things matters |
| "Open decisions — Saurabh's, do not decide for him" (4 items) | no |
| "We can't do money path in my absence" | no — a standing rule quoted from a person |
| "Quota expires Saturday morning" | no — external state |
| "Bare `python3` lacks fastapi" | no — an environment trap learned by hitting it |

The pattern is clean. **The machine owns the inventory. The human owns the
judgment, the open decisions, and the traps.** So this tool does not replace
the handoff file. It replaces the two-thirds of it that is transcription, and
leaves a section the human fills.

Say that in the output. A generated handoff that silently omits the open
decisions is worse than no handoff, because the next session reads it as
complete.

## The MCP tools

    session_handoff(session_id?, max_lines?, include_loose_ends?)

| Param | Type | Default |
| :--- | :--- | :--- |
| `session_id` | string | the most recent session for the caller's cwd |
| `max_lines` | integer | 60, hard ceiling 120 |
| `include_loose_ends` | boolean | true |

Returns `{ "markdown": "…", "receipt": {…}, "gaps": […] }`. The markdown:

    # Session 452c23fd · motionvector/studio · 2026-09-01 14:02–19:40

    **Task** _(prompt_preview empty — unknown)_
    **Outcome** rung 4, commit_observed
    **Cost** 2.99M tokens · 29,642 events · 5h 38m

    ## Files touched (14)
    - crates/storage/src/lib.rs — 9 edits, last 19:12
    …

    ## Ran
    - `cargo test -p agentworth-storage` — exit 0, 19:20
    …

    ## Said it would, no evidence it did (3)
    - "I'll re-run the export once the schema lands"   [seq 8,441]
    …

    ## Not in this handoff
    Open decisions, PR state, and environment traps are not in the index.
    Add them by hand.

    ---
    session 452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd · generated 2026-09-02T09:41Z
    index last updated 2026-09-01T18:58Z

The last two lines are the receipt and they are not optional. A handoff that
cannot be traced to a session is a paragraph, and the next session has no way
to check it.

`gaps` is the machine-readable "I don't know": `["prompt_preview_empty",
"no_file_modifications", "no_outcome_detected"]`. When the whole session
resolves to nothing but a token count, the tool returns the receipt and an
empty body rather than padding it. A fabricated handoff is read by an agent
that cannot check it — that is the failure mode this product exists to
prevent, and it would be self-inflicted here.

    carry_forward(repo, n?, since?)

| Param | Type | Default |
| :--- | :--- | :--- |
| `repo` | string, as `extract_repository_or_workspace` returns | required |
| `n` | integer | 3, ceiling 10 |
| `since` | RFC 3339 | none |

Returns the last `n` handoffs for that repo, newest first, each with its
receipt — so a session's first tool call can be "what happened here recently"
and the answer is structured, not a file it has to find and parse.

**Worktrees are the trap.** `extract_repository_or_workspace` prunes the `--`
worktree suffix, so `motionvector/pluto` and its 14 worktrees collapse to one
key. That is right for carry-forward and wrong for anything that needs to know
which checkout ran. Measured over 2,960 non-stub sessions: 253 distinct
project slugs on disk collapse to 43 repo keys. Six worktrees of one repo
answer to one `repo` value, which is what carry-forward wants and what a
"where did this run" question does not.

## New work

1. **Fill `prompt_preview`.** Nothing else in this spec matters without it.
2. Port `looseEnds.ts` to Rust, in `agentworth-outcomes`. Same regex, same
   three gates, same measured thresholds. One definition, then delete the
   TypeScript or have it call the API.
3. Persist a shell-command exit-code index. `TestOrBuildPassed` proves *a*
   command passed; the "Ran" section needs to name which ones. The events hold
   this; the index does not.
4. A markdown renderer with a real line budget — 60 lines means truncating the
   file list, not emitting 200 lines and apologising.

**Shipped:**

1. **Already fixed, in #47, before this spec was written.**
   `Storage::upsert_session` has populated `prompt_preview` from the first
   user message since v0.1.10, with a regression test
   (`test_prompt_preview_extracted_from_first_user_message`). The measurement
   above — 0 of 2,960 — is true of *rows written before #47*, not of the code:
   nothing rescanned them, so they still carry the null they were indexed
   with. Verified on lenovo by scanning a fresh fixture transcript, where the
   handoff opens with its real task line.

   So the gate this spec called blocking is `agentworth scan --force`, not a
   missing feature. The tool still does not depend on it: a session indexed
   before #47 renders "first prompt not indexed yet" and puts
   `prompt_preview_empty` in `gaps`.
2. Done — `crates/outcomes/src/loose_ends.rs`, verified sentence-for-sentence
   against the TypeScript in node. Two things the port had to get right: the
   25..=240 length window is measured in UTF-16 units, because Rust byte length
   would move the window for any non-ASCII transcript; and the `(?<=[.!?])\s+`
   split is hand-rolled, because `regex` has no lookbehind. The TypeScript is
   still there and still the dashboard's path — deleting it needs an `/api`
   route the dashboard can call, which is not in this change.
3. **Not built, and the premise was wrong.** "The events hold this" is not true
   for Claude Code: `crates/adapters/src/claude.rs` builds every `ShellCommand`
   with `exit_code: None`, because a `Bash` tool call records the command that
   was requested and its result arrives as a separate event. Persisting an
   index of a field that is always null buys nothing. Instead the "Ran" section
   correlates tool call to tool result at read time and falls back to the
   harness's own `is_error` flag, which is a weaker receipt than an exit code
   and says so on the line: "reported an error, no exit code recorded". Getting
   real exit codes is adapter work, not index work.
4. Done — `apps/cli/src/handoff/markdown.rs`. The budget is allocated before
   anything is written: every section gets one row before any gets a second,
   truncated sections say how many rows they dropped, and a section that could
   not be afforded at all is named rather than vanishing.

### Also shipped, not in the design above

- **A "Said it decided" section.** Sentences that state a choice was made,
  quoted verbatim with their sequence number. It does not contradict "the human
  owns the judgment" below: it decides nothing, summarises nothing, and claims
  nothing is current. The heading says "said it decided" for that reason, and
  the "Not in this handoff" note still ships underneath it.
- **`agentworth handoff` and `agentworth loose-ends` on the CLI.** The spec is
  MCP-only. `README.md` and `SKILL.md` had claimed `agentworth loose-ends`
  existed since #44 when nothing on the CLI could reach the detector; it exists
  now.
- **`Storage::list_sessions_for_repo`**, backing `carry_forward` and the
  cwd-relative default. `repo` is still not a stored column — the scan is
  bounded and reports `scan_exhausted` when the bound was hit.

## Deliberately not built

- **No writing to disk.** The tool returns markdown. Where it lands is the
  caller's business, and a tool that writes files into repos is a different
  trust boundary.
- **No summarisation by a model.** Every line is a fact from a row. The moment
  a model writes the prose, the receipt stops meaning anything.
- **No git or GitHub state.** Out of scope, and it is the largest thing the
  hand-written version has. Say so in the output rather than faking it.
- **No auto-run at session end.** Nothing in this product watches for a session
  ending. The agent asks.

## Sequencing

1. `prompt_preview`. It is already on the README's list and it gates this.
2. Loose ends in Rust, exposed as its own MCP tool first — it is useful alone
   and it is the piece most likely to be wrong.
3. `session_handoff`, assembling what exists, with `gaps` honest from day one.
4. `carry_forward` — the same renderer over an ordered query.
5. The shell exit-code index, which upgrades "Ran" from a claim to a receipt.

## Open questions

- What is a session's boundary for a handoff? A 29,642-event session compacted
  eight times is several days of work in one row.
- Should `carry_forward` merge overlapping handoffs or list them? Listing here,
  because merging needs judgment about which of two contradictory facts is
  current.
- Does the human ever fill the "Not in this handoff" section, or does the
  generated file quietly replace the hand-written one and lose the decisions?
  That is the risk this spec creates, and nothing in the data can catch it.
