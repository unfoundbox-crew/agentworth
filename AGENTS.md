# AGENTS.md

## Mission

Build AgentWorth: a local-first native tool for discovering, normalizing, and understanding AI-agent histories already present on a user's machine.

V0 should answer:

> What useful AI experience already exists on this machine?

The immediate product is not a marketplace.

## Technology

Use:

* Rust for the core
* Rust for the CLI
* SQLite for the local index
* streaming filesystem/JSONL processing
* React + TypeScript only for the optional localhost UI
* a thin npm wrapper for `npx agentworth`

Do not make Node or Python a runtime dependency of the core product.

All distribution methods should execute the same native binary.

## Core invariants

* Never upload user data without explicit user action.
* Scanning must work offline.
* Raw histories remain the source of truth.
* Do not duplicate entire histories into AgentWorth storage.
* Parse large files as streams.
* Assume individual files may be multi-GB.
* Treat agent formats as unstable.
* Keep source-specific logic inside adapters.
* Preserve provenance for derived claims.
* Never infer success only because an agent says it succeeded.
* Prefer deterministic extraction before LLM-based analysis.
* Keep marketplace logic outside the core scanner.
* Gemini / Antigravity (gemini-3.7-flash): Never rush or take shortcuts. Read the full specification, understand the structural intent, and execute thoroughly — other agents are not in a hurry, and accuracy beats hasty completion.

## Pipeline

```text
source discovery
    ↓
adapter
    ↓
NormalizedEvent[]
    ↓
AgentWorthTrace
    ↓
features + outcomes + sensitivity
    ↓
SQLite index
    ↓
CLI / local UI / export
```

## Repository shape

```text
crates/
  core/
  schema/
  adapter-sdk/
  adapters/
  storage/
  outcomes/
  scoring/
  redaction/
  export-atif/

apps/
  cli/
  web/

packages/
  npm-wrapper/
```

## Adapter contract

Each adapter owns:

* source detection
* session enumeration
* parsing
* token-accounting semantics
* source metadata
* format/version quirks

Core code must not contain Claude-, Codex-, Gemini-, or OpenCode-specific field handling.

Malformed records should degrade gracefully rather than invalidate an entire session.

Adapters should expose a common Rust trait resembling:

```rust
trait AgentAdapter {
    fn detect(&self) -> Result<DetectionResult>;
    fn enumerate(&self) -> Result<Box<dyn Iterator<Item = SessionSource>>>;
    fn parse(
        &self,
        source: &SessionSource,
    ) -> Result<Box<dyn Iterator<Item = NormalizedEvent>>>;
}
```

Exact signatures may evolve; separation of responsibilities should not.

## Canonical model

Use `AgentWorthTrace` internally.

A trace should represent:

* user and agent messages
* model invocations
* tool calls/results
* files and artifacts
* shell commands
* errors and retries
* human interventions
* model switches
* token usage
* tests/builds/benchmarks
* outcome evidence
* provenance
* sensitivity/redaction state

Do not make ATIF the internal database schema.

Export to ATIF when useful.

## Outcome hierarchy

Prefer stronger evidence:

```text
agent says "done"
    <
artifact changed
    <
test/build passed
    <
commit observed
    <
CI / PR / deployment verified
```

Store confidence and supporting evidence for every inferred outcome.

## Storage

SQLite stores:

* source paths
* fingerprints
* session metadata
* derived features
* scores
* outcomes
* provenance references
* redaction metadata

Do not copy complete raw transcripts into SQLite.

Raw content should be loaded lazily from the original source when required.

## Incremental scanning

Fingerprint sources using values such as:

```text
path
size
mtime
content fingerprint
adapter version
```

On rescans:

* skip unchanged files
* resume append-only JSONL where safe
* reprocess when adapter/schema changes require it

## Performance

Assume:

* tens of thousands of sessions
* tens or hundreds of GB of logs
* corrupt or partially written records
* append-only histories

Requirements:

* bounded memory
* streaming parsers
* incremental rescanning
* batched SQLite writes
* cancellation
* progress reporting
* no raw-log duplication

Optimize correctness before micro-optimizing throughput.

## Privacy

Before export:

```text
select
  ↓
redact
  ↓
preview
  ↓
explicit approval
  ↓
export
```

Never modify original histories.

Default share/export behavior must avoid leaking:

* secrets
* `.env` values
* API keys
* private keys
* credentials
* repository names
* absolute paths
* organization names
* personal information

## Scoring

Do not estimate monetary value without real buyer demand.

Use an explainable `TraceScore` based on properties such as:

* outcome quality
* verifiability
* trajectory richness
* complexity
* recovery signal
* provenance
* rarity

Every score component should be inspectable.

## V0 priority

Build in this order:

1. canonical schema
2. Claude Code adapter
3. streaming scanner
4. SQLite index
5. Codex adapter
6. Gemini/OpenCode adapters
7. outcome extraction
8. TraceScore
9. local trace explorer
10. safe export
11. ATIF export
12. network/bounty experiments

When uncertain, optimize for:

> correct normalization of real, messy traces

over abstraction elegance, UI polish, or premature marketplace functionality.

## GitHub Multi-Account & Doppler Secrets Conventions

To maintain strict separation between personal and autonomous collective operations:

1. **Primary Account (`@unfoundbox`)**:
   - Secret Name: `GITHUB_TOKEN`
   - SSH Key: `~/.ssh/id_ed25519`
   - Role: Personal repositories, studio website (`unfoundbox.com`).

2. **Autonomous Collective (`@unfoundbox-crew`)**:
   - Secret Name: `GITHUB_CREW_TOKEN` (Never overwrite `GITHUB_TOKEN`).
   - SSH Key: `~/.ssh/id_ed25519_unfoundbox_crew` (Configured Host: `github.com-crew`).
   - Role: Public tool repos (`agentworth`, `memes`, `commongain`, `worldtrainer`).
   - Command Execution: Always use `GITHUB_TOKEN=$GITHUB_CREW_TOKEN gh ...` or SSH host `git@github.com-crew:...`.