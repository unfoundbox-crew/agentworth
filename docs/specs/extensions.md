# The extension surface

Status: proposed, 2026-09-03.

## The one-line version

AgentWorth does not become a coding harness. Archie reaches people as an
extension inside the harnesses, shells, editors and pipelines they already run
— and this is the ranked list of those doorways, with what each one needs from
the core.

## Why a surface and not a product

`AGENTS.md` says AgentWorth is a cockpit above harnesses. `mcp-server.md` said
the same thing about screens: "the consumer of an answer is an agent over MCP,
or a person in a chat app — neither of them opens a pane." Both sentences point
the same way. The reach of this product is not how good its dashboard is; it is
how many places its answer can appear without anyone deciding to go and look.

So the ranking below is reach per token: how many people meet Archie for every
token spent building the doorway. That is a different order from what is
interesting to build, and it is deliberately not the order the panes were built
in.

## The table

| # | Extension | Reach | What it needs from the core | Exists today |
| :-- | :--- | :--- | :--- | :--- |
| L1 | The done gate (`done-gate.md`) | every stop, in every session, in two harnesses | one new tool, `session_gate` | nothing; the tool is new, the plugins are new |
| L2 | A GitHub Action on a pull request | every reviewer of every PR, including people with no AgentWorth installed | `repo_suspect` and `session_list --repo` over a checkout, plus a receipt format a developer can attach | both tools ship; the Action and the receipt format do not |
| L3 | A shell prompt segment | every prompt, all day, one line | `archie window show --plain` under 50 ms | the command ships and is fast enough — measured below. `--plain` exists; the one-line form does not |
| L4 | An editor extension | every file save, in the status bar | the local HTTP API | ships: `archie serve` and ten routes |
| L5 | Declarative adapters | every harness nobody has written Rust for | the adapter trait accepting a data-driven implementation | the trait ships; a manifest loader does not |
| L6 | Detector plugins | every session, as labels rather than as a surface | a detector interface and a sandbox | four detectors ship, none of them pluggable |
| L7 | Discovery | everyone who has not heard of it | a topic, a registry entry, and later `archie plugin add` | the Claude Code plugin ships (`.claude-plugin/`, the repo is its own marketplace); the community-marketplace listing is submitted by hand; the topic does not exist |

## Each one, and what it can honestly do

### L1 — the done gate

Its own spec, measured: `done-gate.md`. On this machine it would fire on 12.6%
of the sessions that claimed done, holding 2.5% of the corpus's tokens. Small
in spend, and at the exact moment a person is about to believe something.

The first extension because it is the only one that changes what an agent
*does*, rather than what a person can look at.

### L2 — a GitHub Action on a pull request

Post the rung and the sessions that produced a change, as a PR comment or a
check.

**Say plainly what it cannot know.** The Action runs on a runner. The
transcripts are on the developer's laptop and, by the product's own rule, they
never leave it. So the Action sees the diff, the commits, the branch — and any
receipt the developer chose to commit or attach. It does not see the local
index, cannot compute a rung from transcripts it has no access to, and must
never be built in a way that tempts someone to upload one.

Which leaves one honest design: **the receipt travels in the pull request
body.** The developer runs `archie repo suspect` and `archie session list
--repo` over their own checkout, gets a redacted receipt through the existing
`select → redact → preview → explicit approval → export` pipeline that
`AGENTS.md` requires, and pastes or commits it. The Action then does the small,
verifiable part: parse the receipt, check that the commits it names are the
commits in this PR, and render it as a check. An Action that reports "no
receipt attached" is doing its job; an Action that phones a laptop is not a
thing this product will ever ship.

`repo_suspect` already takes an absolute path to a checkout and resolves the
repository root with `git rev-parse --show-toplevel`, so the local half needs
no new query. The new work is a receipt format stable enough to parse, and the
Action itself.

### L3 — a shell prompt segment

