---
title: The effort dial we can't read
slug: the-effort-dial-we-cant-read
date: 2026-09-02
description: A developer report calls reasoning effort a behavioral control, not a percentage of the model. We checked the load-bearing claims against Anthropic's and OpenAI's own docs, then asked what AgentWorth can actually see about it in your transcripts today.
tags: [reasoning, effort, cost]
author: AgentWorth
---

Every major coding model now takes an effort setting: low, medium, high, and
usually more. It changes how much work the model puts into a response. It
does not change how much of the model you get.

That second sentence is the whole idea, and it is easy to get backwards. Low
effort is not "20% of the network." High is not "80% capable." Anthropic
says so directly: "Effort is a behavioral signal, not a strict token
budget." ([Effort](https://platform.claude.com/docs/en/build-with-claude/effort))
Think of it as a dial on willingness to spend inference-time compute, not a
dial on intelligence.

## What we checked

A report someone shared with us — produced with GPT — makes that same
argument at length, with diagrams and a five-level Anthropic ladder and a
comparison of who documents effort better. Per our own rule for anything
that can drift, we did not take its claims at face value. We read it in
full, then verified the three claims the rest of it leans on against the
primary docs.

**Anthropic's effort parameter has five levels, and `high` is the
default.** Confirmed directly on the current docs page:

| Level | What it does |
| :--- | :--- |
| `low` | Most efficient; real capability reduction |
| `medium` | Moderate token savings |
| `high` | Default — same as omitting the parameter |
| `xhigh` | Long-horizon agentic work, token budgets in the millions |
| `max` | No constraint on token spending |

Source: [Effort — Claude API docs](https://platform.claude.com/docs/en/build-with-claude/effort).
Not every model supports `xhigh`; some only go up to `max`.

**Effort changes tool-use behavior, not just hidden thinking.** The same
docs page states it plainly: lower effort means Claude "combines multiple
operations into fewer tool calls," "makes fewer tool calls," and "proceed[s]
directly to action without preamble." Higher effort means more calls, a
stated plan before acting, and fuller summaries afterward. This is a real
finding worth knowing if you've ever wondered why a low-effort agent run
looks terser end to end, not just in its answer.

**OpenAI's current flagship, GPT-5.6, takes the same six-name ladder:**
`none`, `low`, `medium`, `high`, `xhigh`, `max`, as separate model variants
(`-sol`, `-terra`, `-luna`) trade off capability for cost. Source:
[OpenAI's model guidance docs](https://developers.openai.com/api/docs/guides/latest-model).
OpenAI also keeps reasoning effort and answer verbosity as two separate
controls — you can ask for deep reasoning and a short answer. Source:
[OpenAI's reasoning guide](https://developers.openai.com/api/docs/guides/reasoning).

What we left out: the report's claim that Anthropic explains effort's
*meaning* more clearly while OpenAI exposes more knobs and telemetry.
That's a judgment call about two documentation styles, not a measurement,
and we're not going to launder someone else's opinion as a finding. Its
"12 mediocre iterations vs. 4 better iterations" cost chart is also labeled
illustrative in the report itself — we're repeating that label, not the bar
heights.

## What AgentWorth can see about this today

Here's the part that matters for us specifically: can we tell, from a
transcript on your machine, what effort level a session actually ran at?

Not for Claude Code. We parsed 190,573 records across 618 local session
files, and Claude Code never writes an `effort` field to disk — only the
harness that set it would know, and Claude Code doesn't log its own
request config. What Claude Code *does* write for a thinking-enabled turn
is a `thinking` content block, and that block is a summary, not the
reasoning itself: "No `display` setting returns the raw chain of thought,"
and summarization "is processed by a different model" than the one that
did the thinking. Source:
[Thinking — Claude API docs](https://platform.claude.com/docs/en/build-with-claude/thinking).
So even where we can see that a model thought, we can't see how hard.

Codex is different. Its `turn_context` record carries `effort` on every
turn, alongside `model`, `approval_policy`, and `sandbox_policy` — per-turn
harness configuration that Claude Code simply does not write. That's a
measured fact from parsing real session files, documented in
[docs/research/traces-and-open-models.md](https://github.com/unfoundbox-crew/agentworth/blob/main/docs/research/traces-and-open-models.md).

| Harness | Effort on disk? |
| :--- | :--- |
| Claude Code | No — not logged at all |
| Codex CLI | Yes — `turn_context.effort`, every turn |

So if you want to correlate effort level with outcome rate, that analysis
is available today for Codex sessions and not for Claude Code ones. We're
not going to claim otherwise, and we're not going to infer an effort level
from output length or tone — that's a guess wearing a measurement's
clothes.

## What this means day to day

We can't tell you the one right effort level, because the data we can
actually see doesn't support that sentence. What the verified claims above
do support: effort is worth setting on purpose rather than leaving on
default, because it changes real behavior — tool-call count, preamble,
summary length — not just an invisible thinking budget. If you're on
Claude Code, you're flying blind on which level you actually used unless
you set it yourself and remember; if you're on Codex, that number is
sitting in your session files right now. Everything past that — which
level is "right" for which task — is a claim about your own repo and your
own evals, not something a report, or we, can hand you.

This draws on a report produced with GPT; we verified the claims we repeat
here against the primary sources linked above.

```
npx -y agentworth scan
```
