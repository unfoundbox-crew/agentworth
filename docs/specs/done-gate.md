# The done gate

Status: proposed, measured 2026-09-03.

## The one-line version

When a harness's agent is about to stop and say done, a plugin asks
AgentWorth what evidence this session actually left, and if the answer is
below the evidence line it hands the receipt and the one missing condition
back into the loop instead of letting the turn end silently.

## What it is, in five lines

1. The agent finishes its last tool call and the turn is about to close.
2. The harness fires a stop event. A plugin catches it.
3. The plugin calls one AgentWorth tool over MCP. No model, no network.
4. Below the line: the plugin returns six lines of receipt and one missing
   condition, and the harness runs another step.
5. Above the line, out of budget, or the index cannot answer: it stops.

## Why this and not a coding harness

AgentWorth is a cockpit above harnesses, never a harness (`AGENTS.md`). The
gate is what that sentence looks like when it reaches a user: Archie does not
run the loop, he stands at the exit of somebody else's loop and asks for the
receipt. Every harness already has that exit as a public extension point, and
two of them are quoted below.

`convergence.md` measured why a gate and not a stop switch. A threshold on
turns-since-progress kills working sessions at almost exactly the rate it
saves tail spend — there is no knee in that curve, so nothing here ever ends
a session. This is the opposite motion: it *prevents* an ending, once, cheaply,
and only when the ladder says the session left nothing behind.

## The measurement

Saurabh's own index, his sessions, one machine. `~/.agentworth/agentworth.db`
read-only on 2026-09-03: 4,887 session rows, 3,178 non-stub
(`total_events > 1 AND total_tokens > 0`), 2,103 of those carrying a
`primary_outcome`. His data, counted on his machine, and it never left it.
Queries in the appendix.

Two definitions inherited from `archie-bench.md` so the numbers stay
comparable. The denominator is a non-stub session with a non-null
`primary_outcome` — a session that claimed done at all. **Below the evidence
line** is rung 1 or 2, `done_claimed` or `artifact_changed`; verified is rung 3
or higher.

`archie stats ladder --json` could not be used: the shipped binary on this
machine is 0.1.15 and `stats ladder` landed in #124 (0.1.17). Everything below
is SQL against the same columns that command reads, plus one raw-transcript
walk for section 3.

### 1. How often the gate would have fired

| | sessions that claimed done | below the line | share |
| :--- | ---: | ---: | ---: |
| all time | 2,103 | 264 | **12.6%** |
| last 30 days | 1,351 | 152 | 11.3% |

By adapter. Only two adapters produce outcomes at all, which
`capability-matrix.md` already says and this inherits:

| adapter | n | `done_claimed` | `artifact_changed` | below | share |
| :--- | ---: | ---: | ---: | ---: | ---: |
| claude_code, all time | 2,065 | 4 | 241 | 245 | 11.9% |
| claude_code, last 30d | 1,313 | — | — | 133 | 10.1% |
| opencode, all time | 38 | 0 | 19 | 19 | 50.0% |
| opencode, last 30d | 38 | 0 | 19 | 19 | 50.0% |

Read the opencode row as 19 of 38, not as a rate. It clears no confidence
floor (`min_n` is 20, `agentworth_storage::OUTCOME_RATE_DEFAULT_MIN_N`) and it
is printed with its `n` for that reason.

**Rung 1 is nearly empty and rung 2 is where the gate lives.** Four sessions
in the whole index stopped at `done_claimed`. 241 stopped at
`artifact_changed` — files were written and nothing ever checked them. A gate
built to catch the agent that says "done" with no work behind it would fire
four times in three months. A gate built to catch the agent that *wrote code*
and never ran anything fires 245 times.

By repo, every group clearing n>=20, 17 of 44 groups covering 2,024 of the
2,103 sessions (96.2%):

