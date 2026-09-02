# MCP server

Status: built. Implemented in `apps/cli/src/mcp/` (module, not a new crate) as
the `agentworth mcp` / `agwt mcp` subcommand, on `rmcp` 3.2.0. Six read-only
tools ship exactly as decided below: `sessions_find`, `session_get`,
`blame_find`, `usage_summary`, `pacing_window`, `coverage_stats` (with
`include_matrix` folded in, not a separate tool). See `README.md`'s "MCP
Server" section for the registration command and `apps/cli/src/mcp/tests.rs`
plus `apps/cli/tests/mcp_stdio_test.rs` for coverage. Originally written as a
draft spec for someone implementing this in a fresh session with no memory of
how this doc came to exist -- kept below as the design record.

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
| `events_offset` | integer, optional, default 0 | zero-based offset into `trace.events` |
| `events_limit` | integer, optional, default **500** | max events returned; must be > 0 (0 is rejected as invalid params) |

**Implementation note (added when pagination shipped):** `trace.events` on
a real session can run to tens of thousands of entries — tens of MB of
JSON — and a remote model asking for "the session" with no further
qualification used to get that in full. `events_limit` now defaults to
500 rather than unbounded, so a call without these params can never
return a session's whole event list by accident; pass a larger
`events_limit` explicitly to see more. The response carries `events_total`
(the session's real event count, independent of how many events this
particular call returned) and `events_offset` (the offset actually
applied), so a caller can tell "sliced" from "this session genuinely has
few events" and knows how far there is left to page. Score, outcomes, and
recoveries are always computed from the *full*, unsliced trace first —
detection accuracy shouldn't depend on which page of events was
requested — and only `trace.events` itself is sliced afterward (then
redacted, unless `include_raw` is set). The same slicing helper
(`paginate_events`, `apps/cli/src/server/routes.rs`) backs `GET
/api/traces/:id`'s own `offset`/`limit` query params, so the two surfaces
share one pagination contract.

`score` is the five-component `TraceScore` (`crates/scoring/src/scorer.rs:38`:
`outcome_score`, `verifiability_score`, `complexity_score`, `recovery_score`,
`provenance_score`, `composite_score`, plus human-readable `explanations`).
`outcomes` is `Vec<OutcomeEvidence>` (`kind`, `summary`, `confidence`).
`recoveries` is `Vec<RecoverySignal>` (`failure_sequence`, `failure_summary`,
`recovery_sequence`, `recovery_summary`, `steps_to_recover`).

