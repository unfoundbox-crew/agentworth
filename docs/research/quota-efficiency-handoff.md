Source: external research handoff via Saurabh, 2026-09-02.
Context-only. Not a decision record; the spec it fed is docs/specs/efficiency-receipts.md.

# AgentWorth — Quota Efficiency / Waste Receipts

## Goal

Build **local, receipt-backed analytics** answering:

> Why did my Claude/Codex quota disappear so quickly, and which repeated work could have been avoided?

AgentWorth is local-only Rust + SQLite. No telemetry, accounts, cloud, or per-turn hooks.

Existing:
- transcript adapters for ~20 coding agents
- evidence ladder: claimed → files → tests → commit → CI
- 5-hour pacing/burn alarm
- loop sentinel
- compaction tracking
- recovery detection
- loose ends
- MCP:
  - `session_handoff`
  - `carry_forward`
  - `forgotten_context`

## Key research conclusion

Do **not** model this as only:

```text
tier → bucket size → wasted tokens
```

Use:

```text
                   PROVIDER LIMIT
                  partly unknowable
                         │
          ┌──────────────┴──────────────┐
          │                             │
   DIRECT INEFFICIENCY          CONTEXT AMPLIFICATION
   provable locally             locally measurable proxy
          │                             │
   rereads / retries             unnecessary context
   churn / recovery              survives many turns
          │                             │
          └──────────────┬──────────────┘
                         ▼
                    TASK VALUE
             test → commit → CI
```

A useless 15k-token read followed by 20 turns is more interesting than merely recording “15k reread”.

Measure:

```text
context_exposure =
    unnecessary_payload_tokens
    × subsequent_turns_until_compaction/clear/end
```

Call this **token-turn exposure**, never provider quota consumption.

---

# 1. Deterministic Waste Signatures

Start strict. No LLM classification.

| Signature | Detection | Main false positive |
|---|---|---|
| Exact reread | same canonical path + range/content hash; no intervening write | verification |
| Duplicate inspection | same normalized grep/find/diff + cwd + result hash | intentional recheck |
| No-state retry | same failed command + same error; no state mutation between | flaky test/network |
| Invocation mistake | bad path/flag/executable/syntax before useful state change | environment discovery |
| Edit oscillation | `A→B→A`, inverse patches, same hunk repeatedly rewritten | exploration |
| Post-compaction rehydration | unchanged information reacquired after compaction | necessary recovery |
| Duplicate subagent work | sibling agents acquire same file/hash or command/result | independent verification |
| Cold-start rebootstrap | same repo repeatedly reacquires same initial context | intentional fresh session |
| Exact abandoned work | edits exactly reverted without advancing evidence ladder | exploration |

Label evidence conservatively:

```text
PROVEN_DUPLICATE
PROBABLE_WASTE
AMPLIFICATION_RISK
UNKNOWN
```

Do **not** claim semantic “waste” where intent is unknowable.

---

# 2. Important Provider Boundary

Locally observable:

```text
input_tokens
output_tokens
cache_creation_input_tokens
cache_read_input_tokens

tool calls/results
reads
patches
commands/errors
subagents

compaction:
  preTokens
  postTokens
  cumulativeDroppedTokens

observed rate-limit messages
tier metadata
```

Provider-only / currently unknowable exactly:

```text
absolute Max 5x/20x quota units
exact five-hour accounting formula
exact weekly accounting formula
weighting of cache reads vs writes vs output
server-side cache eviction reason
other-device / claude.ai consumption
internal model/effort accounting
remaining authoritative allowance
```

Important research finding:

```text
NO VERIFIED ANTHROPIC PRIMARY SOURCE
publishes an absolute token budget for
Max 5x / Max 20x five-hour or weekly limits.
```

Anthropic documents:
- Max 5x / Max 20x relative usage multipliers
- session limit resets every five hours
- weekly limits also exist
- usage depends on model, context, effort/features
- Claude web/Desktop/Code can share allowance

Do **not** assume the subscription limit uses the API's documented continuously replenishing token-bucket algorithm.

Do **not** translate API cache pricing into Max subscription quota accounting.

Tier should therefore be:

```text
tier = partition/prior
NOT
tier = known token bucket
```

Learn an **empirical depletion frontier**, not “Max20 contains X tokens”.

Example:

```text
Observed:
4 local exhaustion events occurred around
this combination of usage/cache/model/effort.

Caveat:
other Claude surfaces may have consumed allowance.
```

---

# 3. Feature Output

Call it **Efficiency Receipts**.

```text
┌─────────────────────────────────────┐
│ DIRECT INEFFICIENCY                 │
│ 18.4k attributable payload tokens   │
│                                     │
│ 9.7k exact rereads                  │
│ 5.1k no-state retries               │
│ 3.6k compaction rehydration         │
│                                     │
│ receipts: session/event IDs         │
├─────────────────────────────────────┤
│ CONTEXT EXPOSURE                    │
│ 312k token-turns                    │
│ derived metric — NOT provider quota │
├─────────────────────────────────────┤
│ DEPLETION FRONTIER                  │
│ earlier than 3/4 observed hits      │
│ Max20 · Aug/Sep-2026 regime         │
└─────────────────────────────────────┘
```

