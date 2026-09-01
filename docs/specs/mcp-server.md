# MCP server

Status: draft spec, not yet built. Written for someone implementing this in a
fresh session with no memory of how this doc came to exist.

## The problem

The owner runs many long coding-agent sessions across several machines and
repos. Every day he hand-writes handoff files so the next session knows what
the last one did. A better dashboard does not fix this — a human still has
to look at a screen, read it, and retype a summary. The fix is for a session
to ask directly: "what was I doing in spacepilot yesterday," "which sessions
touched `api.ts`," "what did I decide about the outcome enum." Today nothing
can answer that without a human in the loop. This spec gives a session a way
to ask the local index those questions itself, over MCP.

AgentWorth already has the data. `crates/storage/src/lib.rs` indexes it,
`apps/cli/src/server/routes.rs` exposes eight of nine handlers as JSON over
HTTP (`/api/stats`, `/api/traces`, `/api/traces/:id`, `/api/matrix`,
`/api/blame`, `/api/usage`, `/api/pacing`, `/api/archaeology`, plus
`POST /api/scan` and `POST /api/export/:id`). What's missing is a surface an
agent can call mid-session without a human opening a browser tab first.

One thing this spec is not: `crates/adapters/src/mcp.rs`. That file
normalizes MCP tool names found *inside* recorded traces (`mcp__server__tool`
→ `mcp:server:tool`) so the scanner can tell which MCP servers a past session
called. It has nothing to do with AgentWorth exposing itself as an MCP
server. No server exists in this repo today.

## Transport

**stdio, and only stdio for v1.**

MCP defines two transports: stdio and Streamable HTTP (the successor to the
now-deprecated HTTP+SSE transport, standardized since March 2025). stdio is
the one every coding agent already speaks fluently — Claude Code, Codex, and
Cursor all spawn stdio MCP servers as subprocesses routinely — and it has no
listening socket at all, which fits "local-only, forever" more literally
than binding `127.0.0.1` does. `agentworth serve` already binds a port for
the dashboard; stdio doesn't need one.

**Should `agentworth serve` also expose an MCP-over-Streamable-HTTP
endpoint?** Only as a later fast-follow, not in v1. What it would buy: a
client that already has `agentworth serve` running for other reasons (the
dashboard, a forwarded port in a dev container) could reach the same tools
without spawning a second process. What it costs: Streamable HTTP needs
session and origin handling stdio doesn't, and it would be a second
transport carrying the same tool set that has to stay in sync with the
first. Since the actual consumer here — a coding agent's own MCP client —
already defaults to spawning a stdio subprocess, there's no real user this
unblocks in v1. Build it only if someone asks for it after stdio ships.

## Where the server lives

Follow the precedent already in this repo: `apps/cli/src/server/routes.rs`
is not its own crate — the HTTP surface lives directly inside `apps/cli`,
composing `agentworth-storage`, `agentworth-outcomes`, `agentworth-scoring`,
and `agentworth-redaction`. Do the same for MCP: a new `apps/cli/src/mcp/`
module, not a new workspace crate. It shares the same `Storage` and
`Scanner` instances the HTTP server already builds (`AppState` in
`routes.rs`), so a running `agentworth serve` and a running `agentworth mcp`
read the identical SQLite index with no duplicated wiring.

This needs one new dependency: **`rmcp`**, the official Rust MCP SDK
(`modelcontextprotocol/rust-sdk`, 4.7M+ downloads on crates.io, current
release line 3.x, implements the 2026-07-28 spec, supports both stdio and
HTTP transports with a macro-driven tool API). Verified via its GitHub repo
and docs.rs listing; not currently a dependency of this workspace.

New CLI surface: a `Commands::Mcp` subcommand (`agentworth mcp` /
`agwt mcp`), alongside the existing `Scan`, `Serve`, `Search`, etc. in
`apps/cli/src/main.rs`. It opens the default (or `--db-path`-specified)
`Storage`, builds an `rmcp` stdio server, registers the tools below, and
blocks on stdin/stdout until the parent process closes the pipe — the same
lifecycle every other stdio MCP server has.

## The tool surface

Every tool here is grounded in a query `crates/storage/src/lib.rs` or
`apps/cli/src/server/routes.rs` can already answer. Nothing below invents a
new column, endpoint, or crate beyond what each tool's "New work" line
states plainly.