| repo | n | below | share |
| :--- | ---: | ---: | ---: |
| katana/video | 30 | 18 | 60.0% |
| Users/saurabh | 36 | 15 | 41.7% |
| upscaler/backend | 291 | 55 | 18.9% |
| motionvector/pluto | 84 | 12 | 14.3% |
| code/motionvector | 378 | 49 | 13.0% |
| motionvector/studio | 84 | 10 | 11.9% |
| unfoundbox/memes | 38 | 4 | 10.5% |
| katana/upscaler | 143 | 15 | 10.5% |
| apps/vibelaunch | 288 | 30 | 10.4% |
| motionvector/motionvector | 34 | 3 | 8.8% |
| motionvector/spacepilot | 97 | 8 | 8.2% |
| unfoundbox/agentworth | 102 | 6 | 5.9% |
| saurabh/code | 95 | 5 | 5.3% |
| upscaler/frontend | 148 | 7 | 4.7% |
| tinkers/blog | 26 | 1 | 3.8% |
| mvec/engine | 109 | 4 | 3.7% |
| video/frontend | 41 | 0 | 0.0% |

Sixty points of spread, and repo is again the axis that moves — the same
finding `verified-outcome-rate.md` and `archie-bench.md` both landed on.
`Users/saurabh` is not a repo; it is what `extract_repository_or_workspace`
returns for a session started outside one, and it stays in the table because
hiding it would flatter the spread.

### 2. What the gate would cost, and what it could save

For the sessions it would fire on, the whole session's tokens are the **upper
bound** on what a working gate could have saved. That is a ceiling and not an
estimate: the index has no events table, so nothing here can locate the moment
the agent first claimed done, and most of a below-the-line session's spend
happened before any gate could speak. `convergence.md` reached the same wall
and walked the transcripts; this does not, because the ceiling already answers
the question.

| band | sessions | tokens | share of tokens |
| :--- | ---: | ---: | ---: |
| at or above the line | 1,839 | 96.8B | 97.5% |
| below the line | 264 | 2.5B | **2.5%** |

**This is the number that decides what the gate is for.** Every session it
could ever fire on holds 2.5% of the tokens in the corpus. The gate is not a
cost-saving device and this spec does not sell it as one — it is an evidence
device that happens to be cheap. Anyone reading section 1's 12.6% as "an
eighth of my spend" has the wrong denominator.

Below-the-line sessions are also small. Session size, below the line against
at or above it:

| | p50 tokens | p75 | p90 |
| :--- | ---: | ---: | ---: |
| below the line (n=264) | 3.4M | 10.3M | 22.2M |
| at or above (n=1,839) | 9.1M | — | 98.5M |

Turns in a below-the-line session: p50 23, p75 47, p90 91 as the index counts
messages; p50 72, p75 148, p90 252 walking the transcript, which counts every
record and not only user-visible turns. Both are printed because they answer
different questions and neither is wrong.

**The re-entry budget has to be smaller than the room a session normally has
left.** `convergence.md` measured, over 1,072 claude_code sessions that reached
a verified progress, that the gaps which *ended* in progress run p50 9 turns,
p75 28, p90 67 — that is how long it takes a working session to get from
nothing to evidence. A budget of two or three re-entries costs a fraction of
one such gap. Restated from that spec, not re-measured here.

### 3. What the gate can say

Walking the raw transcripts of all 245 below-the-line claude_code sessions,
with the command predicates ported from this repo's own Rust so the answer
means what the product means (`crates/outcomes/src/outcome.rs` for the
test/build, commit and CI command sets and `has_test_failure_markers`;
`crates/adapters/src/exit_status.rs` for `exit_code_from_result`). Lint is not
in the Rust classifier and was counted separately, the way `convergence.md`
does it. All 245 files were present and parsed.

| what the gate would name as missing | sessions | share |
| :--- | ---: | ---: |
| no gate command ran at all | 245 | **100.0%** |
| a gate ran and none of them passed | 0 | 0.0% |

| what ran at all, any exit code | sessions | share |
| :--- | ---: | ---: |
| a test or build command | 0 | 0.0% |
| a lint command | 0 | 0.0% |
| a commit command | 0 | 0.0% |
| a CI or deploy command | 0 | 0.0% |
| any Bash call at all | 212 | 86.5% |

4,541 Bash calls across those 245 sessions, and not one of them matched a
test, build, lint, commit, or deploy pattern. The first words are `cd` (1,829),
`grep` (320), `echo` (223), `ls` (220), `ssh` (142).

