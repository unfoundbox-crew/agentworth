# Governor

Status: proposed, 2026-09-06. Nothing built. Answers the three-part
operational thesis (in-flight circuit breakers, asymmetric division of
labour, cache-prefix preservation) with what the harnesses actually expose
and what the index already holds. Sources are dated 2026-09-06.

## The one-line version

Archie stops being the claims adjuster and becomes the governor: it meters
every live session from its own transcript, tells the model what it is doing
wrong with the receipt attached, and, when the person has said so, stops the
next model call before it is paid for.

## What is true today

| Fact | Source |
| :--- | :--- |
| A Claude Code `PreToolUse` hook can deny one tool call (`hookSpecificOutput.permissionDecision: "deny"` plus `permissionDecisionReason`, or exit 2). `PostToolBatch` with `continue: false` "stops the agentic loop before the next model call". `ConfigChange` and `PreModelSwitch` can block. `UserPromptSubmit`, `SessionStart`, `PostToolBatch`, `Stop` accept `additionalContext` the model sees. Sync hooks have a timeout; `async: true` hooks cannot block | code.claude.com/docs/en/hooks |
| Hook payloads carry no token usage. Usage is in the transcript: every assistant record has `usage.{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens}` | the Claude adapter, `crates/adapters/src/claude.rs` |
| Codex CLI has hooks (`hooks.json`, `PreToolUse` blocks with `{"decision":"block","reason"}`, on by default since mid-2026) according to third-party guides. **Not confirmed against OpenAI's own docs.** No `PostToolBatch` equivalent was described | knightli.com, agenticcontrolplane.com, hookstack.app |
| Archie already has: the Loop Sentinel (`crates/outcomes/src/loops.rs`: three identical tool calls, four revisions of one file, self-corrected vs human-rescued), `archie session watch` polling it, a pricing table with cache rates and `estimate_model_tokens_cost_usd`, the cache-economics report (#42) and cache doctor, `window show` pacing, and since v0.1.21 the loop: hook events over a socket, per-session state, `tool_intents`, verification-shaped commands classified by result | the checkout at v0.1.21 |
| The tweet that started this (a $200/month Codex weekly limit gone in 14 hours) was not fetched; the search found Codex's 5-hour window and weekly cap, not the post | WebSearch |

## The thesis, torn apart

**1. "Circuit breakers."** Three of the four levers exist, and the one people
reach for first is the weakest.

| Lever | What it does | Trap |
| :--- | :--- | :--- |
| `PreToolUse` deny | blocks one call; the model sees the reason | a loop given a denial has a new error to loop on. Denial is not a pause |
| `PostToolBatch` `continue: false` | halts before the next model call; that is the only lever that stops the next spend | the person sees the turn end. It must say why, in the reason and in a row |
| `additionalContext` | the model reads a sentence of evidence before it acts again | the strongest lever by far, and the cheapest. A model told "you have edited `lib.rs` four times, the last test run failed with this line, no run has passed since" stops thrashing more often than one that is denied |
| a real pause | none. No harness exposes "suspend the session" | say so; `continue: false` per batch is the closest thing |

The burn-rate cap needs a meter, and hooks carry none. The meter is the
transcript, which Archie already tails (`live_tail.rs`): each new assistant
record gives tokens per turn, the cache read share, and the price at the
table rate. Two traps here. The provider's remaining quota is not in the
transcript, and the provider's accounting is not the token count: Archie
must never say "you have 30% left"; it reports burn against a budget the
person set. And every meter and every gate fails open: if `archie serve` is
down, the sync hook exits 0 inside its budget, or every agent on the
machine is bricked by its own governor.

**2. "Asymmetric division of labour."** Archie must not route. It never
sends a prompt to a model on its own (AGENTS.md), and a governor that
silently swaps a Fable turn for a Flash turn is a different product with a
different trust boundary. What Archie can do deterministically is measure
the split and say it: per session, tokens by model cost class by tool class
(read-only calls, edits, verification), which yields one number, the rake
share: what a frontier model spent on `Read`, `Grep`, `Glob`, `ls`. It can
put that number in front of the model at `UserPromptSubmit` ("the last five
sessions in this repo spent 62% on reads; the Agent tool takes a cheaper
model") and, if the person turns it on, deny the fortieth consecutive
read-only call by a frontier model with that reason. The threshold is high
because deep reading is often the job; the default is advisory. Choosing
the cheap tier is a policy line the person writes, naming a model, and the
fleet already has one (agy, Flash). Archie tells the agent; it does not
call the tier.

**3. "Cache-prefix preservation."** Archie cannot pin a system prompt, order
files, or make history append-only; the harness builds the prompt. What it
can do is name every break and block the two causes it can see. A break is
visible in the transcript as `cache_read_input_tokens` collapsing between
consecutive turns while input stays large, and its cause is usually a hook
event in the same window: `ConfigChange`, `InstructionsLoaded`,
`PostModelSwitch`, `PreCompact`/`PostCompact`, or a `deferred_tools_delta`
attachment (an MCP tool list changed). Archie attributes the break, prices
it from the table (the cache read rate is a pricing-table fact, not a
number this spec asserts), and writes the row. `ConfigChange` and
`PreModelSwitch` can be blocked mid-session by policy; compaction can be
blocked too, and must not be by default, because the alternative is a full
context. The discount figure people quote is the provider's; the pricing
table carries whatever it is.

**4. The trap under all three:** a governor that only lives in Claude Code
answers Rasmus with a working solution for the harness he is not using.
Codex's hooks, per third-party guides, offer `PreToolUse` deny and nothing
like `PostToolBatch`. For Codex the breaker is deny plus reason, weaker, and
that is unconfirmed until OpenAI's docs are read. Cursor and Gemini CLI: not
researched here.

## The shape

```
 transcript (usage per turn) ─▶ meter ─┐
 hook events (v0.1.21 loop) ─────────▶ governor: policies ─▶ note | deny | halt ─▶ governor_events
                                       ▲ sync gate: archie hook --gate (≤ 50 ms, fails open)
```

| Part | What |
| :--- | :--- |
| meter, in `crates/loop` | per live session: tokens per turn, tokens per minute, price at the table, cache read share, cache breaks with attributed cause, rake share by model cost class, verification runs and their results. Fed by the transcript tail the server already has and by the hook events already flowing |
| policies, `~/.agentworth/policy.toml`, repo override `.agentworth/policy.toml` | budgets (tokens or dollars per session per window the person names), thrash (N edits of one file with no passing verification between; the Loop Sentinel's rules), rake (M consecutive read-only calls by a cost class above X), cache (block `ConfigChange`, block `PreModelSwitch`, warn on compaction), and per rule an action: `note`, `deny`, `halt`, plus `fail: open|closed` |
| gate, `archie hook --gate` | the same binary in synchronous mode for `PreToolUse` (matcher `Bash|Edit|Write|MultiEdit|NotebookEdit|Read|Grep|Glob`), `PostToolBatch`, `UserPromptSubmit`, `ConfigChange`, `PreModelSwitch`. One socket round trip against in-memory state, 50 ms budget, exit 0 on any failure unless the rule says closed. Emits the exact JSON the harness documents |
| receipts | `governor_events(session_id, at, seq, rule, action, reason, evidence JSON)`; `archie session burn [id]` (live: rate, price, cache share, breaks, rake share); `archie policy show|check|replay`; `session_wake` gains a "Burn" line; MCP `session_burn` |
| replay | `archie policy replay --since 30d` runs every rule over the index without touching anything and prints what would have tripped, per rule, with tokens after the trip. Deterministic, so the launch post has a number instead of a claim |

`archie hook print claude --govern` prints both the async recorders that
exist today and the sync gates, so a person who wants only the meter keeps
what they have.

## What each rule needs, and where it comes from

| Rule | Evidence | Already in Archie |
| :--- | :--- | :--- |
| thrash: file edited N times, no passing verification between | `tool_intents` for edits; verification commands and their results from `PostToolUse` | yes: `is_verification_command`, result classification, the Loop Sentinel |
| blind loop: identical tool call three times | the Loop Sentinel | yes |
| burn: tokens per window over budget | the transcript tail | tail yes, per-turn usage in the runtime no |
| rake: M read-only calls by a frontier model | hook events with `tool_name`, model from the transcript's assistant records | partly: model cost class comes from the pricing table |
| cache break | consecutive `usage` records; hook events in the window | cache doctor does it after the fact; in-flight no |

## Sequencing

| Release | What | Gate |
| :--- | :--- | :--- |
| v0.1.22, the meter | per-turn usage in the loop runtime from the transcript tail; `session burn`; `governor_events` in shadow mode (every rule evaluated, every action logged as `would_have`); `policy replay` over the index | the replay prints, for this machine's 5,256 sessions, how many would have tripped each rule and the tokens spent after the trip |
| v0.1.23, the gates | `hook --gate`, `note` by default, `deny` and `halt` opt-in, `ConfigChange`/`PreModelSwitch` blocks | a thrashing fixture session is halted before its next model call, with the reason visible and the row written; the gate's p95 under 20 ms |
| v0.1.24, the other harnesses | Codex `hooks.json` adapter once OpenAI's docs are read; Cursor and Gemini CLI if they expose a gate | one turn governed end to end on Codex |

## What stays true

No model is called by Archie; the governor speaks to the model through the
harness's own channel. No routing. No upload. Every action is a row with
its evidence, and shadow mode runs before any rule can block. The gate fails
open by default, and the person, not the tool, turns `deny` and `halt` on.

## Open questions

- The provider's accounting: a budget in tokens at the table price is the
  honest unit. Should the policy also accept the harness's own cost line when
  it prints one, so the two can be compared?
- `halt` ends the batch; nothing resumes it but the person. Is a
  `resume-with-note` (the next prompt gets the governor's evidence as
  context) worth building, or is the halt reason enough?
- Rake thresholds are guesses until replay reports the distribution of
  consecutive read-only runs on real sessions.
