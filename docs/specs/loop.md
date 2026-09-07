# Loop

Status: built, PR #TBD (2026-09-06). Facts below were read from the three
codebases and Claude Code's hooks reference on 2026-09-06; each carries its
source. Where the build diverged from the design, a **Shipped:** note sits
next to the paragraph it changes, same convention as `handoff.md`.

## The one-line version

Archie stops being a flight recorder that reads the tape afterwards and
becomes the return path: the harness tells it what an agent is about to do,
Archie records what actually changed, and any agent can ask "what moved under
me, and who moved it." One field joins that to what SpacePilot measured and
what MotionVector made.

## What is true today

| Fact | Source |
| :--- | :--- |
| Herdr's socket is JSON-lines, and it is not request/response only: it also supports `events.subscribe` (protocol 20), which pushes `pane_agent_status_changed` events without polling. Confirmed live on 2026-09-07 -- `apps/cli/src/server/home/gateway.rs`'s `subscription_loop` holds one such connection open per `archie serve --home` process; the `home-gateway` lane's report has the captured frames. The `pane.report_agent_session` method (sent by hook scripts on session start) still exists alongside it. Idle/working can now be pushed as well as pulled through the `herdr` CLI. Herdr's source is not on this machine | `apps/cli/src/server/home/gateway.rs`; `~/.claude/hooks/herdr-agent-state.sh`, `~/.cursor/…`, `~/.codex/…`; `live-show-spike/src/types.ts:64` |
| Claude Code fires hooks on `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `SubagentStart/Stop`, `PreCompact/PostCompact`, `CwdChanged`, `SessionEnd`, each with `session_id`, `cwd`, `transcript_path`, and for tool events `tool_name`, `tool_input`, `tool_use_id`, `tool_response`. `async: true` hooks never block the agent | code.claude.com/docs/en/hooks, fetched 2026-09-06 |
| `Measurement.run_id` is "the join key. One uuid per served call, so a caller that holds an outcome (AgentWorth holds cost per outcome) can find the run that produced it." Machine is `system_id` → `System.host_fingerprint`, a truncated salted hash | `spacepilot/spacepilot/measurements.py:130-194` |
| `Receipt` v1 has `document.canonical_sha256`, `render.out_sha256`, `machine.{os,arch,target,adapter}`. No id, no time, no session | `runtime-ops/src/receipt.rs:19-108` |
| AgentWorth has `session_id`, `source_path`, `content_fingerprint`, and since v0.1.19 `metadata.workspace.{cwd,git_branch}`. No machine id anywhere. Its live path is a filesystem-notify tail into SSE; no socket, no daemon | `crates/schema/src/{trace,provenance}.rs`, `apps/cli/src/server/live_tail.rs` |

So: herdr is a source Archie can join to (its hooks put `HERDR_PANE_ID` in the
environment) and, since `events.subscribe`, a bus Archie can subscribe to as
well for presence -- `apps/home`'s gateway does both. The harness's own hooks
remain the bus for everything herdr itself doesn't push (tool calls, file
changes, outcomes), which is exactly what herdr uses.

## 1. The loop

```
  harness hook ──stdin JSON──▶ archie hook ──UDS line──▶ archie serve (loop)
   (async, exit 0 always)        │ no socket?               │
                                 └──▶ ~/.agentworth/spool/  │ state machine + SQLite
                                        ▲                   ▼
                                  archie scan ingests   agent_status / session_drift
