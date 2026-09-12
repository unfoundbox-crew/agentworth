# Roadmap: what is left, in what order, and how to build it

Status 2026-09-12, after v0.1.23, each row checked against main at 0dde921. Sources: `docs/specs/README.md`, `docs/DECISION-INBOX.md`,
`docs/capability-matrix.md`, open PRs, and the index of the session that shipped v0.1.23.
Cost is in tokens for one lane, end to end (read, write, test, review).

## Priority matrix

Value: what a user gets. Cost: tokens to ship. P0 first, P3 last.

| P | Item | Why now | Cost | Lane type |
| :- | :--- | :--- | :--- | :--- |
| P0 | Spec index stale: `suspect-commits` is built, `agent-bus`, `home`, `home-latency` missing from the table | The roadmap doc misleads every new session | 100k | docs |
| P0 | Search model download is silent; README says "100% Offline, zero network calls" | Air-gapped user gets a bare failure; copy overstates | 250k | search + docs |
| P0 | Capability matrix measured 2026-09-02; Codex row still shows 1 session with tokens | Numbers predate #116 and v0.1.23; rescan and regenerate | 300k | docs + rescan |
| P1 | Land #133 (Claude Code plugin), then #134 (DSH plugin, stacked on #133) | #133 conflicts with main on `CHANGELOG.md` and `SKILL.md` only; #134 is clean on top | 400k | plugins |
| P1 | Land #167 (state.vscdb adapter for Cursor and VS Code forks) | Mergeable; needs fixture + PARSER_VERSION check | 500k | adapter |
| P1 | Land #135 + #138 (landing page, blog) | Content, waits on your read only | 100k | your call |
| P1 | Rescan and refresh verified-outcome-rate numbers | Every quoted number predates the exit-code fix | 150k | docs |
| P2 | done-gate: `session_gate` tool | Measured 2026-09-03, top extension doorway | 800k | loop + mcp |
| P2 | efficiency-receipts: `fanout_reads`, `repeat_check`, then `window receipt` | Measured; unblocks cli-grammar §4(3) | 900k | mcp + cli |
| P2 | agent-bus: `agent_messages`, `claims`, `archie msg send/inbox/ack` | Spec 2026-09-07; home deck needs it | 1.2M | storage + cli |
| P2 | Machine memory (#162 spec) | Spec open; typed facts with receipts; build after the bus | 1.5M | storage + loop |
| P2 | `session_list` over-fetch (4x) | Every MCP call pays it | 250k | mcp |
| P2 | beliefs: `claim_check` tool | Measured, not built | 600k | mcp |
| P3 | Desktop app | Blocked on index-ownership decision | your call | — |
| P3 | Local search embeddings, archie-bench aggregate export, spacepilot-loop | Preconditions not met or no consumer yet | — | — |
| P3 | Voice spec #144, docIR terminal demos | Product and marketing, your read first | — | — |

Decisions only you can make: merge #135 and #138; whether #144 proceeds; desktop index ownership.
Everything else above is engineering and gets decided in the lane.

Checked and closed, not on the list: the coverage page reads a per-adapter table, not a constant;
`real_verified_rate` uses a matched denominator (live value 47.2%); `/api/traces/:id` sits behind a
compression layer; the `archie home` test is green on CI, its one red was on a self-hosted box.

## Tech spec: fan-out with modular TDD

One coordinator, many lanes, one gate per train. The coordinator never reads a transcript,
a fixture, or a diff itself. It reads reports.

### Roles and models

| Role | Model | Does | Never does |
| :--- | :--- | :--- | :--- |
| Coordinator | Fable | Picks lanes, writes briefs, merges reports, decides | Reads files, runs builds, reads transcripts |
| Reader | Haiku | Transcripts, docs, logs, fixtures, screenshots via zero-vision | Writes code |
| Builder | Sonnet | One lane: red test, green code, fixture, self-test table | Touches a second lane's files |
| Reviewer | Opus | Reads the diff against the brief, names what the test does not prove | Fixes what it finds |

Vision goes to `agy` Flash or the zero-vision skill. Frontier eyes only for a design verdict.

### A lane

A lane is one row of the matrix, one branch, one PR, one crate or app touched.
Two lanes never share a file. If two rows touch the same file, they are one lane.

Every lane runs the same five steps. The builder reports each with a receipt.

1. **Measure.** Query the index or the fixture before writing a number. Paste the query and result.
2. **Red.** Write the failing test first. For an adapter or CLI change, the test is a redacted
   fixture scanned into a temp SQLite index and exercised through the real binary
   (`apps/cli/tests/doctor_self_test.rs` is the pattern). Fixture paths come from
   `agentworth_schema::fixtures`, never a literal.
3. **Green.** Smallest change that passes. Bump `PARSER_VERSION` if parse output changed.
4. **Prove.** `cargo test -p <crate>` on the build box, plus `agentworth doctor --self-test`
   against the fixture index. Paste the pass/fail table. A skipped step is reported as skipped.
5. **Report.** Under 300 words: what was measured, what changed, what the test proves, what it
   does not, and every `NOT CONFIRMED` item. No chips; findings outside scope go in the report.

### The brief

The coordinator writes one brief per lane. It states what was measured and what was assumed.
A lane that finds an assumption wrong says so and stops. It does not route around it.

```
Lane: <matrix row>
Files you own: <paths>          Files you must not touch: <paths>
Measured: <query, result>        Assumed: <claim, unverified>
Red test: <name, what it asserts>
Done when: <the self-test table is green and the report has no open NOT CONFIRMED>
```

### The train

Independent lanes merge in one train. The train gets one full-suite run on the final tree,
on the build box or in CI, never on the laptop. A red bisects; only the implicated lane re-runs.
Merging to main is your call. Everything before it is the coordinator's.

### Budget

A reader lane costs under 100k. A builder lane costs what the matrix row says.
A reviewer pass costs about a third of the builder lane. The coordinator's own spend
should stay under 10% of the train. If it climbs, a lane brief was too thin.
