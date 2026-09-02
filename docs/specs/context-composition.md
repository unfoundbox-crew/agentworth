# Context composition — what fills the window before the work does

Status: built, #46. Measured against real logs, and one part of the original
idea turns out not to be measurable at all.

Working name in conversation was "chewed". This document argues for **context
composition**, on the grounds below.

## The question

> How many tokens does a model chew on tool calls and `.md` files before it
> has the actual working context in attention?

It is a good question because it is about waste rather than volume. Nobody
minds a large session; people mind a large session where most of the window
went to things that were not the task.

## What is actually measurable — corrected after being wrong once

An earlier draft of this spec claimed the fixed per-request overhead "cannot be
measured from session logs, by anyone". **That was wrong, and it was asserted
after reading 400 lines of one file.** Corrected against the full 19,955-line
log.

| Component of the window | Where it lives | Reachable? |
| :--- | :--- | :--- |
| Tool results | `ToolResult` content | yes |
| Injected files and attachments | `Custom` events, raw record verbatim | yes |
| User prompts | `UserMessage` content | yes |
| Assistant text and thinking | `AssistantMessage`, `thinking_tokens` in raw usage | yes |
| Shell output | `ShellCommand` | yes |
| **Tool names and count** | `deferred_tools_delta.addedNames`, `compactMetadata.preCompactDiscoveredTools`, `attachment.allowedTools` | **yes — names, not schemas** |
| **Total context size** | `compactMetadata.preTokens` / `postTokens` / `cumulativeDroppedTokens` | **yes, exactly, at every compaction** |
| Tool schema bodies | not recorded | no |
| System prompt text | not recorded | no |

The two rows in bold are what the earlier draft missed.

### Total context size is recorded exactly

Every `compact_boundary` record carries real numbers. From one session:

    preTokens  874,862   postTokens 15,189   dropped   859,673   tools  7
    preTokens  756,440   postTokens 22,512   dropped 1,593,601   tools 15
    preTokens  766,934   postTokens 24,019   dropped 2,336,516   tools 21
    ...
    preTokens  684,576   postTokens 21,860   dropped 5,067,603   tools 28

Eight of them in that session. This is the provider's own accounting of how
large the context actually was, not an estimate.

### So the overhead is derivable by subtraction

If the total context is known exactly and the transcript content is known,
the fixed block — system prompt plus tool schemas — is the difference.

A first check, using a crude 4-chars-per-token estimate on message content
before the first compaction:

    transcript content   3,707,156 chars  ~= 926,789 tokens (estimated)
    preTokens reported                       874,862 tokens (exact)
    difference                                -51,927

Within 6%. The estimate slightly overshoots, which is expected from
chars-over-four rather than real tokenisation. **With a proper tokeniser the
residual is the overhead**, and the tool count from
`preCompactDiscoveredTools` gives a per-tool schema cost once several sessions
with different tool counts are compared.

That is a harder measurement than reading a field, and it is derived rather
than recorded. It is not impossible, and this document should not have said it
was.

### What ships first anyway

Composition of the recorded transcript, which needs none of the above and is
the part a user can act on: a 40k file read re-sent two hundred times is a
thing you can stop doing. The derived-overhead work is a second step, and it
wants a real tokeniser before it claims a number.

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

    Transcript only. Tool schema bodies and the system prompt text are not
    recorded; total context size is, at each compaction, so the overhead can
    be derived separately.

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