The one-line Archie already exists in the brand kit
(`packages/ui/brand/archie/archie-tui.txt`), designed for exactly this slot:

```
 (*) archie  scanning  ──────────────────·······  68%  1,204 sessions
```

with `(*)` on, `( o )` sweeping, `(-)` nothing, `( )` error. The kit is
explicit that this is "the only form that fits a status bar, a spinner slot,
or the right edge of a prompt".

A prompt segment runs on every command, so the budget is a hard one: **under
50 ms, or it does not ship.** Measured 2026-09-03 on this machine's real index
(263 MB, 4,887 sessions), five runs each, `agentworth` 0.1.15 — the shipped
binary predates the 0.1.16 rename, so these are the old spellings of what is
now `archie window show` and `archie stats`:

| command | wall time, 5 runs |
| :--- | :--- |
| `agentworth usage --json` | 30, 10, 10, 10, 20 ms |
| `agentworth stats --json` | 30, 40, 30, 30, 30 ms |

Both clear the budget with room. The gap between them is the shape of the
query, not the size of the index, so the prompt segment should call the
cheaper one and never the aggregate. What is missing is only the rendering: a
`--plain` one-liner in the shape above, and the four state glyphs.

The kit also sets the constraint that matters most here: "In a terminal that
cannot repaint in place — a piped log, a CI transcript — print frame 1 once and
nothing after it. A loop that scrolls is a loop that lies."

### L4 — an editor extension

The ladder in the status bar, and the sessions that touched the open file.

This needs the least. `archie serve` already binds a local port and serves
fourteen routes, ten of them GET, including `/api/stats`, `/api/traces`,
`/api/traces/:id`, `/api/traces/:id/events` and `/api/blame` — the whole
surface an editor extension wants. The work is entirely on the editor side.

Two facts from `AGENTS.md`'s hard-won list apply before anyone starts.
`/api/traces` returns 50 rows by default and excludes stubs, so an extension
that filters client-side is filtering a slice while looking normal. And Rust
and TypeScript drift here: `types/index.ts` is a claim, not a contract — curl
the endpoint.

### L5 — declarative adapters

A manifest of paths and field mappings, loaded at runtime, instead of a Rust
file per harness.

`capability-matrix.md` is the argument. Twenty adapters, nine have ever
produced a row, two produce tokens, two produce outcomes. Most of that gap is
not hard parsing — it is that nobody has written the file. A manifest turns a
new harness into a data change.

**The invariant it must not break:** `AGENTS.md` says source-specific logic
stays inside adapters, and core code must not contain Claude-, Codex-,
Gemini-, or OpenCode-specific field handling. **A manifest is an adapter.** The
loader is core and knows nothing about any harness; every path, field name and
quirk lives in the manifest, which is the adapter in data form. The moment the
loader grows a branch for one harness's oddity, this has failed and the right
answer is a Rust adapter for that harness.

Two more rules carry over unchanged. A manifest declares its own
`PARSER_VERSION`, and bumping it re-parses files an incremental scan would
otherwise skip. And malformed records degrade gracefully rather than
invalidating a session — which is harder in a manifest than in code, and is the
main reason this sits at L5 and not higher.

### L6 — detector plugins

Loop, blunder and suspect rules as data or as sandboxed modules.

Four detectors ship inside the binary today: the loop sentinel
(`crates/outcomes/src/loops.rs`), demoted claims, recoveries, and suspect
commits. They are useful and they are opinionated — `convergence.md` measured
the loop detector firing on 25.0% of sessions and pointing at *busy* sessions
rather than wasteful ones, which is a fine thing to detect and not the thing
that spec needed. That is the case for letting someone write their own.

**A detector reads and labels. It never calls a model.** `AGENTS.md`'s
invariant is not relaxed for third-party code — if anything it tightens, since
a plugin that made a model call would do it with the user's key inside a tool
that promises not to. Rules as data first; sandboxed modules only if data
turns out to be too weak, and with the sandbox designed before the first
plugin exists rather than after.