**Half of this is definitional and half is not, and the half that is not is
the finding.** A gate that exited 0 would have lifted the session to rung 3, so
of course none is found below the line. But "a gate ran and failed" also lands
below the line, and it happens zero times in 245 sessions. So does a commit,
and so does a deploy. These sessions did not try and fall short. They read,
searched, moved around the tree, wrote files, and stopped.

The gate therefore has exactly one thing to say, always the same thing, and it
should say it in one line rather than shipping a decision tree for branches
that have never fired on this machine. Confidence: **moderate** — one machine,
one adapter for 245 of 264 sessions, and a definition of a gate limited to
shell commands. `convergence.md`'s open question applies unchanged: a frontend
repo whose gate is a browser leaves no shell command and reads as naked when
it is not.

### 4. Sessions with a gate available that never ran it

Restated from `convergence.md`'s coverage table, measured 2026-09-02 over
3,046 claude_code sessions, filtered here to an n floor of 20 rather than that
spec's 10. One group drops ("a second client repo", n=11); 21 survive. The
naked share is the share that ran no test, build, or lint command at all:

| repo | n | naked |
| :--- | ---: | ---: |
| apps/studio | 49 | 100.0% |
| apps/learn | 21 | 100.0% |
| tinkers/blog | 45 | 95.6% |
| resolution/lab | 28 | 92.9% |
| Users/saurabh | 32 | 84.4% |
| upscaler/backend | 421 | 81.7% |
| motionvector/pluto | 178 | 79.8% |
| motionvector/spacepilot | 144 | 75.0% |
| unfoundbox/memes | 46 | 71.7% |
| motionvector/studio | 179 | 69.8% |
| katana/video | 43 | 69.8% |
| saurabh/code | 123 | 65.9% |
| upscaler/free | 23 | 65.2% |
| motionvector/motionvector | 51 | 64.7% |
| apps/vibelaunch | 441 | 64.6% |
| code/motionvector | 450 | 58.2% |
| a client repo, not ours | 222 | 56.3% |
| mvec/engine | 144 | 52.8% |
| upscaler/frontend | 207 | 46.4% |
| video/frontend | 58 | 39.7% |
| unfoundbox/agentworth | 80 | 33.8% |

Pooled, 66.1% of claude_code sessions ran no gate. **The gate cannot reach most
of them and should not pretend to.** A naked session usually never claimed done
in the ladder's sense either, so it carries no `primary_outcome` and never
reaches the gate's denominator. The two tables measure different populations:
this one says most sessions have no verifier at all, section 1 says an eighth
of the ones that do claim something claim too little. The first is a coverage
problem `convergence.md`'s snippet already addresses in prose. The second is
what this gate is for.

### 5. Subagents

| | sessions claiming done | below the line | share |
| :--- | ---: | ---: | ---: |
| main sessions | 355 | 62 | 17.5% |
| subagent runs | 1,748 | 202 | 11.6% |

A main session is likelier to stop below the line, but **83% of all
below-the-line sessions are subagent runs** (202 of 264). A gate wired only to
the main stop event would miss four in five of the sessions it was built for.
Both harnesses expose a separate subagent stop event and section "The three
plugin bodies" covers both; whether to fire on it by default is the open
question at the end.

## The contract

### Input: which session is this

The gate has to turn "the harness is stopping" into a session id AgentWorth
knows. Both harnesses hand it something to work with, and they do not hand it
the same thing.

**Claude Code** puts the transcript file on stdin. Quoting the official docs
(`https://code.claude.com/docs/en/hooks.md`), the `Stop` event's input carries
`session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`,
`stop_hook_active`, `last_assistant_message`, `background_tasks`, and
`session_crons`, with `transcript_path` shown in the doc's own example as
`~/.claude/projects/.../00893aaf-19fa-41d2-8238-13269b9b3ca0.jsonl`. That is
exactly the path AgentWorth's claude_code adapter indexes, so `session_id` and
`transcript_path` both resolve directly.

