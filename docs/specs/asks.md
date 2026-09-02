# Asks: a questions-to-answers index

Status: built, PR #TBD (2026-09-02). Implemented as `agentworth_outcomes::asks`,
`agentworth asks`, and the `session_asks` MCP tool. Tier 1 only — see "What's
deliberately not in this build" for tier 2 (`--summarize`) and tier 3
(rejected).

## The problem

In a long session Saurabh asks a question. The answer lands several messages
later, after tool calls, subagent notifications, and other assistant text —
buried, not missing. He re-asks it because scrolling costs time and re-asking
costs tokens. The fix is not a better scrollback UI; it's a session that can
be told, on request, "the answer to that is already here" — for free, no
model, no network call.

## What counts as a question

Two sources, both cheap to detect deterministically:

- **A `?` sentence in a user turn.** Split the turn into sentences (reusing
  `loose_ends.rs`'s splitter, so the two extractors cut text the same way);
  any sentence containing `?` is a question. Multiple qualifying sentences in
  one turn each become their own entry, same convention `loose_ends.rs`
  already uses for multiple stated intents in one message.
- **A flag-prefixed line in an assistant turn.** A line starting with `⚑` or
  `🚩`, once a common markdown list/bold prefix is stripped — the convention
  `~/.claude/CLAUDE.md` calls "prefix the line with a flag character" for a
  decision that needs Saurabh specifically. This is the assistant asking him
  something, not the other way around.

## What counts as an answer

For a user-asked question: scan forward through the trace, skipping
everything that isn't `AssistantMessage` (tool calls, tool results, task
notifications, compaction records — none of them stop the scan, they're just
not candidates). The first assistant message carrying a line of at least 20
characters that isn't pure filler ("waiting", "on it", "one moment", ...) is
the answer; the excerpt is that line onward, trimmed to 200 characters. A
`UserMessage` before any such line ends the scan with no answer.

Three statuses come out of this:

- **`answered`** — a substantive line was found, and it doesn't read as a
  question itself.
- **`flagged_back_to_user`** — either the question *was* a flag line (always
  this status, since there is no assistant text later that would count as
  answering a question the assistant itself asked), or the first substantive
  reply's own first sentence ends in `?` — the assistant handed the question
  back rather than answering it.
- **`no_reply_yet`** — no assistant text before the next user turn, or before
  the trace ends.

Every entry carries a `pointer` (event sequence and timestamp) to jump to:
the answer's location when there is one, otherwise the question's own —
there is always somewhere to go, even when the answer is "nowhere yet."

No model reads the transcript. Three regexes and a line-length check, the
same `regex_v1` posture `compaction-diff.md`'s extractor already shipped.

## What ships (tier 1)

1. **Extraction** — `agentworth_outcomes::asks` (`find_asks`,
   `find_asks_in_trace`), next to `loose_ends.rs` and `compaction_diff.rs`,
   reusing `loose_ends::split_sentences`. Unit-tested against all three
   statuses, both flag glyphs, filler/short-line skipping, trimming, and
   newest-first ordering.
2. **CLI** — `agentworth asks [--session <id-or-prefix-or-path> | --current]
   [--since <2h|1d|RFC3339|YYYY-MM-DD>] [--unanswered] [--json]`, rendered
   through the same `handoff()` grid `agentworth forgotten` reuses rather than
   a bespoke table — one section, newest first, `seq N status` gutter,
   `question — excerpt` claim. `--session` accepts an indexed ID/prefix or a
   raw JSONL path (parsed directly through the Claude Code adapter,
   bypassing the index, when it isn't indexed). Times and reports its own
   wall-clock cost on every run.
3. **MCP tool** — `session_asks(session_id?, since?, unanswered_only?,
   limit=50, include_raw=false)` in `apps/cli/src/mcp/`, same redaction
   convention as every other tool here (`docs/specs/mcp-server.md`): redacted
   by default, `include_raw` is a per-call opt-in, never global. Reuses the
   shared `crate::asks` report-builder the CLI calls, so the two surfaces
   can't drift.

## What's deliberately not in this build

- **Tier 2 — `--summarize`.** A small model reads each question/answer pair
  and produces a one-line summary, roughly 500 tokens per pair. Worth
  building once tier 1 shows real questions whose "first substantive line"
  answer is technically correct but not a clean one-liner. Print the
  estimated cost before running whenever the count exceeds 20 pairs, so
  nobody eats a summarization bill by accident.
- **Tier 3 — an agent reads the whole transcript.** Rejected outright.
  Everything tier 1 answers deterministically for zero tokens, an agent could
  also answer by reading every event — at full transcript cost, every time,
  with no receipt. The entire point of this index is that the answer is
  already sitting in the trace; paying an agent to re-derive it defeats the
  reason to build an index at all.

## Decisions made here

- Only `AssistantMessage` events count as "assistant text" — tool calls,
  tool results, `Custom` (subagent delegations, compaction summaries), and
  everything else are skipped while scanning, never a stopping point. Only a
  `UserMessage` stops the scan.
- A flag-line question never gets a forward scan for an answer. It's the
  assistant's own question; the only thing that could answer it is the
  user's next message, which this index doesn't reach for (it only follows
  assistant text). It is unconditionally `flagged_back_to_user`.
- "The reply is itself a question" is judged by the *first sentence* of the
  candidate answer ending in `?`, not by the presence of a `?` anywhere in
  it — a real answer that happens to contain a rhetorical question later on
  shouldn't get miscategorized.
- `--unanswered` means "status is not `answered`" — it includes both
  `flagged_back_to_user` and `no_reply_yet`. A pointer is still returned for
  a `no_reply_yet` entry (it points at the question itself), so "unanswered"
  results are never missing a place to jump to.

## Open questions

- Is 20 characters the right substantive-line floor, and is the filler list
  complete? Both are first-pass values, not measured against a corpus the
  way `compaction-diff.md`'s Jaccard threshold was.
- Should a flag-line question's excerpt show the user's actual next reply
  (crossing into `UserMessage` content, which this extractor otherwise never
  reads for "answers")? Left out here because it would blur the "assistant
  text only" rule that keeps the scan simple and fast.
