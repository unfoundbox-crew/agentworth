# The questions worth answering

Status: proposed. This is not a UI spec. It is the list of questions the data
can already answer and nobody has asked, plus the method for finding more.

Build the answers before designing screens for them. Most of these will turn
out to be boring, and finding out which ones are boring is cheap.

## What kind of question this is

Every useful question here is **conditional and comparative**. Not "how many
tokens did I use" but "does this get worse after that."

A number alone means nothing. 68% cache warmth is neither good nor bad until it
is compared to your own average, to last month, or to a session that went well.
Every question below is a comparison in disguise.

## The method

Cross every dimension against every measure. Most cells are unasked.

Dimensions available today, or derivable from `source_path` with no new
plumbing:

    model · adapter · repo · worktree · orchestrator vs executor
    compaction count · turn position · time of day · fan-out size
    session length · first-prompt length

Measures available today:

    outcome rung · composite score · tokens · duration · cache warmth
    recovery count · tool call count · files changed

That is roughly eleven by eight. Three cells have been asked. The rest are
open.

## The questions

Ordered by how cheap they are to answer, not by how interesting they sound.
Cheap and boring beats expensive and speculative, because you find out sooner.

### Answerable now, no new indexing

| Question | Why it might matter |
| :--- | :--- |
| Does outcome degrade by **hour of day**? | You are comparing yourself at 11am to yourself at 2am. Nobody has checked. |
| Does **fan-out size** predict success? Is 29 subagents worse than 5? | Real sessions in this index fan out to 29. If large fan-outs land lower, that is directly actionable. |
| Do sessions in **worktrees** score differently from the shared checkout? | Tests whether the isolation discipline actually pays, rather than assuming it. |
| Does **first-prompt length** predict the outcome rung? | The cheapest possible intervention, because it is the one thing entirely under the user's control before anything runs. |
| Which **adapter** recovers best from failure? | `recoveries` is indexed and nothing reads it. |
| Do **long sessions** produce less verified work than short ones? | Uncomfortable if true, which is what makes it worth asking. |

### Needs one new field

| Question | Needs |
| :--- | :--- |
| Does performance drop after **N compactions**? | compaction count — see `compaction.md`, the flag is already in the logs |
| Does **cache warmth** correlate with outcome? | warmth — see `cache-economics.md`, derivable from data already parsed |
| Which models **orchestrate** well? | fan-out-weighted score, described below |
| Which models **execute** well? | subagent model plus its own rung, derivable from `source_path` today |

## Two of these need their measure defined carefully

**Orchestration is not measured by the orchestrator's own score.** A parent
session barely touches files, so it will always score low on evidence. The
right measure is whether its subagents landed evidence — a fan-out-weighted
outcome. A good director is one whose 29 subagents reach rung 4, not one who
wrote a tidy plan. Nothing computes this today.

**Execution is the cleaner experiment.** Subagent sessions each record their
model and receive their own outcome rung. Same task shape, different models,
which is close to a controlled comparison. This is the one most likely to
produce a real finding.

## The shift that matters more than any single question

Everything above is **descriptive** — what happened. The value is in the same
data pointed forward:

> You have compacted twice, it is 1am, and this session has run four hours.
> Sessions shaped like this one land on rung 1.

That is the difference between a dashboard and something that changes a day.
It needs no new data, only the confidence to say it — which means answering
the descriptive questions first and finding out which correlations are real.

Do not build the prescriptive version until at least one correlation survives
scrutiny. A warning based on a pattern that turns out to be noise is worse than
no warning, because people act on it.

## An honest limit

This is one developer's data. Twenty-two compacted sessions, three orchestrator
sessions in a fifty-session sample. That is personal analytics, not a benchmark.

"Sonnet executes better than Opus" cannot be concluded from one person's
habits, tasks and prompting style. It becomes a benchmark only if the data
pools across many people, which is a different product with different consent
questions.

Be clear which one is being built. The personal version is genuinely useful and
ships now. The benchmark version is a claim about models and needs far more
before it can be made.

## How to find more questions

Three prompts that reliably produce good ones:

**Compared to what?** Any metric without a baseline is decoration.

**What would embarrass me if true?** Good questions have uncomfortable answers.
*Do my longest sessions produce the least verified work?* is a better question
than *how many sessions did I run*, precisely because the answer might sting.

**What can only I see?** Anything answerable inside a single session belongs to
the harness that ran it. This project's questions are the ones that need
several sessions, several tools, or the gaps between them.

## Open questions

- Which of these correlations are real? All of them are assumed here, none measured.
- What sample size makes a finding trustworthy for one user's data?
- Should a finding that fails to replicate be shown at all, and how?
