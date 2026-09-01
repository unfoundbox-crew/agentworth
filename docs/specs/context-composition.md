# Context composition — what fills the window before the work does

Status: proposed. Measured against real logs, and one part of the original idea
turns out not to be measurable at all.

Working name in conversation was "chewed". This document argues for **context
composition**, on the grounds below.

## The question

> How many tokens does a model chew on tool calls and `.md` files before it
> has the actual working context in attention?

It is a good question because it is about waste rather than volume. Nobody
minds a large session; people mind a large session where most of the window
went to things that were not the task.

## What is actually measurable, and what is not

Checked against a real Claude Code JSONL and against what the adapter
normalises, rather than assumed.

| Component of the window | Where it lives | Reachable? |
| :--- | :--- | :--- |
| Tool results | `ToolResult` content, normalised | yes |
| Injected files and attachments | `Custom` events, which carry the raw record verbatim | yes |
| User prompts | `UserMessage` content | yes |
| Assistant text and thinking | `AssistantMessage`, `thinking_tokens` in raw usage | yes, partly normalised |
| Shell command output | `ShellCommand` | yes |
| **System prompt** | **never written to the transcript** | **no** |
| **Tool definitions / schemas** | **never written to the transcript** | **no** |

The last two rows are the correction, and they matter enough to change the
feature.

**The fixed per-request overhead cannot be measured from session logs, by
anyone.** Claude Code records `usage.input_tokens`,
`cache_creation_input_tokens` and `cache_read_input_tokens`, but nothing that
says how those totals divide between the system prompt, the tool schemas, and
the conversation. That is not an indexing gap this project can close — the
information was never written down. A backend change cannot recover it and
neither can a frontend one.

So the literal question — "how much before working context" — is not fully
answerable. Say that plainly rather than shipping a number that implies it is.

What **is** answerable is the composition of everything the transcript does
hold, which is most of the window on any long session and all of the part the
user can actually act on:

    of the conversation that was re-sent on every request,
    what share was tool output, what share was injected files,
    and what share was the actual dialogue?

That is a real question, it is actionable — a 40k-token file read that gets
re-sent 200 times is a thing you can stop doing — and it needs no new data.

## Why not "chewed"

`agentworth chewed` is memorable and it reads as an accusation of the model.
The measurement is not about the model at all: the window fills because tools
returned a lot, or because files were injected, both of which are decisions in
the harness and the prompt rather than model behaviour. Naming it after
chewing points blame at the wrong thing, in the same way `loose-ends.md`
argues that "misses" points blame at the wrong thing.

`context-composition` says what it is. The CLI reads:

    agentworth context --session <id>

    Context composition · 6,366 events · 191h span

      tool output      68%   ████████████████████░░░░░░░░░
      injected files   19%   █████░░
      dialogue         11%   ███
      thinking          2%   █

      Largest single contributor
      · Read of packages/runtime/schema.json — 41k tokens, re-sent 213 times

    Not measured: system prompt and tool definitions are never written to the
    transcript, so this covers the conversation only.

That last line ships with the output. It is not a footnote.

## Lane

Almost entirely frontend, which is unusual for this repo and worth stating so
nobody waits on a backend change that is not needed.

| Piece | Lane | Why |
| :--- | :--- | :--- |
| Composition from a loaded trace | frontend | `Custom` carries the raw record verbatim, so the content is already in the browser |
| The dashboard surface | frontend | one panel beside cache warmth |
| `agentworth context` CLI | backend | same data, different surface |
| Cache TTL split | backend, small | see below |

### One thing worth normalising while nearby

Raw Claude Code usage carries a field the index currently drops:

```json
"cache_creation": {
  "ephemeral_1h_input_tokens": 48059,
  "ephemeral_5m_input_tokens": 0
}
```

That is the provider labelling which cache tier each write went to. It is
directly relevant to `cache-economics.md`, which infers a 60-minute boundary
from gap-versus-recreation behaviour: this field would let that boundary be
**confirmed against the provider's own accounting** rather than only inferred
from the shape of the data. `output_tokens_details.thinking_tokens` is dropped
the same way and would make the thinking share exact rather than estimated.

Neither blocks this spec. Both are small, and both belong to whoever owns the
adapter.

## What it produces

One panel, in the inspector, beside cache warmth. Not a tab.

- A single stacked bar: tool output, injected files, dialogue, thinking.
- One line naming the largest single contributor, with how many times it was
  re-sent. That is the actionable part — the bar tells you the shape, the line
  tells you what to do about it.
- The not-measured caveat, always visible.

Deliberately not: a per-event breakdown, a chart with a legend of eight
categories, or a "context health score". The bar plus one sentence is the whole
feature.

## Design

Categorical series, so `--mv-cat-1` through `--mv-cat-4`, one per share. No
`--mv-warn` or `--mv-danger` anywhere: a session that is 70% tool output is not
failing, it is a session that used a lot of tools. Tabular numerals on the
percentages. The accent stays out of it entirely — nothing here is a state or a
selection.

## Open questions

- How to count a tool result that was later compacted away. It occupied the
  window while it was there, which argues for counting it, but it is not in the
  window now. `compaction.md` has the same tension.
- Whether "re-sent N times" should be estimated from position in the
  transcript, or only stated where it can be counted exactly.
- Whether injected files should separate `AGENTS.md`-class context, which is
  deliberate and per-session, from file reads, which are incidental. They are
  different decisions with different fixes.