**dsh** does not. Its Codex compatibility layer states plainly that
`transcript_path` "is always `null`, because the persistence seam exposes no
artifact paths and the default-zstd session log is not readable by hook
scripts" (`packages/hooks/hooks-codex/README.md:176`). So a dsh plugin resolves
its own session from the persistence config it can see — the JSONL plugin's
`root` is required and has no default, and the layout is
`<root>/--<normalized-cwd>--/<encoded-id>/session.jsonl.zstd`
(`packages/session/session-persistence-jsonl/README.md`) — or it passes `cwd`
and lets AgentWorth resolve the most recent indexed session for that
repository, which is the default `session_handoff` and `session_forgotten`
already use.

### The call: one new tool

The existing tools each answer a different question. `stats_ladder` is an
aggregate over a population; `session_show` returns a trace; `session_handoff`
renders a document. None of them answers "where on the ladder is this one
session, right now, and what is the single next rung" in a shape a plugin can
act on in under a second. So: **one new read-only MCP tool, `session_gate`.**

| Param | Required | Type | Description |
| :--- | :--- | :--- | :--- |
| `session_id` | no | string or null | Defaults to the most recent indexed session for the repository the server process is running in, the same default `session_handoff` uses |
| `cwd` | no | string or null | Resolve the session by working directory instead, for a harness that exposes no transcript path |
| `budget_used` | no | integer | How many times this gate has already re-entered this session. Defaults to 0 |

Returns:

```json
{
  "rung": 2,
  "rung_name": "artifact_changed",
  "below_line": true,
  "missing": "no test, build, or lint command ran in this session",
  "receipt_lines": ["...", "..."],
  "budget_left": 2,
  "stale": false,
  "indexed": true
}
```

`rung` is the ladder position `AGENTS.md` already defines and `stats_ladder`
already computes; nothing here re-types the CASE ladder. `missing` names the
one condition, drawn from what the transcript shows, in the same vocabulary
`convergence.md`'s `session_explain` uses. `receipt_lines` are pre-rendered by
AgentWorth so there is one rendering path, the rule `cli-grammar.md` sets.

**It is a pull tool.** No hooks inside AgentWorth, no per-turn feed, no push.
The plugin calls it at one moment for one reason, the way `repeat_check` is
called before one re-read (`efficiency-receipts.md`). A tool that tells an
agent to prove more work, and expands its context while doing so, has lost the
argument — so the answer is capped at six lines.

### Output: what goes back into the loop

At most six lines, in the receipt shape, glyph set only (`docs/DESIGN.md`:
ASCII, U+2500–259F, and `● ○ · — →`; no emoji, and Archie never appears — the
receipt is evidence and a mascot on it makes it look like marketing):

```
  archie  ○ this session is at rung 2 of 5 — artifact_changed
  ─────────────────────────────────────────────────────────────
  evidence   14 files written   0 test, build or lint commands
  missing    nothing ran that could have failed
  this repo  96 of 102 past sessions here reached rung 3+   (n=102)
  → run the gate, or say in one line why there isn't one
```

The last line is an ask, not an order. `convergence.md` settled that the
snippet must say "report", never "exit", and the same restraint applies here in
the other direction: the gate asks for evidence or for a sentence explaining
its absence, and either one ends it.

### The budget and the loop guard

Three re-entries, then the gate lets the stop happen and prints the receipt.
Sized from `convergence.md`'s finding that a gap ending in verified progress
runs p50 9 turns and p75 28: three nudges cost a small fraction of one such
gap, and the ceiling exists because the falsification in that spec says no
threshold can tell a converged session from a working one — so the gate must
run out of road on its own rather than trust its own judgement.

Both harnesses already carry a guard and the plugin honours it rather than
inventing a second one.

| harness | the guard, quoted | source |
| :--- | :--- | :--- |
| Claude Code | "The `stop_hook_active` field is `true` when Claude Code is already continuing as a result of a stop hook. Check this value or process the transcript to avoid blocking on a condition that will never resolve. Claude Code overrides the hook and ends the turn after 8 consecutive blocks." | `code.claude.com/docs/en/hooks.md` |
| dsh | none. `hooks-codex`'s own README lists "a stop loop-guard" as deferred, unbuilt work, and notes that "An unconditionally blocking hook therefore force-continues every step unless it self-limits." | `packages/hooks/hooks-codex/README.md:187, 175` |

