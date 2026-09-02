# Compaction diff

Status: proposed, measured 2026-09-02.

## The one-line version

AgentWorth holds the transcript compaction threw away. Diff it against the
summary that replaced it, and hand back the decisions the session no longer
remembers making.

## The problem, stated by the person who has it

The session confidently re-proposes something it already tried and rejected
three hours ago. I remember the rejection. It does not, because the turn where
it happened was summarised away.

Then I retype the reason. Then it happens again.

## Why only this product can do it

Compaction is destructive from inside the harness and lossless from outside.
The model's view is replaced by a summary; the full JSONL is untouched on disk.
So the pre-compaction span and the summary that replaced it both exist, side by
side, in a file nothing reads.

`compaction.md` measured the loss in bytes: about a third of one percent
survives per round, eight rounds deep, 28 KB left of a 68.6 MB session. It also
asserted an asymmetry — summaries keep conclusions and lose reasons — and
marked it as an observation, not a measurement.

This spec measures it.

## The measurement

One session, `452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd`, 19,955 JSONL lines,
29,642 indexed events, 2.99M tokens, eight compaction rounds. Compaction is
found by `isCompactSummary` / `compactMetadata` on the summary line; each round
appears as a marker line followed by the summary itself.

A decision-shaped sentence is assistant text, 25–400 characters, matching one
of three patterns. Deterministic, no model:

| Class | Pattern |
| :--- | :--- |
| decision | `decided`, `decide to`, `chose`, `choosing`, `opted for` |
| rejected | `instead of`, `rejected`, `ruled out`, `won't`, `will not`, `not going to` |
| reason | `because` |

For each round, count matches in the span about to be dropped, and in the
summary that replaces it.

| round | dropped: dec / rej / why | summary: dec / rej / why |
| ---: | ---: | ---: |
| 1 | 6 / 14 / 19 | 0 / 4 / 0 |
| 2 | 0 / 28 / 35 | 0 / 3 / 0 |
| 3 | 6 / 29 / 10 | 0 / 0 / 1 |
| 4 | 8 / 31 / 21 | 0 / 3 / 0 |
| 5 | 6 / 29 / 10 | 1 / 3 / 1 |
| 6 | 1 / 19 / 19 | 1 / 1 / 1 |
| 7 | 15 / 13 / 42 | 1 / 1 / 0 |
| 8 | 10 / 19 / 18 | 5 / 2 / 0 |
| **total** | **52 / 182 / 174** | **8 / 17 / 3** |

Survival rate: decisions 15.4%, rejected alternatives 9.3%, **reasons 1.7%**.

Counting distinct sentences rather than class matches, since one sentence can
hit two patterns: **402 decision-shaped sentences went into the eight rounds
and 28 came out. 374 the session decided something in and no longer has.**

**The asymmetry is real and it is worse than stated.** 174 sentences explaining
why something was done went into compaction. Three came out. A session that has
compacted keeps roughly one in six of its conclusions and one in fifty-eight of
its reasons — which is exactly the shape that makes it re-litigate a settled
question, because it kept the answer's shadow and lost the argument.

Round 2 is the clearest single row: 63 decision-shaped sentences in, three out,
and not one of them a reason.

## Scope

Compaction is rare. `compaction.md` measured 22 of 543 sessions over 50 KB
compacted at least once, at a median 23.2 MB against 0.3 MB for the rest. This
tool serves the 4% of sessions that are 70× the size of a normal one — which is
also the 4% where a day of work is at stake.

The index copy used here predates #62, so it has no `compaction_count` column;
the numbers above come from the raw JSONL, not from SQLite. On a current index
`compaction_count > 0` selects the population directly.

## The MCP tool

    forgotten_context(session_id?, round?, classes?, limit?)

| Param | Type | Default |
| :--- | :--- | :--- |
| `session_id` | string | the caller's most recent session for its cwd |
| `round` | integer, 1-based | all rounds |
| `classes` | subset of `decision` \| `rejected` \| `reason` | all three |
| `limit` | integer | 40, ceiling 200 |

Returns:

