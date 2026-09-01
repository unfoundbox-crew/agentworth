# Compaction, and what it costs

Measured on a real index of 612 sessions, not estimated.

## What compaction is

When a conversation gets too long for the model's context window, the tool
summarises everything so far and starts again from that summary. The full
transcript stays on disk. The model's view of it does not.

That is the whole mechanism. A summary replaces the conversation, and the
conversation is gone as far as the model is concerned.

## What it costs

One real session, 68.6 MB, 19,955 events, compacted **eight** times:

| Round | Context before | Summary after | Survived |
| ---: | ---: | ---: | ---: |
| 1 | 6,516,597 | 27,542 | 0.42% |
| 2 | 13,722,993 | 28,598 | 0.21% |
| 3 | 10,877,115 | 27,913 | 0.26% |
| 4 | 5,533,358 | 28,598 | 0.52% |
| 5 | 9,124,858 | 28,331 | 0.31% |
| 6 | 5,830,258 | 26,967 | 0.46% |
| 7 | 10,229,258 | 33,979 | 0.33% |
| 8 | 8,537,298 | 28,863 | 0.34% |

Every round keeps about a third of one percent.

The number that matters is not any single row. It is that **round 8 is
summarising a summary of a summary, seven layers deep.** Content from early in
that session has been rewritten eight times. Nothing of the original wording
survives, and each rewrite was made by something that could not know which
detail would matter later.

By the end, the model is working from roughly 28 KB of a 68.6 MB session.
About 0.04%.

## What survives, and what does not

Summaries are good at conclusions and bad at reasons.

| Usually survives | Usually lost |
| :--- | :--- |
| what was decided | why, and what was rejected |
| which files were touched | exact paths and line numbers |
| that something failed | the actual error text |
| the current task | how the last three attempts went wrong |

That asymmetry is what makes repeated compaction expensive. A session can know
it chose an approach and have no idea it already tried the alternative and
watched it fail.

## How common is this

| | |
| :--- | ---: |
| Sessions over 50 KB | 543 |
| Compacted at least once | 22 |
| Never compacted | 521 |
| Most compactions in one session | 8 |
| Median size, compacted | 23.2 MB |
| Median size, never compacted | 0.3 MB |

Compaction is rare, and when it happens the session is roughly seventy times
larger than a normal one. It is not a routine event. It is what happens when a
session should have ended and did not.

## What to do instead

**Do not auto-compact every few turns.** That is the worst option available.
Each round is a lossy generation, so compacting often means many generations,
each one summarising something already summarised.

| Situation | Do |
| :--- | :--- |
| One thread, finishing soon | Let it run. One compaction is survivable |
| Multi-day work | Write a handoff and start fresh |
| Already compacted twice | Start fresh. You are reading a summary of a summary |

A written handoff beats compaction at the same job, for four reasons:

- You choose what is kept, not a summariser working without knowing the future.
- It can be re-read. A summary is consumed once and cannot be consulted again.
- It survives to the next session, and the one after.
- It does not degrade. Compaction loses a third of a percent each time; a file
  loses nothing.

The tradeoff is honest: a handoff costs you ten minutes of writing. Compaction
costs nothing up front and takes it later, in a form you will not notice —
a session confidently repeating work it already did.

## The feature this suggests

Compaction is recorded in the session logs. Claude Code writes
`"isCompactSummary": true` on the summary event and
`"compactMetadata": {"trigger": ...}` alongside it, so whether it was manual or
automatic is knowable.

That means AgentWorth can show what nothing else does:

**Per session** — how many times it was compacted, and how much context each
round dropped. A session compacted five times is a different object from one
that never was, and right now they look identical in every tool.

**Across sessions** — whether compacted sessions reach lower outcome rungs than
uncompacted ones. That is a real question with a real answer sitting in the
data, and nobody has checked. If a session that has been compacted three times
is measurably worse at landing a commit, that is worth knowing before the third
compaction rather than after.

**As a warning** — the point of measuring it is to act earlier. Not "you have
compacted eight times", which is too late, but "sessions like this one start
losing the thread around here."

Why AgentWorth and not the harness: Claude Code sees one session and has no
basis for comparison. AgentWorth reads every session across every harness, so
it can say what typical looks like. That is the same argument as the rest of
this product — a tool cannot audit itself, and comparison needs someone
standing outside.

## Open questions

- Does compaction actually correlate with worse outcomes? Assumed here, unmeasured.
- Is manual compaction different from automatic in effect? The trigger is recorded.
- What is the right warning threshold, and does it differ by session type?