### `sessions_find`

Wraps `Storage::list_sessions_filtered(&SessionFilter)`
(`crates/storage/src/lib.rs:683`).

| Param | Type | Maps to |
| --- | --- | --- |
| `repo` | string, optional | not a `SessionFilter` field — see below |
| `adapter` | string, optional | `SessionFilter.adapter` (exact match) |
| `model` | string, optional | `SessionFilter.model` (substring, `LIKE %model%` on `models_used`) |
| `outcome` | string, optional | `SessionFilter.outcome` — see the encoding warning below |
| `search` | string, optional | `SessionFilter.search` (substring across `session_id`, `source_path`, `models_used`, `adapter`) |
| `start_date`, `end_date` | RFC 3339 string, optional | `SessionFilter.start_date` / `end_date` |
| `min_tokens` | integer, optional | `SessionFilter.min_tokens` |
| `order_by` | enum, optional | `SessionFilter.order_by` — one of `started_at_desc` (default), `started_at_asc`, `tokens_desc`, `tokens_asc`, `events_desc`, `events_asc`, `duration_desc`, `score_desc`, `score_asc` |
| `limit` | integer, **required, no silent default** | `SessionFilter.limit` |
| `offset` | integer, optional | `SessionFilter.offset` |
| `include_stubs` | boolean, optional, default false | `SessionFilter.include_stubs` |

**`repo` is not a stored column.** There is no `repo` field on the
`sessions` table — only `source_path`. `extract_repository_or_workspace()`
(`crates/storage/src/lib.rs:1094`) derives a repo/workspace name from
`source_path` at read time, and it's already used this way for
`get_top_repositories()`. The tool implementation fetches a
filter-matched, ordered slice and post-filters in Rust by
`extract_repository_or_workspace(&s.source_path) == repo`, the same
client-side-filter-after-a-bounded-fetch pattern `fleet-view.md` already
uses for mtime. Cheap, no schema change, but means `repo` can't be combined
with `limit` as a hard cap — the tool must over-fetch (say, `limit * 4`,
capped) before repo-filtering, and say so if it truncates.

**The `limit` default trap.** `/api/traces` defaults to 50 and silently
excludes stub sessions when no `limit` is given
(`AGENTS.md`, "Things you cannot learn from the code," item 3) — a client
that forgets to ask for more gets a partial answer that looks complete.
Don't repeat that here: make `limit` a required parameter on this tool, with
a hard ceiling (say 200) rather than a silent default, so a remote model
calling this tool is forced to state how much it's asking for.

**The `outcome` encoding gap.** `docs/specs/README.md`'s "build this first"
item is that `primary_outcome` is stored PascalCase (`"CommitObserved"`) in
SQLite while `OutcomeKind`'s serde form and the rest of the API contract are
snake_case. Confirmed still true in this snapshot:
`crates/storage/src/lib.rs:713` does `primary_outcome = ?` as a raw string
compare, and `crates/core/src/lib.rs:172` writes it via
`outcome_kind_name()`, not the serde encoding. Until that fix lands, this
tool's `outcome` parameter has to accept the *actual on-disk* PascalCase
values (`DoneClaimed`, `ArtifactChanged`, `TestOrBuildPassed`,
`CommitObserved`, `CiOrDeploymentVerified`) to work at all, which is an ugly
parameter contract to ship. Land the backend encoding fix first, as
`docs/specs/README.md` already says, then this tool's `outcome` param can
use the same snake_case values every other consumer expects.

**Return shape:** an array of session summaries (`session_id`, `adapter`,
`source_path`, `started_at`, `duration_seconds`, `total_tokens`,
`total_events`, `tool_calls_count`, `models_used`, `primary_outcome`,
`composite_score` — the `SessionSummary` fields, `crates/storage/src/lib.rs:70`)
plus a `truncated: bool` flag (true when `repo` post-filtering or the hard
`limit` ceiling cut real results). No total-match count — `list_sessions_filtered`
doesn't compute one, and adding a `COUNT(*)` variant is unscoped new work,
noted under Open questions instead of assumed away.

