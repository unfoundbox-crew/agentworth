# The AgentWorth and SpacePilot loop

Status: direction, decided 2026-09-02; endpoints verified against
spacepilot fceecbf. Nothing built. This is a contract between two repos,
not a spec for either one's own build.

## The one-line version

AgentWorth feeds SpacePilot's registry dream, and SpacePilot feeds
AgentWorth's missing intelligence layer.

## Where each product stops

One platform, two products. Each runs alone. Together, they're the point.

| AgentWorth | SpacePilot |
| :--- | :--- |
| Grades trajectories — any agent, any harness, any machine. | Owns the fleet: nodes, open-weight models (Kimi, GLM, DeepSeek, and others), placement, the registry. |
| Never grows a fleet mode. On a SpacePilot cluster its scanner just reads wherever the fleet's agents write transcripts — OpenCode or SpacePilot's own harness — the same way it reads any other adapter today. SpacePilot doesn't collect transcripts itself; a node's transcripts are whatever its harness writes, and AgentWorth's scanner reads them per node. | Runs the agents that produce those transcripts, and reads AgentWorth's grades back into the registry. |
| Feeds `outcome_rate` and cost-per-verified-outcome (`verified-outcome-rate.md`) into the registry, once SpacePilot's corpus has somewhere to put an aggregate (not built yet — see Open questions). | At fleet scale, a group's `n` stops being something the `min_n` floor suppresses and becomes the registry's confidence level instead — more sessions, more trust in the number, never fewer numbers shown. |

Cross-machine identity runs on `key_id` (an Ed25519 public-key hash, one
per node), not on `system_id` (a hardware class like `apple-m1-max-32gb`,
shared by every identical machine). AgentWorth's merge keys on `key_id`.

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

Cost-per-verified-outcome can't be computed yet either way: SpacePilot's
`Measurement` record (one YAML file per run, under
`registry/measurements/<system_id>/<model_slug>/`) has `model_id`,
`runtime_id`, `quantisation`, `backend`, `wall_seconds`,
`peak_memory_bytes`, and a rate metric like `tokens_per_second` — no
`run_id`, no token counts, no energy, no cost. Joining AgentWorth's
outcomes to SpacePilot's runs needs `run_id` and token counts added to
`Measurement` first. That's a build item on SpacePilot's side too.

## What flows from SpacePilot to AgentWorth

Local inference, for three named uses, each meant to sit behind its own
flag: Archie's question router (`archie.md`, `--route`), entity
resolution for beliefs (`beliefs.md`, `--resolve-entities`), and
`asks --summarize` (`asks.md` tier 2, `--summarize`). Nothing else is
meant to call out to SpacePilot; a fourth use gets named here first.

**None of the three can be wired today — SpacePilot has no route that
takes a prompt and returns text.** Verified against spacepilot fceecbf:
`spacepilot serve` runs one FastAPI app on `127.0.0.1:8088` with
SpacePilot-shaped routes only (`/api/runtimes`,
`/api/compute/models/recommended`, `/api/compute/local-profile`,
`/local-status`, `/compatibility`, `/api/engines` — no chat, completion,
or embeddings endpoint among them). Text generation exists only through
the CLI's `run text` drivers, not over HTTP. `spacepilot daemon` is the
fleet process, not an inference server: a Unix-socket API serving
`/healthz`, `/v1/picture`, `/v1/plan`, `/v1/run`, `/v1/fleet/*`, plus a
tailnet-only peer port — none of it takes a prompt.

So this half of the loop is a **build item on SpacePilot's side**: an
OpenAI-compatible route, or an explicit decision not to have one.
Tracked on Saurabh's founder board. Until it lands, every feature above
reports "not available: SpacePilot has no inference route yet" — never a
crash, a hang, or a silent guess.

"Is SpacePilot here" is a reachability check against `spacepilot serve`'s
`/healthz`, falling back to `spacepilot doctor` when nothing is
listening — never a guess based on whether the binary is on `PATH`.

## The invariant

AgentWorth never sends a prompt to a model on its own (AGENTS.md's core
invariants). Once SpacePilot has a route to call, every one of the three
uses above must obey it the same way:

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
- **AgentWorth ships without SpacePilot installed.** When `spacepilot
  serve`'s `/healthz` doesn't answer and `spacepilot doctor` isn't found
  either, every feature above degrades to "not available, install
  SpacePilot" — never to a crash, a hang, or a silent no-op.

## Sequencing

1. **The contract first** — this document, and its mirror in SpacePilot's
   decision inbox. Nothing below starts until both sides agree what a call
   looks like.
2. **`asks --summarize` as the first consumer** — the smallest of the three
   uses, already scoped in `asks.md`, and the easiest to cut if the contract
   needs to change. Blocked until SpacePilot ships an inference route.
3. **An aggregate feed as the first producer** — not raw trajectories.
   SpacePilot's registry is two halves, a read-only catalogue and a corpus
   (`measurements/`, `systems/`); the corpus has no trajectory or session
   concept today. Wiring ATIF export straight into it would introduce one.
   The producer side should instead write an `outcome_rate` table and a
   cost table — aggregates keyed by model, runtime, system, and task class —
   into a new corpus subtree parallel to `measurements/`.

## Open questions

- **One binary or two?** Whether AgentWorth calls out to a separately
  installed SpacePilot process, or the two ship as one distribution with
  a feature flag, is undecided.
- **Contract versioning.** The three flagged calls need a version they can
  negotiate against, so SpacePilot can change its inference surface without
  silently breaking an older AgentWorth build.
- **Aggregates, or rows?** The aggregate feed in Sequencing above is a
  recommendation, not a decision — Saurabh owns the call. The alternative
  is per-session rows or a full ATIF export landing in SpacePilot's corpus,
  with AgentWorth staying the only place trajectories live either way.
- **`run_id` and token counts on `Measurement`.** Without them, joining an
  AgentWorth outcome to the SpacePilot run that produced it — and so
  computing cost-per-verified-outcome — has no key to join on. A build item
  on SpacePilot's side.

## Pointer

SpacePilot's own decision record for this loop lives in that repo's
`docs/DECISION-INBOX.md` (`motionvector-dev/spacepilot`). This document is
AgentWorth's canon; that one is SpacePilot's — neither replaces the other.
