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