`source_path` is redacted per the policy below regardless of this tool's own
redaction setting, since a list of raw absolute paths is exactly the kind
of thing this tool exists to hand to a possibly-remote model.

### `session_get`

Wraps `Scanner::load_trace(&id)` plus `TraceScorer::score()`,
`OutcomeDetector::detect_outcomes()`, `RecoveryDetector::detect_recoveries()`
— the same four calls `get_trace_by_id_handler` makes
(`apps/cli/src/server/routes.rs:312`), returning the same shape
`/api/traces/:id` does: `{ trace, score, outcomes, recoveries }`
(`TraceDetailResponse`, `routes.rs:113`).

| Param | Type | Notes |
| --- | --- | --- |
| `session_id` | string, required | |
| `include_raw` | boolean, optional, default **false** | see Redaction below — this is the one parameter this whole spec turns on |

`score` is the five-component `TraceScore` (`crates/scoring/src/scorer.rs:38`:
`outcome_score`, `verifiability_score`, `complexity_score`, `recovery_score`,
`provenance_score`, `composite_score`, plus human-readable `explanations`).
`outcomes` is `Vec<OutcomeEvidence>` (`kind`, `summary`, `confidence`).
`recoveries` is `Vec<RecoverySignal>` (`failure_sequence`, `failure_summary`,
`recovery_sequence`, `recovery_summary`, `steps_to_recover`).