So on Claude Code the plugin exits early when `stop_hook_active` is true and
never reaches 8. On dsh the plugin owns the whole guard, because nothing under
it has one. That asymmetry is the single most important line in this document
for whoever writes the dsh plugin.

### When AgentWorth cannot answer

The index is a scan of files on disk and it lags the live session by design.
Three cases, one rule: **say it once, in one line, and let the stop happen.**

| case | what the tool returns | what the plugin does |
| :--- | :--- | :--- |
| session not indexed yet | `indexed: false` | one line, allow the stop |
| index older than the transcript's mtime | `stale: true` | one line, allow the stop |
| no index at all, or the binary is missing | the call fails | one line, allow the stop |

A gate that blocks because it could not read something is a gate that gets
uninstalled on its first bad day. Never a crash, never a hang, never a silent
guess — the same rule `spacepilot-loop.md` applies to a missing SpacePilot.

## The three plugin bodies

One core, three thin adapters around it. The core is a function: take a session
identifier and a budget counter, call `session_gate`, return either "allow" or
"re-enter with this text". Everything harness-specific is the shell around it.

| | Claude Code | dsh | Codex |
| :--- | :--- | :--- | :--- |
| Ships as | a hook script + a settings block | `dsh-plugin-archie`, its own MIT repo | dsh's `@deepseek-ai/dsh-hooks-codex` reading a Codex `hooks.json` |
| Event | `Stop`, and `SubagentStop` | `agent/turn-stopping` | Codex `Stop`, mapped onto `agent/turn-stopping` |
| Re-enters by | `hookSpecificOutput.additionalContext`, or `decision: "block"` with `reason` | pushing into the `next-step` inbox during the awaited dispatch | the same, through the compatibility layer |
| Loop guard | `stop_hook_active`, plus an 8-block cap the harness enforces | the plugin's own, there is nothing under it | the plugin's own |
| Session id from | `transcript_path` on stdin | persistence `root` + `cwd`, `transcript_path` is null | as dsh |
| Needs a new repo | no | yes | no |

### Claude Code

`additionalContext` is the right field and the doc names this exact use case.
Quoting `code.claude.com/docs/en/hooks.md`:

> Use `additionalContext` when the hook is working as designed and giving
> Claude guidance, such as 'run the test suite before finishing'. It keeps the
> conversation going through the same loop protections as `decision: "block"`,
> namely the `stop_hook_active` input and the 8-consecutive-continuation cap,
> but the transcript labels it `Stop hook feedback` and no hook error
> notification is shown.

That is the gate, described by the harness's own documentation, one release
before it was proposed. So the plugin emits:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "Stop",
    "additionalContext": "<the six lines>"
  }
}
```

and not `decision: "block"`, which the docs reserve for a hook error and which
would render the receipt as a failure in the transcript. Exit 2 is documented
to work too — the doc's table gives `Stop` / can block: yes /
"Prevents Claude from stopping, continues the conversation" — but it routes the
text through stderr as an error, which is the wrong register for a receipt.

Configuration is a `hooks` block in a settings file; the doc's own table says
`~/.claude/settings.json` is all projects, `.claude/settings.json` is one
project, and a plugin's `hooks/hooks.json` applies when the plugin is enabled.
The documented default timeout for a `command` hook is 600 seconds, far more
than a `session_gate` call needs; the plugin sets its own low `timeout` so a
hung index can never hold a turn open.

### dsh

The re-entry semantics are the reason this plugin is P2 and not P1, and they
are now established rather than assumed. `agent/turn-stopping` is serial and
returns nothing the loop reads — `docs/architecture.md:95` says
"`agent/turn-stopping` is serial and has no `next()`", and the loop discards
the dispatch result:

```
304	        if (turnEnds && this.inbox.nextStep.length === 0) {
305	          await this.dispatch.serial('agent/turn-stopping', { turn, signal })
306	          signal.throwIfAborted()
307	        }
308	        if (turnEnds && this.inbox.nextStep.length === 0) break
309	        target = 'next-step'
```

(`packages/core/agent-loop/src/agent.ts`, fetched 2026-09-03.) The same
predicate is evaluated at 304 and again at 308, and the only thing that can
change in between is `this.inbox.nextStep.length`. **So a listener re-enters
the loop only by pushing a message into the `next-step` inbox while the loop is
awaiting it** — `agent.steer(msg)` is `send(msg, 'next-step', wakeup: true)`,
`agent.inject(msg)` is the same target with `wakeup: false`. Either lands in the
inbox; the wakeup flag is irrelevant here because the driver is already running
and re-reads the inbox directly at line 308.

dsh's own docs describe the mechanism the same way: `docs/subsystems/core.md`
says a listener that objects "steers (`agent.steer(...)`) and the machine
re-reads its inbox: fresh steering runs another step, none closes the turn.
Data decides, so listener order cannot change the outcome."

The plugin is therefore an observer with a side effect, and its whole
correctness rests on its own counter, since dsh ships no stop loop-guard.

Registering AgentWorth as an MCP server under dsh is one plugin instance per
server in a `cordis.yml`, not an `mcpServers` map
(`docs/config-catalog.md`, `@deepseek-ai/dsh-mcp-client`):

```yaml
- insert:
    - id: archie
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: archie
        transport: stdio
        command: archie
        args: [mcp]
        cwd: !!js process.cwd()
