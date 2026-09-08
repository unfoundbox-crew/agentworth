# Agent memory and harness integration, the field on 2026-09-08

Research record for `docs/specs/memory.md`. Four survey lanes plus four primary
spot checks, all fetched 2026-09-08. Every row names its source. A cell marked
NOT CONFIRMED came from a secondary source or was not on the primary page.
Vendor-reported numbers are labelled as such and are not treated as truth.

## 1. What every harness exposes

Nineteen harnesses surveyed: Claude Code, Codex CLI, Gemini CLI, OpenCode,
Cursor, Windsurf, Copilot CLI, Cline, Roo, Kilo, Aider, Amp, Goose, Zed,
Continue, Kiro, Warp, Factory, Augment.

| Surface | Harnesses | What it means for a plugin |
| :--- | ---: | :--- |
| MCP client, tools | 17 of 19 | the one near-universal wire; Aider has none, Zed delegates |
| one instructions file read at start (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `WARP.md`) | about 10 of 19, plus rules directories in 7 | the de facto session-start injection |
| hooks whose stdout enters context | 4 of 19: Claude Code, Codex (flagged), Copilot CLI, Kiro (weak) | the only live push path, three incompatible schemas |
| a compaction event an extension can hook | 3 of 19: Claude Code, Codex, Amp (no hook) | the weakest surface in the field |
| transcript on disk in a documented format | 5 of 19: Claude Code, Codex, OpenCode (SQLite), Copilot CLI, Aider | IDE agents keep a `state.vscdb` blob; the adapter problem is the ecosystem's |
| a native memory feature | 6 of 19: Cursor, Windsurf, Goose, Augment, Cline (convention), Kilo (deprecated) | all closed; none has a write API for a third party |

Verified on the primary page: Claude Code injects `SessionStart` and
`UserPromptSubmit` stdout as context; `PostCompact` is async and has no
documented context return. So re-injection after compaction rides on the next
`UserPromptSubmit`. Source: code.claude.com/docs/en/hooks.

Continue is retiring its own context-provider interface in favour of MCP.
Kilo deprecated its memory bank in favour of `AGENTS.md`. Augment announced a
cross-harness memory vault; it is proprietary with no third-party API.

## 2. Where MCP itself is going

Verified on modelcontextprotocol.io/specification/2026-07-28/changelog.

| Change, 2026-07-28 | Consequence for a memory server |
| :--- | :--- |
| protocol sessions and the `initialize` handshake removed; every request carries version and capabilities in `_meta` | there is no session-start primitive to hang memory on; "read at start" is always the harness's job |
| Roots, Sampling, Logging deprecated (SEP-2577) | a server cannot ask the model anything; memory is tools, full stop |
| `resources/subscribe` replaced by one opt-in `subscriptions/listen` stream | memory as a subscribable resource got harder, not easier |
| tasks moved to an extension, polling only | long extraction runs can be tasks; hydration cannot |
| `ttlMs` and `cacheScope` required on list and read results; deterministic `tools/list` order recommended for prompt-cache hits | cheap wins: stable tool order, honest TTLs |
| roadmap names "progressive discovery", a small entry point that reveals more as the conversation narrows | the direction is fewer tokens per tool surface; not shipped in the spec |

Client support: tools are universal; resources and prompts are missing in
Codex; sampling is absent in Claude Code and Codex. Claude Code caps MCP tool
output around 25k tokens (secondary source, NOT CONFIRMED on a primary page).

## 3. Memory servers that exist

| Project | Store | Unit | Session start | Writes memory by |
| :--- | :--- | :--- | :--- | :--- |
| official `memory` server | JSON file graph | entity, relation, observation | none; agent calls tools | agent |
| basic-memory | markdown files | note | none | agent |
| mcp-memory-service | SQLite-vec | chunk | NOT CONFIRMED | agent, consolidation |
| Graphiti MCP | Neo4j graph, bi-temporal | entity, edge | none | ingestion |
| Mem0, OpenMemory | vector plus graph | extracted fact | none | a model |
| Cognee MCP | graph engine | typed DataPoint | none | pipeline |
| Cline memory bank | markdown files | six docs | system prompt says read them | agent |
| claude-mem, verified on the repo: 93.5k stars, Apache-2.0, v13.24 | SQLite plus Chroma | "observation" of tool use, compressed by a model | `SessionStart` hook injects | a model |