**New work, not currently true of any existing code path:** neither
`/api/traces/:id` nor anything else redacts `outcomes` or `recoveries`.
`Redactor::redact_trace` (`crates/redaction/src/redactor.rs`) only walks
`trace.events`, `trace.provenance.source_path`, and `trace.metadata` — it
never touches `OutcomeEvidence.summary` or `RecoverySignal.failure_summary`
/ `recovery_summary`, which are free text derived from event content and
can carry the same secrets an event can. The only place redaction is wired
up at all today is `POST /api/export/:id`, and only for the `trace` object.
This tool has to add what doesn't exist yet: after computing `score`,
`outcomes`, and `recoveries` from the **raw** trace (detection accuracy
shouldn't depend on redacted text), run every `OutcomeEvidence.summary` and
`RecoverySignal.failure_summary` / `recovery_summary` through
`Redactor::new().redact_text()` before returning, exactly as
`trace.events` already do via `redact_trace`. This is a small addition
(the primitive already exists, `redact_text` is public), not a new
subsystem — but it does not exist today and someone has to write it.

### `blame_find`

Wraps `Storage::find_sessions_for_blame(&file_path_pattern)`
(`crates/storage/src/lib.rs:966`), the same call `/api/blame` makes.

| Param | Type |
| --- | --- |
| `file_path` | string, required — substring pattern, matches `/api/blame`'s `file`/`path` query param |

Returns `Vec<BlameMatch>` (`session_id`, `adapter`, `source_path`,
`started_at`, `models_used`, `total_tokens`, `tool_calls_count`,
`file_path`, `action`, `modified_at`, `model`) — redacted per policy.

### `usage_summary`

Wraps `Storage::get_daily_usage` / `get_weekly_usage` / `get_monthly_usage`
(`crates/storage/src/lib.rs:797-807`), same as `/api/usage`.

| Param | Type |
| --- | --- |
| `period` | enum, required — `day`, `week`, or `month` |
| `limit` | integer, optional, default matches the route's own defaults (30 / 20 / 12) |

Returns `Vec<UsagePeriodSummary>` (`period`, `adapter`, `session_count`,
token breakdown, `total_duration_seconds`, `estimated_cost_usd`,
`cache_hit_ratio`).

### `pacing_window`

Wraps `Storage::get_pacing_window(hours)` (`crates/storage/src/lib.rs:864`),
same as `/api/pacing`. This is the tool that answers "what am I burning
right now" — `fleet-view.md`'s addendum's second daily question.

| Param | Type |
| --- | --- |
| `hours` | integer, optional, default 5 (matches the route default) |

Returns `PacingSummary` (`burn_rate_tokens_per_hour`, `active_adapters`,
`active_models`, token breakdown, `estimated_cost_usd`, `cache_hit_ratio`).

### `coverage_stats`

Wraps `Storage::get_aggregate_stats()` (`crates/storage/src/lib.rs:516`,
same as `/api/stats`) and, optionally, the adapter matrix computation
`/api/matrix` already does (`compute_adapter_matrix`,
`apps/cli/src/server/routes.rs:451`).

No parameters. Returns `AggregateStats` (`total_sessions`, `total_events`,
`token_usage`, `sessions_by_adapter`, `models_usage_count`,
`tools_usage_count`, `verified_outcomes_count`, `first_session_at`,
`last_session_at`) plus, if the caller passes `include_matrix: true`,
`AdapterMatrixResponse` (`total_adapters`, `detected_adapters`, per-adapter
detection/format/capability rows). This is the tool that answers "what does
this machine even have" without a human opening the dashboard's Overview
tab first.

### What's deliberately not in v1

- **No `scan_trigger` tool.** A tool that runs `Scanner::run_scan` would let
  a remote model kick off a filesystem scan across the whole machine on its
  own initiative, with no human watching. That's a bigger blast radius than
  every read-only tool above combined, for a benefit (auto-refreshing a
  stale index) a human can get just as well by running `agentworth scan`
  themselves before asking questions. Keep the v1 surface 100% read-only;
  document `agentworth scan` as a prerequisite, not a tool call. Revisit
  only if staleness turns out to be a real problem in practice — see Open
  questions.
- **No semantic-search tool.** `docs/specs/local-search.md` argues MCP
  should ship before embeddings, and that most questions are exact-match
  SQL, not similarity search. Don't pre-empt that by wiring `agwt search`'s
  vector store into a tool here. If embeddings prove worth shipping, add
  the tool then.
- **No `/api/export` or `/api/archaeology` equivalents.** Export already has
  its own explicit, opt-in redaction flow through the CLI/HTTP surface;
  duplicating it as an MCP tool doesn't add anything an agent mid-session
  needs. Archaeology is presentation logic for the dashboard's forensics
  view (`compute_archaeology_highlights`), not a new query — skip it unless
  something asks for it specifically.

## What it must not expose

Session logs carry prompts, shell commands, tool output, file diffs, and
whatever secrets happened to be in scope when the agent ran. This server
hands that content to whatever process holds the other end of the pipe —
which, for a coding agent's own MCP client, may itself be backed by a
remote model. This is the sharpest privacy question in the whole design,
and here is the direct answer, not a hedge:

**Redacted is the default for every tool that returns event or file
content. Raw is opt-in, per call, never global.**

- `sessions_find` and `blame_find` never return full event content in the
  first place — only summary rows (`SessionSummary`, `BlameMatch`) — but
  their `source_path` fields go through the redaction engine's home-path
  rules regardless, since even a bare list of absolute paths and repo names
  is more than a remote model needs to answer "which sessions touched this
  file."
- `session_get` defaults `include_raw` to **false**. Redacted, it runs the
  trace, its outcome summaries, and its recovery summaries through
  `Redactor` before returning (see the New work note above — this
  redaction of `outcomes`/`recoveries` doesn't exist yet and has to be
  built as part of this tool, not assumed from existing code).
  `include_raw: true` returns the unredacted trace. There is no
  server-side global setting that flips the default; every call chooses.

**A real gap this exposes, worth stating plainly rather than papering
over:** the redaction engine's 13 rules (verified by reading
`crates/redaction/src/rules.rs` directly — not the 15 the product brief for
this doc assumed) cover API keys, JWTs, env vars, credential URLs, emails,
private IPs, PEM keys, and home-directory usernames. **They do not cover
repository names or project directory names.** `AGENTS.md`'s own privacy
section lists "repository names" alongside secrets and absolute paths as
things default export must avoid leaking — but the only rule touching paths
strips the *username* segment of a home directory (`/Users/saurabh` → `~`),
leaving the rest of the path, including the repo name, intact. So today,
even "redacted," `session_get` will hand a remote model the real repository
name via `source_path` and `trace.provenance.source_path`. That's a real
product gap, not a hypothetical — fixing it is redaction-engine work, out
of scope for this doc, but it should be fixed before `include_raw: false`
is trusted as actually safe to hand to a remote model by default. Flagged
under Open questions.