```json
{"session_id": "452c23fd-…", "rounds": 8,
 "dropped_total": 402, "survived_in_summary": 28,
 "forgotten": [
   {"class": "rejected", "round": 3,
    "text": "Going with a marker table instead of a second pass — the second
             pass re-reads 68 MB for one boolean.",
    "sequence": 8441, "timestamp": "2026-09-01T15:12:03Z",
    "model": "claude-opus-5",
    "followed_by": ["tool_call:Edit crates/storage/src/lib.rs",
                    "shell_command:cargo test -p agentworth-storage"]}],
 "receipt": {"source_path": "~/.claude/projects/…/452c23fd-….jsonl",
             "extracted_at": "2026-09-02T09:41Z",
             "method": "regex_v1", "no_model": true}}
```

`followed_by` is what makes a sentence checkable. A stated decision with a tool
call after it was acted on; one with nothing after it is a claim. Both are
returned, labelled, and the caller decides.

The header line a session actually reads:

    Things you decided and no longer remember — 374 sentences dropped
    across 8 compaction rounds, 28 survived in the summaries.

**The "I don't know" cases, all three of them:**

- Session never compacted → `{"rounds": 0, "forgotten": []}`. Not an error, and
  not an empty answer dressed as a finding.
- Session compacted but no pattern matched → `forgotten: []` with
  `dropped_total: 0` and `method: "regex_v1"` in the receipt, so the caller can
  tell "nothing was decided" from "the regex found nothing."
- Raw JSONL missing or unreadable → refuse. `sessions.source_path` can point at
  a file that has since been deleted; returning a partial diff from an index
  row would be inventing content.

## No model, on purpose

A model could extract decisions better than three regexes. It is still the
wrong v1.

The output is fed to an agent that cannot verify it. If a model paraphrases the
dropped span, the tool becomes a second summariser — the exact lossy step this
spec exists to undo — and the receipt stops pointing at words anyone said. A
regex returns the sentence verbatim with a sequence number, which is a quotable
fact. 374 verbatim sentences with false positives in them beats 40 fluent ones
nobody can check.

Revisit when there is a measured precision number for the regex to beat.

## New work

1. Compaction round boundaries as a stored artifact. `compaction_count` and
   `compaction_tokens_dropped` exist since #62; the line offsets of each round
   do not, and re-scanning a 68 MB JSONL per call is not acceptable. One table:
   `session_compaction(session_id, round, start_seq, end_seq, summary_seq,
   tokens_before, summary_tokens)`.
2. The extractor, in `agentworth-outcomes` beside the loose-ends detector. Same
   sentence splitter, same length bounds — one implementation, not two.
3. The MCP tool.

Extraction runs on demand from the raw trace, not at scan time. Storing 402
sentences per compacted session in SQLite would duplicate transcript content
into the index, which `AGENTS.md` forbids.

## Deliberately not built

- **No model in v1.** Argued above.
- **No re-injection.** The tool returns sentences. It does not write them into
  a context, a `CLAUDE.md`, or a prompt.
- **No cross-session diff.** One session's own compaction rounds. "What did the
  last session decide" is `carry_forward` in `handoff.md`.
- **No warning before compaction.** `compaction.md` is right that the useful
  version is a warning, and right that it needs a correlation nobody has
  measured yet.

## Sequencing

1. `session_compaction` boundaries, written by the scanner.
2. The extractor plus a fixture test on a small compacted session.
3. The MCP tool, regex only.
4. Measure precision by hand on 50 returned sentences. Only then consider a
   model, and only if the number is bad.

## Open questions

- What is the false-positive rate of `because`? 174 matches in one session is a
  lot, and "because" appears in narration as readily as in reasoning.
- Do the summaries drop reasons, or do the models simply state reasons more
  often in the dropped spans than the summariser has room for? The measurement
  above cannot separate those.
- Should a decision that was later reversed still be returned? It is forgotten
  either way, and returning it might re-suggest a dead end.
- Is 400 characters the right sentence ceiling? A long decision paragraph is
  the most valuable thing here and the current bound excludes it.