```

### Codex

**Codex-native hooks are unverified.** An official page exists at
`developers.openai.com/codex/hooks`, which redirects to
`learn.chatgpt.com/docs/hooks` and lists event names including `Stop`,
`PreToolUse`, `PostToolUse` and `SubagentStop`, with hooks "discovered in
`hooks.json` files or inline `[hooks]` tables within `config.toml`". The
per-event JSON schema, exit-code semantics and decision fields could not be
read from that source, so nothing in this spec depends on them.

The supported path is dsh's compatibility plugin, which is documented:
`@deepseek-ai/dsh-hooks-codex` "runs the hooks from your existing Codex config
— a `hooks.json` — during agent runs", mapping five Codex hook points, of which
`Stop` "is a serial listener whose blocking result forces another step through
`steer()` (`agent/turn-stopping`)"
(`packages/hooks/hooks-codex/README.md:83`). Note its limits: five of Codex's
ten events are unsupported and their config "is silently dropped during
parsing", `SubagentStop` among them.

## Sequencing

| P | What | Why here |
| :-- | :--- | :--- |
| P0 | The measurement above | Done. It moved the design twice: rung 2 not rung 1, and one missing condition not four |
| P1 | `session_gate` over MCP, plus the Claude Code `Stop` hook script | No new repo, no new harness, and the harness documents this exact use case |
| P2 | `dsh-plugin-archie`, its own MIT repo, topic `dsh-plugin` | A second harness proves the core is harness-shaped, not Claude-shaped |
| P3 | The dsh adapter — zstd frames, packed chunk rows, a real-session fixture | Until it lands the gate can grade a dsh session only through `cwd`, never through its own transcript |

**P1 before anything else, because it needs no new repository.** A hook script
plus one MCP tool is testable from a fixture index the way
`apps/cli/tests/doctor_self_test.rs` already tests the CLI, and the tool is
callable by anything. The MCP tool ships before any UI, for the same reason
`docs/specs/README.md` gives for every other tool on that list.

**P3 is a real adapter and should be estimated as one.** dsh's default
persistence is zstd frames, not plain JSONL: the artifact is "a standard
concatenation of independent Zstandard frames … one checksummed frame
containing only the header line, then one checksummed frame per durable append
batch", and with `packChunks` on, "an eligible run of >=3 consecutive
same-block `assistant/chunk` delta events becomes one packed row"
(`packages/session/session-persistence-jsonl/README.md`). So the adapter needs
a streaming zstd decoder and a packed-row expander before it emits its first
`NormalizedEvent`, and `AGENTS.md`'s fixture rule applies: a real dsh session,
redacted, checked in.

## What it deliberately does not do

- **It never calls a model.** `AGENTS.md`'s invariant, unchanged. The gate
  reads the index, formats what it found, and hands the text back. Nothing in
  it decides whether the work is good, only whether anything checked it.
- **It never edits a file, runs a command, or touches a repo.** It asks the
  agent to run the gate. It does not run one.
- **It never blocks past the budget.** Three re-entries, then it steps aside.
  Below the line and out of budget still ends with a stop and a receipt.
- **It never runs unless somebody installed it.** No auto-registration, no
  config that turns it on globally, no default hook written by an installer. A
  tool that quietly attaches itself to another product's stop event has earned
  every uninstall it gets.
- **It does not fire on every stop.** Above the line it is silent. `convergence.md`
  measured why a stop rule that speaks too often gets ignored.
- **No telemetry, no upload, no account.** The gate is a local read of a local
  index by a local plugin.

## Open questions

- **Should it fire on subagent stops?** 83% of below-the-line sessions are
  subagent runs, so firing only on the main stop misses most of them. Against
  that: a subagent is briefed by a parent that may not have asked it to run
  anything, and a gate that nags every child of a fan-out is a gate that gets
  turned off. Claude Code's `SubagentStop` carries `agent_type`, so the hook
  can match on it; dsh's Codex layer does not support `SubagentStop` at all.
  Not decided here.
- **Is three the right budget?** Picked from `convergence.md`'s gap
  distribution, not measured against what a person wants to be interrupted by.
  Claude Code's own cap is 8, which is a ceiling and not a recommendation.
- **`additionalContext` or `decision: "block"`?** The doc supports both. This
  spec picks `additionalContext` because the register is guidance and not
  error. Nobody has run both and compared what the agent actually does next.
- **Does the gate belong in the same place for a naked session?** Two thirds of
  sessions never claim done at all and never reach the gate. Whether a second,
  much quieter surface should exist for them, or whether
  `convergence.md`'s snippet is already the right answer, is open.
- **One counter or two?** The budget counter has to survive across hook
  invocations. Claude Code offers `stop_hook_active` as a boolean and not a
  count; dsh offers nothing. Whether the plugin keeps its own file, or
  `session_gate` counts its own calls per session, is an implementation choice
  with a privacy consequence either way.

## Appendix: the queries

Read-only throughout: `sqlite3 "file:$HOME/.agentworth/agentworth.db?mode=ro"`.
`NONSTUB` below is
`total_events > 1 AND total_tokens > 0 AND primary_outcome IS NOT NULL`.

Section 1, the firing rate and its adapter split:

```sql
SELECT adapter, COUNT(*) n,
  SUM(primary_outcome = 'done_claimed')    AS done_claimed,
  SUM(primary_outcome = 'artifact_changed') AS artifact_changed,
  SUM(primary_outcome IN ('done_claimed','artifact_changed')) AS below,
  ROUND(100.0 * SUM(primary_outcome IN ('done_claimed','artifact_changed'))
        / COUNT(*), 1) AS pct