### L7 — discovery

Three doorways, in the order they can be opened.

**MCP registries, now.** AgentWorth already ships a stdio MCP server, so it is
listable today with no new code. Verified reachable 2026-09-03 (HTTP 200):

| registry | URL |
| :--- | :--- |
| the official MCP registry | `https://registry.modelcontextprotocol.io/v0/servers`, source at `https://github.com/modelcontextprotocol/registry` |
| Smithery | `https://smithery.ai` |
| mcp.so | `https://mcp.so` |
| Glama | `https://glama.ai/mcp/servers` |

Listed as reachable, not as vetted — a 200 says the site answered, nothing
more. PulseMCP returned 403 to a plain request, which is a bot block and not
evidence either way, so it is left out.

**A GitHub topic, next.** `agentworth-plugin` has 0 repositories today
(checked 2026-09-03 via the GitHub search API). For scale: `dsh-plugin` has
13,309 and `mcp-server` has 27,072. A topic costs nothing and is the cheapest
place for a third-party plugin to be findable.

**`archie plugin add <repo>`, later.** Only once L5 or L6 gives it something
to install. A plugin command with no plugin format is a promise.

## Why this order

**L1 first because it is the only one that changes an outcome.** Everything
below it makes an answer visible to someone who already wanted it. The gate
puts the answer in front of an agent that was about to stop without it, which
is the one moment where a number changes what happens next. It is also
measured, which nothing else on this page is.

**L2 second because it is the only one that reaches people who do not have
AgentWorth installed.** Every other row on this table requires the reader to
have run `archie scan` on their own machine. A PR check is read by reviewers,
by the author's team, by anyone who opens the pull request. That is the widest
audience per token here by a large margin — and it is second rather than first
because half of it is a receipt format that does not exist yet, and because
getting the privacy story wrong here is the way this product becomes something
it promised not to be.

**L3 third because it is nearly free and it is the highest-frequency surface
there is.** The one-line Archie is already designed. The command is already
under budget, measured above. What is left is a formatter. Nothing else on this
page has a better ratio of remaining work to appearances per day.

**L4 fourth because it needs nothing from this repo at all.** The API ships,
the routes are documented in `REFERENCE.md`, and the work is in someone else's
editor. It sits below L3 only because an editor extension is a bigger build
than a prompt formatter, not because it is worth less.

**L5 and L6 are platform work and they are correctly last.** They multiply
everything above them — a declarative adapter makes the gate work for a
harness nobody wrote Rust for — but they multiply by zero until something
above them is worth extending. `capability-matrix.md` is the honest reason to
be careful: twenty adapters exist and two of them produce outcomes, so the
constraint has never been how easy it is to add an adapter. Build these when
somebody outside this repo has asked for one, and not before.

**L7 sits at the bottom and should be started at the top.** Listing an MCP
server that already ships and claiming a GitHub topic that has zero repos are
both an afternoon and neither blocks anything. They are last in the table
because they are not extensions; they are how the extensions get found. Do the
registry entries alongside L1, not after L6.

## What none of these change

Local-only, forever. No accounts, no telemetry, no sync, no hosted dashboard —
the same line `docs/specs/README.md` ends on, and none of these is the excuse.

Three of them look like the excuse and are not, so each gets a sentence:

- **The PR Action never sees a transcript.** It reads a receipt a person chose
  to attach, through the export pipeline that already requires explicit
  approval. It has no path to the local index and must never be given one.
- **A third-party plugin does not get a network.** Detector plugins read and
  label. An extension surface is not a reason to relax the invariant that
  AgentWorth never sends a prompt to a model on its own.
- **Discovery is a listing, not a callback.** A registry entry is a row in
  somebody else's catalogue. Nothing in this product learns that it was
  installed, and nothing phones anywhere on first run.
