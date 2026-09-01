# Dropped commitments

Status: proposed. Measured, not imagined — the numbers below come from one real
session in this index.

## The problem, stated by the person who has it

An agent says it will do something and then does not. It is not lying; the
intent was real when it was written and the conversation moved on. But the
commitment is now in a transcript nobody re-reads, and the only person who
could catch it is the developer, who would have to read every word.

So the failure mode is quiet. The work does not get done, nobody notices, and
if it surfaces at all it surfaces days later as "sorry, I missed that."

## It is detectable, and cheaply

Tested against a single 23 MB session, 7,144 events:

| | |
| :--- | ---: |
| Stated intents found by regex | 99 |
| Of those, no tool call and a user turn next | 55 |

A crude pattern — `I'll`, `I will`, `retrying`, `next I'll`, `I'm going to` —
split into sentences over assistant text blocks. No model, no embeddings, a few
lines of code.

Among the 55 was this, verbatim:

> Still owe you 03 and 04 over HTTP; I'll finish those next unless you want
> something else first.

That was never done. It surfaced hours later in a handoff document, and only
because the human remembered. The detector found it in under a second.

## The signal, and its false positives

The heuristic is: an assistant states an intent, emits no tool call, and the
next event is a user turn. That means the model said it would act and then
handed control back without acting.

It over-fires in three ways, all fixable:

| False positive | Example | Fix |
| :--- | :--- | :--- |
| Conditional offers | "say the word and I'll patch both" | intent is gated on a reply — detect the condition |
| Deferred by design | "I'll report when they land" | fulfilled later, possibly much later — widen the window |
| Narration of a plan | "I'll do this, then that" before doing both | the tool call is in the same turn — already excluded |

None of these are hard. The point of the measurement is that the raw signal is
strong enough to be worth refining, and that refining it needs no machine
learning.

## What makes this AgentWorth's and nobody else's

Every other tool in the observe-and-fix category watches an artefact: a stack
trace, a failing test, a diff, a dependency feed. Each of those describes
something that happened to the code.

This watches the conversation. A dropped commitment leaves no trace in the
code at all — the defect is precisely that nothing was written. There is
nothing for a linter, a test or a stack trace to find.

Only the transcript holds it, and only something reading transcripts across
sessions can notice that the same thing was promised on Tuesday and again on
Thursday and still is not done.

## What this makes the product

Not a coding harness. Not a fixer.

An archaeologist — digging through dot files nobody was ever going to read,
and returning with the one thing in them that still matters.

That is a smaller claim than opening pull requests and a much easier one to
keep.

## Why this is the exception to the triage finding

`market-autofix.md` concludes that trajectory data is real but narrow: it shows
how suspiciously code was written, not what a bug does, so it triages rather
than fixes. A stack trace still produces the better patch.

That conclusion holds everywhere except here. A dropped commitment is a defect
whose entire existence is in the transcript — the failure is that nothing was
written. There is no stack trace to want, because nothing ran. No test fails,
because the test was never added. No linter fires, because the file was never
touched.

So this is the one case where the trajectory is not a hint pointing at a better
signal. It is the whole signal, and a fix generated from it is complete rather
than speculative.

That is worth knowing before anyone builds the general version. Trajectory data
should triage. Except for the things that only ever lived in the conversation.

## What it produces

Three outputs, increasing in ambition. Ship the first.

**A list.** At session end, or on demand: here are the things this session said
it would do, and here is which ones have no evidence of happening. That alone
saves reading 7,144 events, and it is a query, not a feature.

**A carry-forward.** Dropped commitments from the last session become the first
thing the next session reads. This is the handoff file, written by the machine
that made the promises rather than by the human chasing them.

**A handoff, not a patch.** This is the part worth getting right, and the
obvious version is wrong.

The tempting move is to open a pull request. Do not. Writing the fix means
being right about the fix, from a tool that has read a transcript and not the
codebase. That is the weakest position in the whole system.

Hand the gap to the agent instead:

    Opus said it would do 5 things this session. 3 have evidence.
    Want the other 2?

      · retry `claude config` — the flag was wrong, never re-run
      · verify playgrounds 03 and 04 over HTTP

    → copy as prompt        → or point your agent at the MCP tool

The developer copies it, or their Claude Code, Codex or Cursor asks for it
directly over MCP. The agent that already has the repository open does the
work. AgentWorth never writes a line of code.

The bar drops from "be right about the patch" to "be right about what is
missing", and only the second one is answerable from a transcript. That is the
whole design.

The label stays `missed-by-opus` wherever this surfaces, and the joke is
load-bearing: a tool that names which model dropped the ball is more useful,
and more trustworthy, than one that pretends it was nobody.

## Why the joke is the right design

Attribution matters here. "Something was missed" is a shrug. "Opus said it
would do this on Tuesday at 4pm and did not" is a fact with a session id
attached, and it can be checked.

That is the same standard as the outcome ladder: a claim is worth what its
evidence is worth. A dropped-commitment report that cannot point at the exact
sentence is just another dashboard.

## Sequencing

1. Detect and list, per session. Regex, a fulfilment window, and the three
   false-positive filters above.
2. Carry forward into the next session, via the MCP server so an agent can ask
   rather than a human reading.
3. Surface it where the work happens — a copyable prompt in the dashboard, and
   the same list over MCP so an agent can pull it without a human in between.

There is no step for writing the fix. That belongs to whatever already has the
codebase open.

## Open questions

- What is the right fulfilment window? A commitment can legitimately be met
  twenty turns later.
- Should a commitment the user explicitly cancelled count as dropped? The
  cancellation is in the transcript too.
- Does dropped-commitment rate vary by model, and is that a fair thing to
  publish? It is measurable. Whether it is meaningful is a different question.
