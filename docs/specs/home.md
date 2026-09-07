# home

Status: concept locked 2026-09-07, not built. Decision by Saurabh after nine
direction sketches on the design canvas; the chief-of-staff agent concurred.
Scaffold on branch `home-scaffold`, gateway on `home-gateway`. Revisit if the
first week of dogfooding shows the strip board still needs opening to act.

## The one-line version

A local window where a founder runs 5 to 100 coding agents by setting
directions and reading only what needs them.

## The problem, measured by others

| Pain | Source |
| :--- | :--- |
| Silent stalls: an agent blocked on a permission prompt for 86 minutes, nobody noticed | dotzlaw.com on herdr; georgediab.com |
| "I ended up doing the one thing the agents were supposed to save me from: watching" | georgediab.com |
| Self-reported sustainable parallelism is 2–3 agents, 5–10 with worktrees | Ask HN 47573483 |
| Follow-ups to a running agent: nobody knows if they interrupt or queue | openai/codex#37883 |
| Nobody trusts the summary; "read the diff" | ansezz.com, METR 2026-02-17 |
| Plan and background execution are separate objects at approval time | anthropics/claude-code#30510 |

Full reports with every source: the design session's `research/` directory,
copied here as `docs/specs/home-research/` when the first PR lands.

## The shape

See `apps/home/DESIGN.md`, "The shape". Primitive: a direction (rally point).
Surfaces: strip board, track, dock, ambient strip, standup. Default view:
exceptions only. Per decision: one strip, one evidence rung.

## What it stands on

| Need | Source | State |
| :--- | :--- | :--- |
| Presence per agent | herdr `events.subscribe`, protocol 20 | built, `home-gateway` |
| Evidence rungs, burn, halts | Archie ladder, `session burn`, governor | built |
| Speech and work per rider | harness transcripts Archie already tails | next Rust lane |
| Canvas, input, attach, voice | tldraw, assistant-ui or Vercel AI Elements | reuse, not build |

## Not decided

- Dark ground: the system's zinc black, or the owner's terminal slate as a
  home-only override. Both are on the canvas.
- Whether a direction maps 1:1 to a worktree. Fleet convention says yes.

## What the four-day ledger adds (read 2026-09-07, 637 prompts, Sept 4–7)

The ledger itself stays out of this public repo: it carries stream keys and an
auth token. It lives beside the checkout, not in it.

Things Saurabh asked for more than once, each a requirement here:

| Ask, in his words | What home must do |
| :--- | :--- |
| "who is waiting on what now? anyone waiting for me?" | the exceptions view answers this with zero clicks |
| "this agent died and will be reborn at so and so time" | a dead or quota-halted rider is shown as such, with the reset time, never as silence |
| "once they are done, they report back", "no endless chatter", "I don't like wait/listen/schedule" | riders report on settle; no polling loops in the UI, no chatter surfaced |
| "can I trust you all or am I needed in the building all the time" | the governor's halts and the evidence rungs are the answer; home shows them, it does not add a babysitting surface |
| "is this token machine a casino" (quota exhausted mid-task, 200 dollars overnight) | budget per direction, and per subscription window, visible before it is spent |
| "wake up zombie" (agents that lose context and lecture) | a rider that restarts gets `session_wake`; home shows a restart as a stop on the lane |
| "send my exact prompts, not your inference" | a steer is delivered verbatim; the chief of staff never paraphrases the human to a rider |
| "no characters or show references in the soul or the codebase" | roles only in code; themes are data |
| "reading HTML as images is super dumb" | home never asks a model to read a screenshot of text; text is text |
| "is that UI for humans or bots?" | home is the human side; the ACI (observe, plan, apply, receipt, interrupt) is the agent side; both read one state |
| "the Claude Code app is super customisable, complexity grows with your viewport and bandwidth" | elastic: rest state is one line; the human adds lenses (strips, track, map, faders, PTY) as needed; nothing is forced |
| "UI is cheap, the backend must be solid: latency, perf, quality" | the build order is substrate first; the canvas is where the human picks lenses, and can build their own |
| "shouldn't it be spaceship, not train?" | open. The shape holds under either name: a course and a heading instead of a track. Naming is a theme decision, not a code one |