**Done, ahead of tool implementation** (backend session, 2026-09-01):
`Redactor::redact_outcome_evidence(&[OutcomeEvidence]) -> Vec<OutcomeEvidence>`
and `Redactor::redact_recovery_signal(&[RecoverySignal]) -> Vec<RecoverySignal>`
now exist in `crates/redaction/src/redactor.rs`, redacting `summary` /
`failure_summary` / `recovery_summary` / `correlated_files` the same way
`redact_trace` already redacts `trace.events`. When `session_get` gets
built: compute `score`/`outcomes`/`recoveries` from the raw trace first
(detection accuracy shouldn't depend on redacted text), then run
`outcomes`/`recoveries` through these two functions on the *same* redactor
instance used for the trace — see the repository-identity note right below,
which is exactly why "same instance" matters now.

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

Returns `{ rows, cost_basis, subscription_tier }`. `rows` is
`Vec<UsagePeriodSummary>` (`period`, `adapter`, `session_count`, token
breakdown, `total_duration_seconds`, `estimated_cost_usd`,
`cache_hit_ratio`). `cost_basis` is always `"api_list_price_equivalent"`
— every `estimated_cost_usd` here is computed from the public API price
list, not what the account actually paid. `subscription_tier` is present
when `~/.claude.json` names one (e.g. a Claude subscriber's plan); its
presence means the cost figures above are not the account's real bill.

`agentworth usage`'s own richer rollup (`--period day|week|month|year|all`,
`--by adapter|model|repo`, `--since`, honest period-count `--limit`) isn't
exposed over MCP yet — this tool still mirrors the older adapter-grouped
`/api/usage` shape.

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

**Fixed, not just flagged** (backend session, 2026-09-01): the named-rule
gap above was real — the redaction engine's rules cover API keys, JWTs, env
vars, credential URLs, emails, private IPs, PEM keys, and home-directory
usernames, but nothing matched a repository or project name, so even
"redacted" output leaked it via `source_path`. `crates/redaction/src/rules.rs`
now has `repository_identity_rule(repo_or_workspace: &str) -> Option<RedactionRule>`,
which builds a literal-match rule for one session's own identity (derived
via `agentworth_schema::extract_repository_or_workspace`, moved there from
`agentworth-storage` — still re-exported from storage so existing callers
don't change — specifically so `agentworth-redaction` could reach it
without taking on storage's SQLite dependency). `Redactor::for_trace(&trace) -> Self`
returns a redactor augmented with this trace's own identity rule;
`redact_trace` calls it internally now, so every existing caller (`export
--redact`, and this tool once built) gets repository-name protection for
free. `session_get`'s `include_raw: false` path should build its redactor
via `Redactor::new().for_trace(&trace)`, then use that same instance for
`redact_trace`, `redact_outcome_evidence`, and `redact_recovery_signal` —
that's what makes the repository-identity rule apply to all three instead
of just the trace object. Real tests in
`crates/redaction/tests/redactor_test.rs` cover the rule builder, the
end-to-end trace case, and the composition across trace/outcomes/recoveries
via one `for_trace`-augmented instance. Verified on lenovo — see this
repo's `docs/DECISION-INBOX.md` for the real build/test output.

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

- Both redaction questions that used to be here — whether `outcomes`/
  `recoveries` redaction should be a shared `agentworth-redaction` function,
  and whether the repository-name gap gets fixed before this ships or
  documented as a caveat — are resolved. See the "New work"/"A real gap"
  notes above: both landed as real `agentworth-redaction` functions
  (`redact_outcome_evidence`, `redact_recovery_signal`, `for_trace`), not a
  documented caveat. `/api/traces/:id?redact=true` (mentioned as a possible
  second consumer) doesn't exist yet — whoever adds it should reuse these
  same functions rather than one-off logic, but that route itself is still
  unbuilt and out of scope here.
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

## Implementation notes (resolved during the build)

- **The outcome-encoding fix has landed on main** (confirmed against
  `crates/outcomes/src/outcome.rs` and `crates/storage/src/lib.rs`'s
  `get_aggregate_stats` query, both snake_case). `sessions_find`'s `outcome`
  parameter is passed through raw to `SessionFilter.outcome`, unvalidated —
  same choice `SessionFilter` itself already makes, so the tool layer isn't
  inventing a new contract. Revisit if a client turns out to routinely pass
  a stale PascalCase value.
- **`limit` out of range is rejected, not clamped.** Both `limit == 0` and
  `limit > 200` return an `invalid_params` MCP error naming the valid range,
  rather than silently clamping — clamping would repeat exactly the
  "looks complete but isn't" shape this tool exists to avoid.
- **`coverage_stats`'s `include_matrix` reuses `compute_adapter_matrix`
  directly** (widened from private to `pub(crate)` in
  `apps/cli/src/server/routes.rs`) rather than re-deriving the 20-adapter
  capability table — one definition, shared by `/api/matrix` and this tool.
- **Tracing goes to stderr for the `mcp` subcommand specifically.** The CLI's
  existing global tracing setup defaults to stdout, which would corrupt the
  stdio JSON-RPC stream for any log line emitted while a client is attached.
  `apps/cli/src/main.rs` now branches on `Commands::Mcp` before initializing
  the subscriber. This wasn't called out above; it's the kind of thing that
  only surfaces once you actually try to run an MCP server on top of a CLI
  that already logs.
