# Beliefs

Status: proposed, measured 2026-09-02. Nothing built.

## The one-line version

AgentWorth already extracts what a session decided and verifies what it claims
it did. It never checks whether a stated fact is still true. Give it a
`claim_check` a session can call before asserting.

## The problem, stated by the person who has it

This morning a session told me a machine was asleep. It has been up for days. I
fixed it myself and said so in a session two days ago.

One session wrote the wrong fact into a rules file. The session that fixed the
machine never went back to that file. Every session since has read the file and
repeated it, and not one of them ran the one command that would have settled it.

## Why only this product can do it

The correction and the stale assertion are in the same corpus. The rules file
that carries the wrong fact was written by a session whose transcript is on
disk, and so is the turn where the fact was corrected. Nothing reads across
those two files today.

The verifier in `crates/outcomes` already does the harder half of this for
outcomes: it takes a claim of the form "I committed X" and checks it against
git. A fact about the world has no git to check against — but it has a prior
tool result, which is the same kind of receipt.

## The measurement

Four detectors, measured on this machine's index: 10,329 sessions, 4,061
readable transcripts, 4.26 GB, nine adapters.

### 1. Ungrounded claims

A claim-shaped sentence is assistant text, 25–400 characters, not a question
and not opening with a plan verb, matching a state verb (`is`, `has`, `runs`,
`exists`, `merged`, `passes`, `failed`, `broken`, …) or a number with a unit.
Measured on the 20 largest sessions of one adapter: 85,844 stream events,
**46,385 claim-shaped sentences**.

The evidence window is the tool activity in the 8 events before the sentence.

| Grounding test | Claims | Share |
| :--- | ---: | ---: |
| any tool result in the window | 46,146 | 99.5% |
| the claim's own anchor appears in the window | 10,642 | 22.9% |
| anchored, and the anchor is absent | 9,449 | 20.4% |
| no checkable anchor in the sentence at all | 26,294 | 56.7% |

**"Any tool result in the window" measures nothing.** In a tool-driven session
there is almost always one. The test only becomes a test when the claim's own
anchor — a backticked identifier, a path, a number, an issue number — has to
appear in what the window actually read.

Twenty flagged sentences, read by hand:

| Verdict | Count |
| :--- | ---: |
| genuinely unchecked | 6 |
| checked, but the evidence sits outside the 8-event window | 8 |
| not a claim about the world (a plan, a definition, an argument) | 6 |

So 30% precision, and both failure modes are fixable: widen the window to the
whole session, and drop sentences whose subject is the session's own reasoning.
Neither fix needs a model.

### 2. Beliefs that died

Five entities picked by hand — a host, a database column, a platform, a CLI
subcommand, a CI gate — each with a hand-written positive and negative pattern.
One pass over all 4,061 transcripts.

| Entity | Assertions | Sessions | Polarity flips in time order |
| :--- | ---: | ---: | ---: |
| host | 81 | 46 | 18 |
| database column | 4 | 2 | 0 |
| platform support | 11 | 6 | 3 |
| CLI subcommand | 14 | 6 | 2 |
| CI gate | 36 | 18 | 15 |
| **total** | **146** | **78** | **38** |

