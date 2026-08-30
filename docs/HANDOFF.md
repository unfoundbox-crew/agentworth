# HANDOFF — read this before touching anything

You're picking up **AgentWorth**. This is not a fresh idea — there's a real, shipped
V0 in this repo. Read `AGENTS.md` and `README.md` in full before this file; they're
the contract. This file is the part they don't say: *why it matters* and *what's next*.

## What's already real — don't rebuild this

V0 is shipped and on npm (`npx agentworth` / `cargo install agentworth`, site at
agentworth.dev). It scans the AI-agent history already sitting on the user's machine
(Claude Code, Cursor, Codex, Antigravity, Goose, Pi, Herdr, Hermes, OpenClaw, Grok,
OpenCode — 11 adapters), normalizes it into a canonical `AgentWorthTrace`, and scores
it with two things nobody else has:

- **Outcome Evidence Hierarchy** — doesn't trust "agent said done." Ladder is
  `DoneClaimed < ArtifactChanged < TestOrBuildPassed < CommitObserved < CiOrDeploymentVerified`.
- **TraceScore** — explainable 5-factor score: outcome quality, verifiability,
  complexity, recovery signal, provenance.

Pipeline: `source discovery → streaming adapters → NormalizedEvent[] → AgentWorthTrace →
outcomes + scoring → SQLite index → CLI / web UI / ATIF export`. Rust core throughout,
React/Vite/Tailwind only for the optional localhost UI, thin npm wrapper for distribution.
Design language is already set — monochrome "thermal receipt" aesthetic, don't reinvent it.

**Repo state right now:** on `fix/ci-cost-and-runners`, with uncommitted changes across
`apps/web/*` (index.html, App.tsx, Footer/Navbar/LandingPage, index.css, tailwind.config,
plus new AgentLogos.tsx / ThemeToggle.tsx / hooks/ / icons/). That's UI/theming work sitting
uncommitted on a CI-named branch — check `git status` and ask before assuming what it's for.
Other branches: `main`, `feat/v0-release`, `feat/gh-pages-v0`, `feat/schema-claude-scan`.

## Non-negotiables (the highlight reel — full list is in AGENTS.md)

- Rust core. Node/Python are never a runtime dependency, only distribution sugar.
- Local-first, zero telemetry, nothing uploads without explicit user action.
- Read-only on raw histories. Never duplicate full transcripts into SQLite — index
  metadata/scores, lazy-load raw content.
- Never infer success because an agent claims it. Evidence ladder or nothing.
- Adapters own all format-specific logic. Core code has zero Claude/Codex/Gemini
  branches in it.
- Streaming, bounded memory. Assume multi-GB files and tens of thousands of sessions.

## The two-phase product — why this is bigger than a nice dashboard

**Phase 1 (V0, shipped): receipts.** Retrospective. Answers "what did my agents
actually do, what did it cost, did it actually work." Read-only, forensic.

**Phase 2 (not started — the actual differentiator): a policy engine.** Turns the
verified-outcome data Phase 1 already collects into a *forward-looking* decision:
which model, which effort, single agent or fan-out, before you run the task — and a
handoff that survives a provider switch when quota runs out, instead of a blind
cold restart on Codex/Antigravity/OpenCode.

Already sketched end to end (see `agentworth-routing-problem-solution-diagram.html`,
attached in this handoff) — the shape is:

```
task intake  →  feature builder  →  priors + ledger  →  policy engine
  (aw run        (task class,          (public priors      (optimizes cost
  "investigate    novelty, risk,        + YOUR local         per completed
  X")              domain signals,      history: task →      task → picks
                   context/cache        model/effort →       route + effort
                   state)               verified outcome)     + shape + verifier)
                                                                    │
                                                                    ▼
                                              route → effort → shape → verifier plan
                                                                    │
                                                                    ▼
                                    execute (existing harnesses) → observe outcome
                                                                    │
                                                                    ▼
                                              feeds back into priors + ledger
```

`Priors + Ledger` *is* Phase 1's Outcome Evidence Hierarchy + TraceScore, just read
forward instead of backward. That's the whole point: don't build a second data model
for Phase 2. Phase 1's scoring engine is the training data. If Phase 1's adapters or
scoring drift from being trustworthy, Phase 2 has nothing to stand on — fix that
before adding routing logic.

### Phase 2 design principles

- **Durable object = task, not session/worktree/PR.** Those are disposable execution
  detail. The task identity is what survives a provider handoff or a retry.
- **Verifier failure ≠ model failure.** If there's no test/build/bench to check
  against, the fix is a verifier, not a bigger model. Don't let the policy engine
  paper over a missing verifier by escalating.