FROM sessions
WHERE total_events > 1 AND total_tokens > 0 AND primary_outcome IS NOT NULL
GROUP BY 1 ORDER BY n DESC;
```

Add `AND started_at >= '2026-08-04'` for the 30-day rows.

Section 1 by repo, and section 5 by subagent, are the same query grouped
differently. Repo is not a stored column: fetch `source_path` and group in the
caller through `extract_repository_or_workspace`
(`crates/schema/src/provenance.rs`), the fetch-then-group shape
`stats_outcomes` and `stats_ladder` both use. The subagent split is
`source_path LIKE '%subagents/%' OR source_path LIKE '%/agent-%'`.

Section 2, the spend ceiling:

```sql
SELECT CASE WHEN primary_outcome IN ('done_claimed','artifact_changed')
            THEN 'below' ELSE 'at or above' END AS band,
  COUNT(*) n, SUM(total_tokens) tokens
FROM sessions
WHERE total_events > 1 AND total_tokens > 0 AND primary_outcome IS NOT NULL
GROUP BY 1;
```

Section 3 is not SQL. There is no events table, so it walks the raw
transcripts of the sessions the first query returns, matching Bash tool calls
against the ported predicates and pairing each `tool_use` with the
`tool_result` that answered it by `tool_use_id`, then deriving the exit code
through `exit_code_from_result(is_error, output_text)`. `convergence.md`'s
section on the same wall explains why this cannot be a query, and its three
ported predicates are the three ported here.

Section 4 is restated from `convergence.md`, not re-measured.
