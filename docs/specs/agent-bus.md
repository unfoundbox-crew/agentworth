# Agent bus

Status: proposed 2026-09-07, not built. Written by the home director session
after a day of relaying between four harnesses by hand. Facts below are marked
measured (seen this session), documented (read in a primary doc), or assumed.

## The one-line version

Any agent on the machine leaves a message for any other agent, and Archie
delivers it at a moment that is neither too early nor too late, exactly once,
with the receiver told what moved since it was written.

## What already exists (measured 2026-09-07)

| primitive | what it gives the bus |
| :--- | :--- |
| `archie serve` with `~/.agentworth/archie.sock` and SQLite | a daemon that is already up, a store that is already durable |
| the hook loop (`archie hook`) | a call from the harness at every turn boundary: prompt submitted, tool about to run, stop. The gate answers in one 50 ms budget and fails open |
| `session_wake` | the first tool a cold agent calls; an inbox belongs in its output |
| herdr `events.subscribe` | idle / working / blocked / done per pane, pushed. A refusal to prompt a blocked agent |
| home gateway `steer` with `mode: after` | wait for a pane to settle, then send verbatim. Built on branch `home-substrate` |
| Claude Code peer messages | session-to-session delivery today, Claude Code only. This spec generalises it |
| `agent_state`, `tool_intents`, `session_drift` | what moved under a session since a point in time, already recorded |
| identity: harness session id as the key, names as sightings | the address of a message is the session id; a name resolves through sightings |

## Envelope

```jsonc
{
  "id": "01J...",                     // ulid, time-ordered
  "from": { "session": "61eef7db-…", "name": "partner-harvey" },
  "to":   { "session": "1501430c-…" } // or { "name": "luis" } or { "role": "executor" } or { "all": true }
  "kind": "directive" | "handoff" | "warning" | "reply" | "ack",
  "body": "verbatim text, never paraphrased by the bus",
  "reply_to": null,
  "causal": {                         // what the sender believed when writing
    "repo": "/Users/…/agentworth",
    "head": "f4f9a9d",                // git HEAD of the sender's view of the target's worktree
    "paths": ["crates/adapters/src/herdr.rs"],
    "fingerprints": { "crates/adapters/src/herdr.rs": "sha256:…" }
  },
  "deliver": { "when": "idle" | "turn" | "now", "priority": 0 },
  "expires_at": "2026-09-07T20:00:00Z",
  "claim": null                       // or { "key": "issue:#149", "token": "…" } for single-consumer work
}
```

The bus never edits `body`. The relay that paraphrases is the failure this
exists to end.

## Delivery

```
queued ──► deliverable ──► delivered ──► acked
   │            │              │
   │            └─ stale ──────┘ (delivered with a drift note, or dropped by policy)
   └─ expired · superseded
```

Who buffers: `archie serve`, table `agent_messages`, WAL mode.

Who checks: the deliverer, at each delivery opportunity, never at send time.
A message is judged against the target's world at the moment it could land.

Who delivers, in order of preference, the first path that applies:

| path | harness | moment | how "not too early" is met |
| :--- | :--- | :--- | :--- |
| hook injection | Claude Code (documented: `UserPromptSubmit` stdout becomes context), Codex (assumed: hooks accepted, untested on a real install) | the next turn boundary the harness itself reports | the harness is between turns by definition |
| herdr prompt | any pane, including agy and Cursor | when herdr reports idle; refused while blocked; `after` waits for settle | herdr's own presence, already pushed to us |
| MCP pull | any agent with the agentworth MCP | when the agent asks: `inbox_read`, and inside `session_wake` | the agent chose the moment |

"Not too late" is a check at delivery: `git rev-parse HEAD` in the target's
cwd against `causal.head`, and Archie's fingerprints against
`causal.fingerprints`. On mismatch the message is delivered as stale with the
drift attached ("herdr.rs changed twice since this was written, by w9-pj"),
or dropped when `deliver.priority` says the sender would rather it died.
`session_drift` already computes the note.

## Single consumption, no cloud

SQLite is the arbiter. A claim is one statement:

```sql
UPDATE agent_messages SET claimed_by = ?1, claimed_at = ?2
 WHERE id = ?3 AND claimed_by IS NULL;
```

`changes() = 1` for exactly one claimant under SQLite's write lock. For work
items rather than messages, `claims(key PRIMARY KEY, agent, at)` with
`INSERT OR IGNORE`: the row exists once, the first writer owns it, the second
reads who. No lease server, no lock file, no daemon beyond the one that runs.

## Harness agnostic, by construction

Nothing here asks a vendor for a change. Hooks where a harness has them, herdr
where it has a pane, MCP everywhere. A harness with none of the three still
gets the message on its next `session_wake`.

## Prototype in archie, in order

1. Table `agent_messages` and `claims`; `archie msg send|inbox|ack`; MCP tools
   `msg_send`, `inbox_read`, `inbox_ack`. Fold unread count and the first
   three lines into `session_wake`. One lane, tests on a fixture index.
2. Hook delivery: `archie hook` on `UserPromptSubmit` drains the session's
   deliverable messages into its stdout, stale ones annotated. Verify the
   documented injection on a real Claude Code turn with the probe pane before
   building on it.
3. Herdr delivery: the home gateway's `steer` with `mode: after`, addressed by
   session id, for panes without hooks. Already built; wire it to the queue.
4. Measure: message written to message read, per path, on the probe pane. No
   number goes in a doc before it is on a CSV.

## Not confirmed

- Codex hooks accepted by a real Codex install (noted untested in the loop
  work of 2026-09-06).
- Whether agy or Cursor expose any hook at all. The herdr path covers them
  regardless.
- The exact Claude Code hook fields for context injection. Read the hooks
  reference before step 2, do not build from memory.