- **Stronger model vs higher effort are separate decisions.** Don't conflate them —
  "didn't know enough" escalates model, "didn't try hard enough" escalates effort.
- **Default parallelism is 1** unless the uncertainty is genuinely separable
  (breadth-first, independent subtasks). Fan-out on a sequential/dependent task is a
  net loss, not a hedge.
- Model/effort switches invalidate the prompt cache — the policy engine has to price
  that in, not just the sticker cost of the bigger model.

### Phase 2 CLI surface — staged, not all at once

```bash
agentworth route "task"       # recommend model/effort/harness/shape — no execution
agentworth explain <task-id>  # why that route was chosen, in terms of local history
agentworth run "task"         # only after route/explain are trusted — actually dispatches
```

`run` comes last on purpose. A recommendation you can inspect and disagree with is
worth more early than a dispatcher you have to trust blind. Example `route` output
to aim for:

```text
ROUTE
mode       research → experiment
harness    Claude Code
model      Fable 5
effort     high
agents     1

Why:
novelty                high
architecture impact    high
parallelizability      low
similar local tasks    14

Verifier:  benchmark required before implementation
Fallback:  Codex / xhigh
Confidence: 0.72
```

### Phase 2 cold start

Day one, there's no local history to route on. Cold-start policy = deterministic
features (task class, novelty, repo/domain signals, parallelizability) + whatever
local history already exists + optional public priors — not a paid/closed router
API. The whole point is this stays a local, explainable, no-dependency layer; it
gets *better* with local history, it doesn't *require* it to function.

### Phase 2 — first technical task, when someone picks this up

Not "design the router." First: **inspect what V0's schema already captures and
find what's missing** for routing to be possible at all. Specifically, does
`AgentWorthTrace` / `NormalizedEvent` currently carry:

- model switches mid-session
- effort-level changes mid-session
- cache/context cost (cache read vs cache write vs fresh input, per turn)
- quota/fallback events (hit a limit, switched provider)
- a stable task identity that survives a provider handoff
- verified completion (already exists via the outcome ladder — confirm it's queryable
  per-task, not just per-trace)

Whatever's missing gets added to the schema/adapters before any routing logic gets
written. A policy engine trained on incomplete signals is worse than no policy engine.

## The gap is real — checked against the market, not assumed

Three things exist separately and nobody's stitched them together:

- **Blind model routers** — Not Diamond (powers OpenRouter's "auto"), Martian,
  RouteLLM. Route per-prompt on a generic learned model. No git awareness, no idea
  what "done" means for your task, no memory of your repo.
- **Worktree/PR orchestrators** — a genuinely crowded field already: Fletch, Paseo,
  t3code, omg.dev, Garcon, Crewplane, NXTG-Forge, intentic, Alethe, Claudexor,
  Agent Teams. All wire Claude Code + Codex (+others) to worktrees and PR flow.
  Only 2 of ~13 claim any auto-routing, and it's quota-rotation (switch on rate
  limit), not difficulty-aware selection.
- **Coding benchmarks** — Aider's leaderboard scores models on public exercises.
  Real signal, but static and not tied to your repo or your tool fleet.

AgentWorth Phase 2 is the only thing proposed anywhere that routes off *your own*
verified outcomes, across *your own* tool fleet, computed locally, privately. That's
not a guess — it's what's missing after checking what's already shipped in this space.

## The metric that matters

**Cost per verified completed task** — not per-token price, not raw token count.
Phase 1 already computes half of this (TraceScore's outcome + verifiability factors).
The other half is token/cache cost math, which the adapters already extract. Nobody
has wired these into one number. That number is the product.

## Scope discipline — what NOT to build yet

- No marketplace, no bounty network. AGENTS.md puts "network/bounty experiments"
  dead last in the V0 priority list on purpose. Leave it there.
- No live policy engine until the adapter + scoring layer is solid across the full
  tool fleet (see V0 priority order in AGENTS.md — check where we actually are on
  that list before adding anything new). Prescriptive routing on shaky ground truth
  is worse than no routing.
- "The immediate product is not a marketplace" is a direct AGENTS.md quote. It still
  applies.

## Voice

No corporate process language. No "as a decision-maker you should consider."
Short, technical, imperative — the way AGENTS.md itself is written. If a sentence
would read fine in a vendor's product doc, cut it and say the actual thing.

## First move in this session

1. `git status` — there's uncommitted work on the current branch, read it before
   writing anything.
2. `cargo build` / `agentworth doctor` — confirm the toolchain is sane before
   assuming the last session left things working.
3. Ask what *this* session is actually for. Don't default to Phase 2 work just
   because this file describes it — Phase 1 likely still isn't done across the
   full adapter list.