```

- `archie hook` reads the hook JSON on stdin, connects to `~/.agentworth/archie.sock` with a 50 ms budget, writes one line, exits 0. No socket, no time: append to `~/.agentworth/spool/<session_id>.jsonl`. The agent is never slowed and never sees an error.

  **Shipped:** the classifier that turns a `PreToolUse` event into predicted write paths (`classify`) also takes the event's `cwd`, not just `tool_name`/`tool_input`. A relative `Edit`/`Write` path is common and resolving it against the session's own working directory, rather than the daemon's, is what keeps the predicted-path set matching what actually lands on disk.
- `archie serve` owns the socket (a Unix socket is loopback by construction; same JSON-lines framing herdr uses). It keeps `agent_state` per session: `registered → working → idle → ended`, from `SessionStart`, `UserPromptSubmit`, `Stop`, `SessionEnd`. Subagents (`agent_id` present) update their parent's `last_seq`, never its state, for the reason herdr's script gives: a subagent must not revive an idle pane.
- `archie scan` ingests the spool, so the loop works with no server running, offline, and the spool is a raw history like any other adapter's.
- Efference copy, literally. `PreToolUse` is the copy of the motor command: tool, input, and the write set Archie predicts from it (`Edit`/`Write`/`NotebookEdit` → the path; `Bash` → the command string, write set unknown). `PostToolUse`/`PostToolUseFailure` is the reafference: what came back. At `Stop`, Archie compares: `git status --porcelain` and HEAD in the session's `cwd` against the union of this session's predicted writes since its last `Stop`. A changed path nobody in this session predicted is exafference: the world moved. Archie names the mover when another session predicted that path, else "not an agent on this machine".

  **Shipped:** the `Stop` comparison is stored as JSON in `agent_state.last_stop`, not as rows in a dedicated table. It's one report per session per `Stop`, read whole every time (`agent_status`, `session_wake`'s "Moved under you" line) and never queried by column — a table bought nothing a JSON blob didn't already give, at the cost of a migration.
- `support_set` U, narrower than the read set: the paths this session `Read`, hashed at `PostToolUse`. `session_drift` re-hashes U and returns the entries that changed, each with the session that wrote it and its sequence. That is the answer to "did my ground move because of me or someone else", which no harness summary can give.

  **Shipped:** `support_from_read` — the function that turns a `Read` tool result into a `support_set` row — takes the read's own sequence number as a parameter, rather than looking it up again from storage. The caller already has it off the event it's processing; threading it through avoids a second query per read on what can be a high-frequency path.
- Heartbeat is not a timer. It is `Stop`: the moment the agent goes idle is the moment the loop closes, and the next `session_wake` reads a closed loop, not a tape.

## 2. The shared field

`run_id`, minted by the thing that runs, quoted in the tool result, indexed
by Archie. SpacePilot already mints it and already documents it as the join
key. The transcript is the carrier: the agent's own tool result contains the
uuid, so Archie indexes it without either product learning about the other.

| Record | Field today | Archie indexes it as | Ask of that product |
| :--- | :--- | :--- | :--- |
| SpacePilot `Measurement` | `run_id`, `system_id` | anchor `run_id`; `machines.system_id` | none; adopt its `host_fingerprint` algorithm so ids match |

**Shipped:** `system_id` and `host_fingerprint` turned out not to be the same
thing. `host_fingerprint` identifies the box — the salted sha256[:12] over
stable hardware facts, the value that has to match SpacePilot's for the join
to work. `system_id` is a configuration id (SpacePilot mints one per install,
and a box can be reinstalled or reconfigured without becoming a different
machine). So `machines` is keyed on `host_fingerprint`, with `system_id`
carried as a column, not the key — two configurations on one physical
machine resolve to one `machines` row, which is what "the same box" should
mean.
| MotionVector `Receipt` v1 | `document.canonical_sha256`, `render.out_sha256` | anchor `sha256`, hashed at `PostToolUse` on the document and the output path | Receipt v2 adds `run_id` printed by `mvec render`; hashes join until then |
| Herdr pane | `HERDR_PANE_ID` in the hook environment | `agent_state.pane_id` | none |

"This task, on that machine, produced this output" is then one query:
`sessions ⋈ machines ⋈ trace_anchors` against `Measurement.run_id` or a
Receipt's hashes. No shared database, no sync, no telemetry, as
`spacepilot-loop.md` requires.

## 3. Grammar, crates, schema

| Surface | Addition |
| :--- | :--- |
| CLI, top level | `archie hook` (plumbing, beside `scan` and `mcp`); `archie hook print claude` prints the settings.json snippet and never writes it |
| CLI, nouns | `archie agent status` (live states from the loop), `archie session drift [id]`, `archie session anchors [id]` |
| MCP | `agent_status`, `session_drift`; `session_wake` gains a "Moved under you" line when U drifted |
| Crates | new `crates/loop`: socket server, spool, state machine, prediction and drift, pure and testable; `adapters` gains a spool adapter; `storage` gains the tables; `apps/cli` wires serve, hook, MCP |

Schema, all `CREATE TABLE IF NOT EXISTS` plus one guarded `ALTER`, the
mechanism storage already uses:

| Table | Columns |
| :--- | :--- |
| `machines` | `system_id` PK, `host_fingerprint`, `os`, `arch`, `first_seen` |
| `sessions` | `+ system_id` |
| `agent_state` | `session_id` PK, `state`, `since`, `pane_id`, `cwd`, `git_head`, `last_seq`, `updated_at` |
| `tool_intents` | `tool_use_id` PK, `session_id`, `seq`, `tool`, `predicted_paths` JSON, `command`, `at`, `result` |
| `support_set` | (`session_id`, `path`) PK, `sha256`, `read_seq`, `read_at` |
| `trace_anchors` | (`session_id`, `seq`, `kind`, `value`) PK, index on (`kind`, `value`); kinds `run_id`, `sha256`, `pane_id` |

## What stays true

Never uploads, works offline (the spool), never calls a model, raw histories
stay the source (the spool is one), the hook can never block or fail the
agent, the socket is loopback by construction. One stance changes and is
stated: Archie now receives events while work is happening. It still never
scans on its own from MCP, and it still never writes into a repo.

## Cost, and what is not in v0.1.21

About 1.4M tokens across six lanes: loop crate 300k, storage and anchors
200k, drift and support set 250k, CLI and MCP 200k, hook client and fixtures
150k, review 150k plus one full-suite gate. Claude Code hooks only; the
Cursor and Codex hook shapes herdr's scripts imply are not verified here.
`git status` at `Stop` on a large checkout costs tens of milliseconds and runs
async. Files over 4 MB are not hashed into U; they are listed as unhashed.

**Shipped:** the Claude-Code-only scope held. Cursor and Codex hooks were not
built in v0.1.21 — `archie hook print claude` is the only harness snippet
that ships; a Cursor or Codex user still has no path onto the socket.

**Shipped, and it differs from the design above:** drift and the Stop
report name writers by time (`writers_of_path_since`, joined through
`tool_intents.at`), not by sequence: a sequence counts one session's own
events, so one session's seq 5 says nothing about another's seq 2. The
loop's five session-keyed tables are registered with `archie merge`;
`machines` is not, since it has no session id.