**Nothing here changes what's on disk.** Every tool reads through
`Storage`/`Scanner` the same way the HTTP routes already do. No tool writes
to the original session logs; `Redactor::redact_trace` already only ever
produces a sanitized copy (`crates/redaction/src/redactor.rs`), same
guarantee `AGENTS.md`'s "Never modify original histories" already states.

## Registration

Wire it into Claude Code with the standard local-stdio-server flow
(verified against Claude Code's current MCP docs, September 2026):

```bash
claude mcp add agentworth --scope user -- agentworth mcp
```

`--scope user` matters here more than it does for a typical MCP server:
the whole point is a session in *any* repo being able to ask about *any*
other repo's history ("what was I doing in spacepilot yesterday," asked
from inside a totally different checkout). A project-scoped `.mcp.json`
entry would only be live in one repo at a time, which defeats that.

Equivalent hand-written entry, in `~/.claude.json` under the top-level
`mcpServers` key (or `.mcp.json` at project scope, if a team ever wants
that instead):

```json
{
  "mcpServers": {
    "agentworth": {
      "type": "stdio",
      "command": "agentworth",
      "args": ["mcp"]
    }
  }
}
```

No environment variables, no auth — stdio servers run as the local user's
own process, same trust boundary as running `agentworth` from a terminal
already has.

## Why this beats a better dashboard

A dashboard needs a human to open it, read it, and retype what mattered
into a handoff file for the next session. That human is the actual
bottleneck the owner is trying to remove — a nicer chart doesn't remove a
person from the loop, it just gives them a nicer screen to transcribe from.

An MCP tool lets the *next session* ask directly, in the same turn it
needs the answer, and get back structured data it can act on — not prose
it has to re-read and re-interpret. "Which sessions touched `api.ts`" as an
MCP tool call returns a list the calling agent can iterate over
programmatically; the same question against a dashboard returns a screen a
human has to look at and then explain. The dashboard isn't going away —
`fleet-view.md` and `trajectory-scrubber.md` are still worth building for
the times a human genuinely wants to look — but it was never going to solve
the specific pain this spec targets, because a screen still requires a
person.

## Decisions made here

- stdio only for v1; Streamable HTTP deferred, not ruled out.
- The MCP surface lives in `apps/cli/src/mcp/`, not a new crate, mirroring
  where the HTTP routes already live.
- `rmcp` is the SDK to build on — official, actively maintained, supports
  both transports for when HTTP is revisited.
- Seven read tools (`sessions_find`, `session_get`, `blame_find`,
  `usage_summary`, `pacing_window`, `coverage_stats`, with `include_matrix`
  folded into `coverage_stats` rather than a separate tool) cover every
  question named in the brief. No write tool ships in v1.
- Redacted is the default output for anything carrying event or file
  content; raw is an explicit per-call opt-in, never a server-wide switch.
- `limit` is a required parameter on `sessions_find`, with a hard ceiling,
  specifically to not repeat the `/api/traces` 50-default trap.

## Open questions

- Should `outcomes`/`recoveries` redaction land as a shared
  `agentworth-redaction` function (`Redactor::redact_outcome_evidence`,
  `Redactor::redact_recovery_signal`) usable by both this MCP tool and a
  future `/api/traces/:id?redact=true` route, rather than one-off logic
  inside the MCP handler? Feels like the right shape but isn't decided
  here.
- The repository-name gap in the redaction engine (no rule strips project/
  repo names from paths) — is this fixed in `crates/redaction` before
  `mcp-server.md` ships, or does `session_get`'s redacted mode ship with a
  documented caveat that repo names still leak? These are different launch
  postures and a human should pick one.
- Is a `scan_trigger` tool ever wanted, gated behind an explicit
  confirmation round-trip the way the redaction opt-in is, rather than
  omitted entirely? Left out of v1 above, but "never" wasn't argued for —
  only "not without more thought."
- Does `sessions_find`'s missing total-match count matter enough to add a
  `COUNT(*)` variant to `list_sessions_filtered`, or is `truncated: bool`
  sufficient for how a model is likely to use pagination?
- Once the `docs/specs/README.md` outcome-encoding fix lands, does
  `sessions_find`'s `outcome` parameter get validated against the known
  snake_case enum values at the tool layer (reject an invalid value with a
  clear error) or passed through raw the way `SessionFilter.outcome`
  already does?