**The flip count is not a finding until the assertions are read.** All 81 host
rows, by hand: **31 (38%) are assertions about the host's reachability.** The
rest are instructions ("check uptime there first"), conditionals ("if it is
unreachable, stop"), questions, design prose about a UI that shows the state,
and a different entity that shares the word (the machine's GPU, an agent named
after it).

Inside the 31 real assertions the series is clean and the story is legible:
four genuine reversals over five weeks, the last one a user turn correcting the
session. **After that correction there are 8 confirmations that the fact
flipped, and 4 later sessions still asserting the dead version.**

The two rules files carrying the wrong fact were written **three minutes
before** the assertion that was already wrong, and **thirteen minutes before**
the user corrected it. Both still carry it.

Conditionals are the hard part. "If the host is unreachable, stop" and "the
host is unreachable" are the same words to a regex and opposite claims to a
reader. That is where a model would earn its place, and nowhere earlier.

### 3. Corrections landed

10,180 user-role turns across the corpus.

A loose correction regex over all of them returns **768 hits — and a 20-sample
gives 1/20 precision.** The corpus is mostly subagent briefs, which arrive as
user-role turns and are full of `no`, `not`, `again` and `stop`. This is the
same trap `suspect-commits.md` hit: the obvious join flags a third of
everything and nine in ten are false.

Restricting to turns under 400 characters with the marker at a sentence start:

| | |
| :--- | ---: |
| corrections | 49 |
| sessions | 33 |
| share of short user turns | 0.8% |
| hand-labelled precision (all 49 read) | 42/49, 86% |
| of those, corrections of a *fact* | 7 |
| of those, corrections of the *work* | 35 |

Landing, measured as a rules-or-docs edit after the correction:

| Test | Count | Share |
| :--- | ---: | ---: |
| a rules-file edit in the same session, within 24h | 17 | 35% |
| a rules-file edit in any session, within 24h | 27 | 55% |
| a docs-or-rules edit in the same session, within 24h | 41 | 84% |

**The 84% is a coincidence, not a finding.** Sessions edit markdown constantly;
proximity in time proves nothing about content. Only the same-session
rules-file number is worth quoting, and even that cannot show the edit fixed
the thing that was corrected — `file_modifications` stores a path and a time,
not a diff.

**The motivating correction matches none of these patterns.** It reads like a
statement, not a rebuttal: the fact, then why it changed. The highest-value
correction in the corpus carries no correction marker at all.

### 4. Things you have had to say N times

933 rule-shaped user turns (containing `never`, `don't`, `always`, `must`,
`stop`, `from now on`) across 308 sessions.

| Method | Result |
| :--- | :--- |
| exact match on normalised text | 5,419 turns → 3,939 distinct; the top repeats are harness boilerplate, not instructions |
| token-overlap clustering at 0.6 Jaccard | largest real cluster: 10 members |
| content-word clustering | returns topics, not rules |

**This one does not work and should not ship.** A person phrases the same
standing rule differently every time. The lexical signal is the rule's subject,
which clusters into topics far too broad to point at a document. Revisit only
with embeddings, and only after `local-search.md`.

## The MCP tools

Pull only. No hooks, no per-session feed — a session asks when it is about to
assert, the way it already asks `forgotten_context` when it is about to plan.

    claim_check(text, entity?, since?)

Takes a sentence the caller is about to say. Extracts its anchors, searches the
corpus for prior assertions and tool results carrying the same anchors, and
returns what it finds.

| Param | Type | Default |
| :--- | :--- | :--- |
| `text` | string, the claim | required |
| `entity` | string, an explicit anchor overriding extraction | extracted |
| `since` | ISO date | 90 days |

```json
{"claim": "<the caller's sentence>",
 "anchors": ["<host>"],
 "verdict": "contradicted",
 "last_supporting": null,
 "last_contradicting": {
   "kind": "tool_result",
   "session_id": "af994f5e-…", "sequence": 2211,
   "timestamp": "2026-09-01T11:47Z",
   "excerpt": "<the command and its first line of output>"},
 "assertions": [
   {"polarity": "negative", "timestamp": "2026-08-31T05:45Z",
    "session_id": "020e9fb4-…", "sequence": 1180,
    "text": "<verbatim>"},
   {"polarity": "positive", "timestamp": "2026-08-31T05:55Z",
    "session_id": "020e9fb4-…", "sequence": 1204, "role": "user",
    "text": "<verbatim>"}],
 "receipt": {"anchors_matched": 1, "transcripts_searched": 412,
             "method": "regex_v1", "no_model": true}}
```

`verdict` is one of `contradicted`, `supported`, `mixed`, or
**`no_record_either_way`** — and the last one is the common case, not an error.
A tool that answers "I have never seen this claim before" is telling the truth
and is still worth calling.

A user-role assertion outranks an assistant one. When the person says the fact
changed, that is the newest evidence there is.

Three list tools, same shape as the built ones:

| Tool | Returns |
| :--- | :--- |
| `beliefs_died(repo?, since?)` | entities whose asserted polarity reversed, newest first |
| `corrections(since?)` | user corrections, each with whether a rules file was edited in the same session afterwards |
| `repeated_instructions(limit)` | **not built.** See detector 4 |

The header line a session actually reads:

    You are about to assert something the corpus contradicts — last checked
    18 hours ago, opposite result, in another session.

## Deliberately not built

- **No hooks and no push.** Nothing injects a warning into a running session.
  A tool that interrupts on a 30%-precision detector is worse than no tool.
- **No writing to rules files.** `corrections` reports that a correction never
  landed. It does not land it. The wrong fact and the right one look identical
  to a regex, and the file is the one artifact everything downstream trusts.
- **No model in v1**, for the reason `compaction-diff.md` gives: the output
  goes to an agent that cannot verify it, so the receipt must point at words
  someone actually said.
- **No `repeated_instructions`.** Measured, does not work, argued above.
- **No cross-machine anything.** Local-only, unchanged.

## Sequencing

1. **`claim_check` first**, because it is a lookup and not a detector. The
   caller supplies the claim, so the tool inherits none of detector 1's 30%
   precision — it only has to find prior evidence about the anchors it was
   given, which is exact match over a corpus AgentWorth already indexes.
2. **`corrections` second**, at 86% precision on the tightened pattern. Ship it
   as a list tool with the landing column, and expect the landing number to
   stay low until a person looks at it.
3. **`beliefs_died` third**, and only after the conditional-versus-assertion
   split has a measurement. 38% precision is not shippable.
4. **Detector 1 as a scan**, last, and only once the window is the whole
   session and non-claims are filtered out. Re-measure precision on 20 before
   deciding.

Against the "Next: usefulness" table: this is **F**, after **D**. It has the
same shape as D — a stated claim cross-checked against ground truth — and it
reuses D's absolute-path anchoring for any claim about a file. C and E are
built and this leans on both: E already proves a session forgets what it
decided, and F says it also keeps what stopped being true.

## Open questions

- What is the right evidence window? Eight events is too narrow — 8 of 20
  hand-checked flags had their receipt further back. The whole session is the
  obvious ceiling and may be too generous.
- Can a conditional be told from an assertion without a model? It is the single
  biggest error source in detector 2 and the cheapest thing a model would fix.
- How does a claim get an identity across sessions when the anchor is a common
  word? Anchors that are paths and issue numbers resolve cleanly. Anchors that
  are bare nouns do not.
- Should `claim_check` search other people's transcripts on the same machine?
  Everything here is one person's corpus. Nothing in the design needs that to
  stay true, and nothing has been thought through about what happens if it
  does not.
- Does the corpus contain the fix for a stale fact more often than it contains
  the staleness? Measured once, on one entity, the answer was yes by 8 to 4.
  One entity is not a measurement.
