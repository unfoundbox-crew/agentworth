# The AgentWorth and SpacePilot loop

Status: direction, decided 2026-09-02. Nothing built. This is a contract
between two repos, not a spec for either one's own build.

## The one-line version

AgentWorth feeds SpacePilot's registry dream, and SpacePilot feeds
AgentWorth's missing intelligence layer.

## Where each product stops

One platform, two products. Each runs alone. Together, they're the point.

| AgentWorth | SpacePilot |
| :--- | :--- |
| Grades trajectories — any agent, any harness, any machine. | Owns the fleet: nodes, open-weight models (Kimi, GLM, DeepSeek, and others), placement, endpoints, the registry. |
| Never grows a fleet mode. On a SpacePilot cluster its scanner just reads wherever the fleet's agents write transcripts — OpenCode or SpacePilot's own harness — the same way it reads any other adapter today. | Runs the agents that produce those transcripts, and reads AgentWorth's grades back into the registry. |
| Feeds `outcome_rate` and cost-per-verified-outcome (`verified-outcome-rate.md`) into the registry. | At fleet scale, a group's `n` stops being something the `min_n` floor suppresses and becomes the registry's confidence level instead — more sessions, more trust in the number, never fewer numbers shown. |

Mutual dependency is expected here. The platform is the contract between the
two products, not a merge into one codebase.

## What flows from AgentWorth to SpacePilot

| What | Source |
| :--- | :--- |
| Graded trajectories | `crates/scoring` — the five-component `TraceScore` |
| Handoffs | `handoff.md` — `session_handoff`, `carry_forward` |
| Receipts | every outcome and recovery signal, each pointing at the transcript event it came from |
| ATIF export | `agentworth export --format atif` (`crates/export-atif`) |
| Outcome rates | `verified-outcome-rate.md`'s aggregate query |

All of it goes through the same pipeline AGENTS.md already requires for any
export: `select → redact → preview → explicit approval → export`. Redacted
is the default; nothing crosses the process boundary un-redacted unless a
human approved that specific export. AgentWorth does not run a background
sync — every row SpacePilot ever sees came from a command a person ran.

## What flows from SpacePilot to AgentWorth

Local inference, for exactly three named uses, each behind its own flag:

| Use | Where it's needed | Flag |
| :--- | :--- | :--- |
| Archie's question router | `archie.md` — routing a free-text question to one of the four MCP tools | `--route` |
| Entity resolution for beliefs | `beliefs.md` — telling a bare-noun anchor apart from a path or issue number across sessions | `--resolve-entities` |
| `asks --summarize` | `asks.md` tier 2 — a one-line summary per question/answer pair | `--summarize` |

Nothing else calls out to SpacePilot. If a fourth use shows up later, it
gets named here first.

The real surface on the other end is `spacepilot daemon`'s local FastAPI
app (`spacepilot/daemon/api.py`) — each flagged call is an HTTP request to
that daemon's `/v1/run`, the same endpoint `spacepilot run` itself uses.
"Is SpacePilot here" is a reachability check against the daemon's
`/healthz`, falling back to `spacepilot doctor` when no daemon is running —
never a guess based on whether the binary is on `PATH`.

## The invariant

AgentWorth never sends a prompt to a model on its own (AGENTS.md's core
invariants). Every row in the table above obeys it the same way:

- The call only happens behind the flag that names it.
- The estimated cost prints before the call runs.
- Nobody can trigger it by default, and no config setting turns it on
  globally.

This is not a SpacePilot-specific exception — it's the same rule AgentWorth
already applies to any model call, extended to a second binary.

## What this deliberately does not do

- No shared database. Each product keeps its own storage.
- No sync. Every exchange is one command, run by a person, once.
- No telemetry. Neither product learns the other is installed except by
  a command finding it on the machine.
- **AgentWorth ships without SpacePilot installed.** When the daemon's
  `/healthz` doesn't answer and `spacepilot doctor` isn't found either,
  every feature in the table above degrades to "not available, install
  SpacePilot" — never to a crash, a hang, or a silent no-op.

## Sequencing

1. **The contract first** — this document, and its mirror in SpacePilot's
   decision inbox. Nothing below starts until both sides agree what a call
   looks like.
2. **`asks --summarize` as the first consumer** — the smallest of the three
   uses, already scoped in `asks.md`, and the easiest to cut if the contract
   needs to change.
3. **Registry export as the first producer** — ATIF export already exists;
   wiring it to SpacePilot's registry is the first real trajectory this loop
   carries.

## Open questions

- **One binary or two?** Whether AgentWorth calls out to a separately
  installed SpacePilot process, or the two ship as one distribution with
  a feature flag, is undecided.
- **Contract versioning.** The three flagged calls need a version they can
  negotiate against, so SpacePilot can change its inference surface without
  silently breaking an older AgentWorth build.

## Pointer

SpacePilot's own decision record for this loop lives in that repo's
`docs/DECISION-INBOX.md` (`motionvector-dev/spacepilot`). This document is
AgentWorth's canon; that one is SpacePilot's — neither replaces the other.