Every number must resolve to:

```text
session_id
event_seq(s)
detector
source payload/hash
```

---

# 4. Pull-Only Prevention

Add one MCP primitive:

```text
repeat_check
```

Agent calls it only when useful.

```text
task starts
    ↓
carry_forward(repo)

after compaction
    ↓
forgotten_context(...)

about to reread/rerun/reinvestigate
    ↓
repeat_check(...)
```

Examples:

```text
repeat_check(
  kind="read",
  path="src/auth.rs",
  range="120:220"
)
```

Return one line:

```text
UNCHANGED — src/auth.rs:120-220 read at
8f2:e143; no write since → skip reread
[8f2:e143→e211]
```

Failed command:

```text
NO_STATE_CHANGE — cargo test auth failed
identically at e522/e541; no mutation between
→ change state before retry [8f2:e522,e541]
```

Post-compaction:

```text
DROPPED — information existed before compact
e388 and source is unchanged → use
forgotten_context [8f2:e143,e388]
```

Keep MCP answers tiny:

```text
verdict
+ one useful fact
+ suggested action
+ receipt
```

AgentWorth itself must not become another context-expansion mechanism.

---

# 5. Experiment Before Building UI

Test:

> **H1: exact rereading is the largest deterministic direct inefficiency associated with early quota depletion.**

Use the user's last two weeks.

Create normalized events:

```text
session_id
event_seq
timestamp
repo
kind

canonical_path
range
content_hash

normalized_command
cwd
result_hash
is_error

payload_tokens_estimate

compaction_epoch
model
effort
```

Keep provider usage fields separate.

### Query A — rereads

For each read:

```text
find previous:
same canonical path
same range/content hash

AND NOT EXISTS:
intervening write touching file
```

Split:

```text
same compaction epoch
    → reread

cross-compaction
    → rehydration
```

### Query B — retries

Self-join failed commands on:

```text
normalized_command
cwd
error/result hash
```

Require no intervening provable state mutation.

### Query C — edit churn

Reconstruct exact cases:

```text
A → B → A

or

patch P → inverse(P)
```

### Query D — duplicate subagents

Compare sibling agents on:

```text
(path, content_hash)

and

(normalized_command, result_hash)
```

### Calculate two separate metrics

```text
DIRECT INEFFICIENCY
= attributable duplicate/retry/churn payload
```

and

```text
CONTEXT EXPOSURE
= waste_payload
  × later model turns before
    compact / clear / session end
```

Never add token fields together and call the result “quota consumed”.

---

# 6. Falsification

Compare observed limit-hit episodes.

Control/stratify where possible for:

```text
model
effort
turn count
compactions
subagents
elapsed time
```

Possible outcomes:

```text
A.
reread dominates direct waste
+ predicts early exhaustion
→ build reread prevention first

B.
reread itself modest
but token-turn exposure huge
→ build context hygiene first

C.
reread small
and unrelated to early exhaustion
→ hypothesis falsified
```

Do not bake “rereading is the biggest problem” into product language before this experiment.

---

# 7. Competitive Position

Usage/cost tooling already exists.

Examples to inspect:
- Anthropic Claude usage/status tooling
- ccusage
- Claude Code usage monitors/dashboards
- Cursor usage/spending dashboard
- OpenAI Codex usage/status

They already cover much of:

```text
tokens
cost
cache
sessions
burn rate
5-hour blocks
remaining allowance
```

Do **not** position AgentWorth as another token dashboard.

Potential differentiated layer:

```text
transcript
   ↓
deterministic inefficiency evidence
   ↓
context amplification
   ↓
task outcome evidence
   ↓
preventive MCP pull
```

I have not verified another current tool combining all four.

---

# Implementation Priority

```text
P0  two-week offline experiment
 │
 ├─ exact reread detector
 ├─ no-state retry detector
 ├─ exact edit-revert detector
 ├─ duplicate subagent detector
 └─ context-exposure metric

P1  receipts + CLI report

P2  repeat_check MCP

P3  local dashboard

P4  empirical depletion frontier
    once enough observed limit hits exist
```

Do **P0 before productizing the thesis**.

## Non-negotiables

- Rust/local SQLite
- no telemetry/cloud/account
- no push-per-turn instrumentation
- no LLM summaries/classification
- deterministic detectors
- every claim has receipts
- distinguish observation / derivation / inference
- single-user data is personal analytics, never benchmark data
- provider quota mechanics remain explicitly unknown where undocumented

## Primary research to preserve/check

Use current primary documentation from:

- Anthropic: Max plan usage limits
- Anthropic: Claude usage/length limits
- Anthropic: Claude Code models/usage/limits
- Anthropic API: rate limits
- Anthropic API: prompt caching
- Anthropic: current model/effort documentation
- OpenAI: Codex usage/limits documentation
- Cursor: official usage-limit documentation

Critical distinction:

```text
API accounting documentation
        ≠
subscription quota accounting
```

Do not infer one from the other without primary-source evidence.