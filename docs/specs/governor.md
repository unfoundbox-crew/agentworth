# Governor

Status: built, PR #TBD (2026-09-06). Answers the three-part
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
| Codex CLI hooks, from OpenAI's own docs and source: on by default (`hooks.json` or `[hooks]` in `config.toml`); `PreToolUse` denies with the same `hookSpecificOutput.permissionDecision` shape Claude Code uses (the older `{"decision":"block"}` still accepted); `UserPromptSubmit` blocks a prompt with `{"decision":"block","reason"}` or exit 2; `PostToolUse` `decision: block` replaces the tool result the model sees and takes `additionalContext`; no batch-level halt exists. The rollout JSONL carries `event_msg`/`token_count` records with `last_token_usage` and a `rate_limits` snapshot: `used_percent`, `window_minutes`, `resets_at`, `plan_type`, `spend_control_reached` | learn.chatgpt.com/docs/hooks; codex-rs/protocol/src/protocol.rs; both fetched 2026-09-06 |
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
 hook events (v0.1.21 loop) ─────────▶ governor: policies ─▶ note | deny | halt | suspend ─▶ governor_events
                                       ▲ sync gate: archie hook --gate (≤ 50 ms, fails open)
```

**Shipped, and where the build differs from the design above:** batch
edits are deduplicated by `tool_use_id`, so an edit reported at `PreToolUse`
and again in the `PostToolBatch` payload counts once. When a spend halt and
a thrash halt trip on the same batch, only the spend halt suspends. The
gate's round-trip budget is 50 ms by default and `ARCHIE_GATE_BUDGET_MS`
overrides it. `policy replay` replays the current policy over indexed
intents, not over recorded decisions, because a tuning tool must answer
"what would this number have done". The `[cache]` rules parse but do not
act until v0.1.23. The loop rule counts consecutive identical calls in the
runtime; it does not yet read the Loop Sentinel. Codex's
`cache_write_input_tokens` lands in `cache_creation`.

## The brake, v0.1.22

Two rules, both active, both blocking, both shipped in the first release.
A meter that only reports is a post-mortem with a shorter delay; the point
of the release is that a session running tonight cannot burn to zero.

**Thrash halt.** Evidence: the same file edited `N` times (default 3) with
no verification command passing between edits. The edits come from the
`PostToolBatch` payload's own `tools` array and from `tool_intents`; the
verification results come from the transcript, which is written before the
batch hook fires. Action at `PostToolBatch`: `{"continue": false,
"additionalContext": "<the ground truth>"}` where the ground truth is the
file, the edit count, the last failing command and its last output line,
and the sequence numbers. The next model call does not happen. When the
person prompts again, the same text arrives as `additionalContext` on
`UserPromptSubmit`, so the model resumes knowing why it was stopped.

**Session spend cap.** Evidence: the session's tokens or dollars at the
table price since it started, from the transcript tail, against `X` tokens
or `$Y` in `policy.toml` (per session; a per-window cap across sessions is
the same check over `sessions` plus the live tail). Action at
`PostToolBatch`: `continue: false` with the reason. Then, until lifted,
every `UserPromptSubmit` for that session exits 2 with the same reason,
which blocks the prompt before any model call. That is a suspension the
harness can actually enforce. `archie policy lift <session>` clears it;
raising the cap in `policy.toml` clears it for everyone.

**Fail open, and only open.** The gate is one socket round trip with a 50 ms
budget. If `archie serve` is not there, the hook exits 0 and writes one
line to the spool so the miss is on record. When it is there, it stops the
spend. There is no fail-closed mode in v0.1.22: a governor that can brick
every agent on the machine when its own server dies is a worse failure than
one missed halt.

**The meter is inside v0.1.22, not before it.** Per-turn usage from the
transcript tail is what the cap reads, so it ships in the same release,
with `archie session burn [id]` as its face. Shadow mode exists as a
switch (`action = "note"`), not as a phase: a person who wants to watch
before they block sets it. `archie policy replay` stays as a threshold
tuning tool over the index and is not the product.

| Rule | Gate event | Action | Cleared by |
| :--- | :--- | :--- | :--- |
| thrash | `PostToolBatch` | `continue: false` + ground truth; the truth repeats on the next prompt | a passing verification of that file, or the next edit of a different file |
| spend cap | `PostToolBatch`, then `UserPromptSubmit` | halt, then block every prompt with the reason | `archie policy lift`, or a higher cap |
| identical call ×3 | `PostToolBatch` | `note` by default, `halt` if set | the next different call |
| cache: config change or model switch mid-session | `ConfigChange`, `PreModelSwitch` | block with the priced reason, if set | the person |

**On Codex.** OpenAI's docs confirm the levers, so the Codex brake ships in
v0.1.22, not later. Thrash: `PostToolUse` with `decision: block` replaces
what the model sees with the ground truth. Spend cap: once tripped, every
`PreToolUse` is denied with the reason and every `UserPromptSubmit` is
blocked until lifted. What Codex lacks is a batch-level halt, so the model
call that follows a denied tool still happens; the spec says so. What Codex
has that Claude Code does not is the provider's own quota in the transcript:
`session burn` reads the `rate_limits` snapshot and speaks it as "provider
says", the one place a remaining-percentage may be said aloud, because it is
the provider's number and not ours.

## What each rule needs, and where it comes from

| Rule | Evidence | Already in Archie |
| :--- | :--- | :--- |
| thrash: file edited N times, no passing verification between | `PostToolBatch.tools`, `tool_intents` for edits; verification commands and results from the transcript | yes: `is_verification_command`, result classification, the Loop Sentinel |
| blind loop: identical tool call three times | the Loop Sentinel | yes |
| spend cap | the transcript tail, per turn | tail yes; per-turn usage in the runtime is new |
| rake: M read-only calls by a frontier model | hook events with `tool_name`, model from the assistant records | partly: cost class from the pricing table |
| cache break | consecutive `usage` records; hook events in the window | cache doctor does it after the fact; in-flight is new |

## Sequencing

| Release | What | Gate |
| :--- | :--- | :--- |
| v0.1.22, the brake | the meter, `session burn`, `hook --gate` on `PostToolBatch` and `UserPromptSubmit` (and `PreToolUse`/`PostToolUse` on Codex), the thrash halt and the spend cap active on both harnesses, `governor_events`, `policy lift`, `policy show|check|replay`, `hook print codex` | a thrashing fixture session is halted before its next model call with the ground truth in the next prompt; a session over cap cannot submit a prompt until lifted; the gate's p95 under 20 ms; serve killed mid-session, the agent keeps working and the spool has the miss |
| v0.1.23 | rake share and its advisory, cache-break attribution in-flight, `ConfigChange`/`PreModelSwitch` blocks, `PreToolUse` gate for per-call denies | one priced cache break named with its cause while the session runs |
| v0.1.24 | Cursor and Gemini CLI, if they expose a gate | one governed turn on each |

## What stays true

No model is called by Archie; the governor speaks to the model through the
harness's own channel. No routing. No upload. Every action is a row with
its evidence. The gate fails open. The two rules that ship first block by
default once a cap or a threshold is written in `policy.toml`; writing it is
the person's decision, and a missing file means nothing is governed.

## Open questions

- The provider's accounting: a budget in tokens at the table price is the
  honest unit. Should the policy also accept the harness's own cost line when
  it prints one, so the two can be compared?
- `halt` ends the batch; nothing resumes it but the person. Is a
  `resume-with-note` (the next prompt gets the governor's evidence as
  context) worth building, or is the halt reason enough?
- Rake thresholds are guesses until replay reports the distribution of
  consecutive read-only runs on real sessions.