Lineage: the Human Interface Tax was named on 2026-09-05 ("HIT is still there and
I'm paying it the most"); the ACI thesis and the Unix-shape primitive board
came the same day; the fleet ran on herdr with a chief of staff relaying, and
the recurring failure was the relay paraphrasing or forgetting. Home exists so
the human stops relaying through anyone.

## Testing is cheap, by rule (Saurabh, 2026-09-07)

Never probe, ping, or prompt a partner's pane to test the gateway. A frontier
session at hundreds of thousands of tokens of context is the most expensive
thing on the machine to poke, and a ping-pong through it costs cache and
attention that a test has no right to. Probes use a dedicated herdr workspace
labelled `probe` with a Haiku pane started for that purpose, and nothing else.
The first live probe on 2026-09-07 broke this rule once; this section is why it
does not happen again.

## Identity: three layers, one key (settled 2026-09-07 with the herdr adapter lane)

| layer | example | role |
| :--- | :--- | :--- |
| harness session id | `61eef7db…` | the only key. Present in herdr's `session.json` per pane, in `herdr agent list` next to the live pane, and in Archie's `sessions.session_id`. Verified equal for Claude Code, Codex and agy on 2026-09-07 |
| name | `partner-harvey`, a pane label, a session title | display, mutable. Recorded with the time seen, never overwritten, so a rename does not rewrite history |
| pane letter | `w9:pA` | live-only address that herdr allocates at runtime. Not on disk anywhere. The gateway adds it for the deck; nothing stores it as identity |

Grok's adapter names sessions after files, so Grok does not join yet. That is an
adapter fix, not a gateway one.

## Nothing opens on an end state (Saurabh, 2026-09-07, after the first hands-on)

"Why should there be a page that says nothing needs you? A terminal, a chat,
everything opens blank, an invitation to create something. I'm a new user, I
ran some npm commands, I bought tokens. Now what do I do?"

Cruise is where you return once directions exist. It is never the first
screen. The first screen is one blank line with a cursor: what are we
building? Typing it is the whole setup: the text becomes the first direction,
the area is the repo in cwd, and if no rider exists the deck offers the
harnesses found on the machine and starts one in a pane. One command opens it,
`archie home`, serving the built deck from the binary. No second server, no
seeding script, no herdr required to reach the first line.

What a new user must never need: a vite dev server, an environment variable,
a socket script, or a partner's pane.

## Release, branches, standby (from Donna's session, 2026-09-07)

- **Release policy (Saurabh, 11:01 UTC):** everything ships together in v0.1.23,
  not before. Work in progress lives on a dev branch and is tested locally.
  On 2026-09-07 the director merged the train to main instead of a dev branch
  without asking; main is unreleased, so nothing shipped, but the question
  should have been asked. Recorded so the next merge asks first.
- **Standby seats (Saurabh, 12:52):** "I don't want to kill them but keep them on
  standby so my mental load is reduced." A seat can be put on standby: alive,
  out of the seats column and off the course, one count in the status bar,
  brought back with one key. Not mute, which hides speech; standby hides the
  seat. Feature for the deck, not built.
- **Latency (Saurabh, 15:37, unanswered at the time):** "what is the perf and
  latency tax of web versus ghostty, don't tell from vibes." Measured, with the
  method stated, before anyone claims the deck is as fast as a terminal. See
  `docs/specs/home-latency.md` when it exists.
- **A new user's path (Saurabh, 15:34):** "say I get a new laptop tomorrow and
  don't know any of these, what is my step by step." Written as a doc once
  `archie home` exists, and tested on a machine that has none of it.