Every one is tools, not resources. None publishes a token measurement for its
own payload. The nearest neighbour is claude-mem: same hook Archie uses, the
opposite representation. It asks a model to compress what the agent did;
Archie stores the receipt and reads the words back verbatim.

## 4. What has been measured

| Claim | Number | Who measured | Source |
| :--- | :--- | :--- | :--- |
| verbatim chunks beat model-extracted artifacts | +15.9 pts LoCoMo, +22.0 pts LongMemEval-S | independent controlled ablation | arXiv 2601.00821 |
| Mem0 v1 over full context | 91% lower p95 latency, 90% fewer tokens | vendor | arXiv 2504.19413 |
| Zep on LongMemEval | 71.2% | vendor; Mem0's table reports Zep at 63.8% | arXiv 2501.13956 |
| tool search over loading every schema | 85% fewer tokens | vendor, Anthropic | anthropic.com/engineering/advanced-tool-use |
| memory tool plus context editing | 84% token saving on a 100-turn task | vendor, Anthropic | secondary blog citing Anthropic |
| TOON over JSON on uniform arrays | about 40% fewer tokens | two arXiv papers, CSV still wins on flat tables | arXiv 2603.03306, 2605.29676 |
| a markdown rules file against a structured equivalent | none published | | |

Vendor leaderboards disagree with each other on the same system. The one
independent ablation puts extraction-based memory far below every vendor's
self-reported score. Design around the ablation, not the leaderboards.

## 5. Vendor memory converged on files

Anthropic's API memory tool is a directory of files the client executes and
owns. Letta moved to git-backed memory directories. Claude Code, Codex, Gemini
CLI and Goose all load a markdown file at start. Two independent vendors and
four harnesses chose inspectable files with history over a memory database.

## 6. Standards, ranked by who honours them

| Standard | Honoured by | Gives a memory tool |
| :--- | :--- | :--- |
| `AGENTS.md`, Agentic AI Foundation | about 60k projects, most harnesses | a write target every agent reads; no schema |
| Agent Skills `SKILL.md`, agentskills.io | Claude Code, Codex, Gemini CLI, Copilot, Cursor | a 100-token trigger; cannot carry an MCP server or hooks |
| ACP, verified registry: 50+ agents including claude-acp, copilot, cline, cursor, devin, gemini, goose | Zed, JetBrains as clients | a structured turn stream, but only to the ACP client; no third-party subscribe on the page |
| plugin manifests | every harness, none compatible | bundle MCP plus hooks plus skills per harness |
| ATIF, Harbor RFC 0001, v1.8 | NVIDIA NeMo and Arize Phoenix consume it | the trajectory interchange bet; OpenTelemetry GenAI is the tracing lane |
| hooks | no shared schema | one shim per harness |
| A2A, Linux Foundation, v1.0.1 | 150 organisations | inter-service, not this machine |

## What this settles for Archie

1. Files are the harness-agnostic read path. Reading each harness's transcript
   on disk, which Archie does with 22 adapters, reaches every harness on this
   list. No protocol does.
2. MCP tools are the harness-agnostic query path. Memory is a tool that
   returns rows, with deterministic tool order and honest TTLs. Not a resource.
3. Session-start injection is per harness and stays that way. Hook shims for
   the three harnesses whose stdout enters context, an `AGENTS.md` line and a
   skill trigger everywhere else, and `session_wake` as the universal pull.
4. Do not bet on compaction hooks, native memory features, or ACP as a capture
   path. Sparse, closed, or a bigger integration than a plugin.
5. The differentiator is the receipt. The largest neighbour compresses with a
   model; the only independent measurement says verbatim wins. Keep it.
