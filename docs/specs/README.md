# Specs, and the order to build them

Six specs sit beside this file. They are independent of each other but not of
the backend, and two of them are worth less than they look until a bug lands
first. This is the sequencing.

## Build this first, or the rest measures nothing

**The `OutcomeKind` encoding fix.** The database stores PascalCase while the
server, the API contract and the whole frontend expect snake_case. Until it
lands, every session in a real index reads as unresolved, `verified_outcomes_count`
is zero for everyone, and the outcome layer the dashboard is built around is
displaying nothing. Owned by the backend session, top of their queue.

Nothing below is worth judging until this is true.

## Then, in this order

| # | What | Spec | Depends on | Shape |
| :-- | :--- | :--- | :--- | :--- |
| 1 | `busy_timeout` on SQLite connections | — | nothing | one pragma, one test |
| 2 | Populate `prompt_preview` | — | nothing | backend, small |
| 3 | Resizable session column | — | nothing | frontend only |
| 4 | Group by repo / worktree / subagent | — | nothing | frontend only |
| 4b | MCP server | `mcp-server.md` | the outcome-encoding fix, for one filter param | backend, new module, no frontend |
| 5 | Fleet strip, mtime inference | `fleet-view.md` | surfacing `sources.mtime` | frontend + one field |
| 6 | Trajectory scrubber zoom and pan | `trajectory-scrubber.md` | nothing | frontend, the largest of these |
| 6b | Cache economics | `cache-economics.md` | nothing | arithmetic on data already parsed |
| 6c | Compaction awareness | `compaction.md` | nothing | one flag already in the logs |
| 7 | SSE endpoint | `fleet-view.md` | file watcher | backend, shared with `agentworth watch` |
| 8 | Desktop app | `desktop-app.md` | 7 is nicer with it | config, then signing |
| 9 | Local search / embeddings | `local-search.md` | 4b, and real usage showing structured queries aren't enough | backend, and only if 4b shows it's warranted |

### Why that order

**1 and 2 are cheap and unblock judgement.** No busy timeout means a write
collision returns `SQLITE_BUSY` immediately, and `agentworth serve` alongside
`agentworth scan` already reaches that today. An empty `prompt_preview` is why
sessions are unrecognisable — `agent-af702e89` tells a human nothing, and the
field built to fix that has never been filled. Neither is glamorous; both change
what every screen shows.

**3 and 4 are free wins already asked for.** Repo, worktree and subagent are all
derivable from `source_path` with no NLP and no embeddings — a 500-session
sample yields 23 distinct repos, and 440 of those 500 were subagent runs. Do
these before anyone reaches for a vector database.

**4b sits right after the free frontend wins, ahead of the fleet strip,**
because it is pure backend, needs no UI at all, and answers the actual
question `fleet-view.md`'s addendum names as the real one: a session asking
what it did yesterday, not a human reading a screen. It only waits on the
top-of-file outcome-encoding fix for one filter parameter, nothing else in
this table.

**5 before 6** because the fleet strip is smaller, ships without streaming, and
answers a question first: is the live direction interesting at all? If nobody
looks at it, 7 gets cheaper to decline.

**6b rides alongside 6** because both read the same per-event timestamps, and
because it needs no backend at all — `cache_read_tokens`, `cache_creation_tokens`
and a timestamp are already on every model invocation. It is the cheapest real
insight left in the repo.

**6 is the biggest frontend piece here.** At 6,978 events the current strip is a
solid bar. Zoom and pan turn it from a decoration into an instrument, and the
spec argues for moving the axis from sequence to time so that idle gaps become
visible instead of invisible.

**7 makes 5 honest.** mtime inference says "probably running". A file watcher
says "running". The spec is deliberate about not letting the UI claim certainty
it does not have until then.

**8 is last because it changes nothing about what the product does.** A `.dmg`
is a distribution format. It is worth doing when the thing being distributed is
worth installing.

**9 is last, and conditional, on purpose.** `local-search.md` argues that most
questions people actually ask are exact-match SQL that 4b already answers, and
that embeddings only earn their place for "sessions like this one" or "what
was this session about" — real, but not the daily case. Build 4b, see what it
can't answer, then decide whether 9 is worth it.

## What none of these change

Local-only, forever. No accounts, no telemetry, no sync, no hosted dashboard —
that decision is recorded internally and is not up for revisiting here. A
desktop app does not become the excuse. Neither does
a fleet view that happens to look like a monitoring product.
