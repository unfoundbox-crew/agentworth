> **Correction, verified 2026-09-01.** An embedding pipeline already exists in
> this repo — `crates/storage/src/{chunker,embedder,vector}` and a working
> `agwt search` command. This spec was briefed as greenfield and is not.
> Read the existing code before designing anything.
>
> The model download from Hugging Face is a dependency fetch, not telemetry —
> it sends nothing about the user. Document it; do not treat it as a privacy
> problem.
>
> The redaction engine has **15** rules, not 13: anthropic_api_key,
> aws_access_key, bearer_token, credential_url, email_address, github_token,
> google_api_key, jwt_token, linux_home_path, macos_home_path,
> openai_api_key, pem_private_key, private_ip_address, sensitive_env_vars,
> windows_home_path. None of them covers repository or project names, which
> AGENTS.md says exports must not leak. That gap is real.

# Local search and the small-model angle

Status: draft spec, not yet built (mostly). Written for someone implementing
this in a fresh session with no memory of how this doc came to exist.

## The honest ordering

**Ship `docs/specs/mcp-server.md` first. Embeddings may never be needed.**

Walk through what the owner actually asks, from `fleet-view.md`'s addendum
and this doc's own brief: which sessions touched this file, what ran in
this repo yesterday, what did I decide about the outcome enum, what am I
spending per model. Every one of those is an exact-match filter over fields
that already exist in SQLite — `source_path`, `adapter`, `started_at`,
`primary_outcome`, `models_used`. None of it is fuzzy. `sessions_find`,
`blame_find`, `usage_summary`, and `pacing_window` in `mcp-server.md`
answer all four with a `WHERE` clause, not a vector index.

Embeddings earn their place for exactly two questions structured filters
can't answer: **"find sessions like this one"** (similarity, not identity —
nothing in the schema says two sessions are "alike") and **"what was this
session about"** (a gist, when the literal words in the prompt don't match
what you'd type to search for it). Both are real. Neither is what gets
asked daily. Build the MCP server, watch what people actually ask it, and
only build the rest of this document if a real question shows up that the
structured tools can't answer.

## What's already built — read this before designing anything

This is not a green-field spec. `crates/storage/src/` already has
`chunker.rs`, `embedder.rs`, and `vector/` (`mod.rs` +
`vector/sqlite_store.rs`), and `apps/cli/src/commands/search.rs` wires them
into a working `agwt search "<query>"` CLI command. None of it is wired
into the dashboard, the HTTP API, or (per `mcp-server.md`) the MCP surface.
The rest of this document evaluates what's there against the four questions
the brief asked, rather than designing from scratch.

```
TrajectoryChunker::extract(trace)          crates/storage/src/chunker.rs
        │  5 chunk kinds per session
        ▼
LocalEmbedder::embed_batch(texts)          crates/storage/src/embedder.rs
        │  fastembed ONNX, 384-dim, or deterministic hash fallback
        ▼
SqliteVectorStore::insert_embeddings(...)  crates/storage/src/vector/sqlite_store.rs
        │  BLOB storage, same SQLite file
        ▼
SqliteVectorStore::search_filtered(...)    brute-force cosine similarity
        │
        ▼
agwt search "<query>"                      apps/cli/src/commands/search.rs
   (only consumer — not the dashboard, not /api/*, not MCP)
```

### What gets embedded — already decided, and it's a good decision

`TrajectoryChunker` (`crates/storage/src/chunker.rs`) extracts five chunk
kinds per session (`ChunkKind`, `crates/schema/src/vector.rs`):

| Kind | What it captures |
| --- | --- |
| `SessionSummary` | First user message + final outcome/assistant text + model + token stats — one per session |
| `ErrorRecovery` | A tool failure paired with the assistant's next corrective turn |
| `ToolInvocation` | Destructive or critical commands (`rm -rf`, `git reset`, `DROP TABLE`, etc.) |
| `ApologyPanic` | Assistant retreat/apology/confusion turns |
| `CodeLineage` | Significant file diffs |

This is finer-grained than "first user message plus a tool/file summary,"
and it's the right call: it targets exactly the moments a human-written
handoff note would call out — what happened, what broke, what recovered,
what changed — rather than one flat blob per session. `SessionSummary`
alone is the answer to "what was this session about." Nothing here needs
redesigning; if embeddings ship, ship this chunking as-is.

### Where the model comes from — this is where the real gap is

`LocalEmbedder` (`crates/storage/src/embedder.rs`) wraps `fastembed`
(`BAAI/bge-small-en-v1.5`, 384-dim ONNX), with a deterministic
hash-embedding fallback if ONNX initialization fails. `fastembed` is a
**default** Cargo feature on `agentworth-storage`
(`crates/storage/Cargo.toml`: `default = ["fastembed"]`) — it ships in
every normal build, not behind an opt-in flag.

**Verified, not assumed:** `fastembed`'s `TextEmbedding::try_new` downloads
model weights from the Hugging Face Hub (or a GCS mirror) on first use and
caches them locally afterward — confirmed against the `anush008/fastembed-rs`
README and an open `qdrant/fastembed` issue describing that download path.
This means **the first time anyone runs `agwt search`, this binary makes an
outbound network call, with no prompt and no consent screen.** The
`CHANGELOG.md` entry for `agwt search` ("backed by a FastEmbed ONNX
embedding engine that runs fully offline") is only true *after* that first
download succeeds, or on a machine where it fails and the deterministic
fallback silently takes over instead.

This is not a hypothetical tension with `AGENTS.md`'s "Scanning must work
offline" and "Never upload user data without explicit user action." It's a
shipped contradiction of the framing this project uses about itself
everywhere else — `fleet-view.md`'s addendum and `desktop-app.md`'s open
questions both independently assert "AgentWorth has never made an outbound
network call," and as of this snapshot that's no longer true for two
separate reasons: this one, and `agwt blunder --submit`
(`apps/cli/src/commands/blunder.rs`), which POSTs a redacted incident
report to `stfuopus.lol` when a user passes `--submit`. The `blunder` case
is at least gated behind an explicit flag a human has to type. The
`fastembed` download isn't gated behind anything — it fires the first time
someone runs a command whose name doesn't suggest networking at all.

**The deterministic fallback is not a semantic embedding.** Read
`deterministic_hash_embedding()` (`crates/storage/src/embedder.rs`): it's
unigram/bigram/character-trigram feature hashing into a normalized
384-dimensional vector — a structured bag-of-words, not a model that
understands paraphrase or synonymy. It will find "rm -rf repository
deletion error" close to "rm -rf repository deleted by accident" (shared
words), not close to a paraphrase with no shared vocabulary. Whichever
embedding source ends up gated by default until a human opts in, be
accurate about what it buys: on a machine that hasn't downloaded the ONNX
model, `agwt search` is closer to fuzzy keyword search than semantic
search. That actually strengthens the case for MCP-first: if the honest
default is closer to keyword matching, embeddings buy even less over
`sessions_find`'s substring search than the ideal case would suggest.

**Decision needed, not made here:** pick one.

| Option | What it costs |
| --- | --- |
| Explicit runtime consent gate — first `agwt search` call prints "this downloads a ~133MB model from Hugging Face, proceed?" and defaults to the hash fallback until approved | No binary-size cost; preserves "explicit user action"; adds one interaction the first time |
| Bundle the ONNX model in release artifacts | `model.onnx` fp32 is ~133MB (verified against the `BAAI/bge-small-en-v1.5` Hugging Face repo file listing); an int8 quantized version is ~32MB. Real cost to a project that currently ships a single small native binary per platform |
| Drop `fastembed` from `default` features; ship the hash fallback by default, ONNX as an explicit opt-in build or runtime flag | No network call ever without a human explicitly requesting the better model; permanently weaker default search quality unless someone opts in |

This doc recommends the first option — it's the only one that costs neither
binary size nor search quality, and it's the smallest change from what
exists today (the download logic is already written; it just needs a
prompt in front of it, defaulting to "no" until answered).

### Where vectors live — already answered, and it's a reasonable answer

`SqliteVectorStore` (`crates/storage/src/vector/sqlite_store.rs`) stores
chunks and their raw `f32` vectors as BLOBs in the same SQLite file
`Storage` already uses, and computes cosine similarity as a Rust-side
linear scan over every row (`cosine_similarity()`, `crates/storage/src/vector/mod.rs`)
— not the `sqlite-vec` extension.

**Checked, not assumed, since the brief asked to check rather than
assert:** `sqlite-vec` is real and viable — a pure-C, dependency-free SQLite
extension (the `vec0` virtual table, KNN queries in plain SQL), with Rust
FFI bindings on crates.io. It's pre-1.0 (`0.1.10-alpha.4` as of this
check) but actively maintained.

At AgentWorth's realistic scale — thousands of sessions, five chunks each,
so tens of thousands of 384-dim vectors — a brute-force linear scan in Rust
is fast enough that an index buys little. The real cost of `sqlite-vec`
here isn't runtime speed, it's packaging: it's a native C extension that
has to be compiled and loaded per platform, which is exactly the kind of
per-platform packaging surface a project that ships one native binary via
`npx agentworth` (per `AGENTS.md`'s Technology section) wants to avoid
unless it's actually earning its keep. **Recommendation: keep the current
DIY approach. Revisit `sqlite-vec` only if profiling ever shows the linear
scan is the bottleneck** — not before, and not as a default assumption that
"a real vector database" is obviously better.

### Staleness — the one real functional gap

Sessions are appended to constantly, and `agwt search`'s indexing trigger
(`apps/cli/src/commands/search.rs:34-107`) is: if `vector_store.stats()?.total_chunks == 0`,
index every session found by `list_sessions_filtered` once, then never
again. There is no hook into `Scanner::run_scan`, no incremental
re-chunking, nothing keyed to the fingerprint checks `AGENTS.md`'s
incremental-scanning section already establishes for sources
(`path`/`size`/`mtime`/`content fingerprint`/`adapter version`).

Concretely, once that first index has run:

- A session appended to after being embedded (a JSONL that keeps growing,
  or a rescan that picks up new events) keeps its **original, now-stale**
  chunks forever. `agwt search` will return results from an old cut of
  that session and never know it's outdated.
- A session that didn't exist at first-index time, indexed by a later scan,
  is **never embedded at all** — the auto-index guard only fires when the
  vector store is completely empty, and it won't be empty after the first
  run.

Cost of staleness here isn't cosmetic — it means `agwt search` silently
stops covering the live index the moment the first index finishes, with no
signal to the user that this happened. **Required new work:** hook
embedding into the same fingerprinted rescan path the scanner already has,
not a separate mechanism. Concretely: at the end of a successful
(re)scan of a session whose fingerprint changed, call
`VectorStore::delete_session(id)` (already exists, unused for this) then
re-chunk and re-embed just that session — both `TrajectoryChunker::extract`
and `LocalEmbedder::embed_batch` already take a single trace, so this is
wiring, not new extraction logic.

## The local model — small, on-device, translating questions into tool calls

The piece from the brief that's genuinely new: a 2-5B model running locally
(MLX on Apple Silicon) whose only job is turning a natural-language
question into an MCP tool call against `mcp-server.md`'s tool surface. The
tool does the reasoning — querying, filtering, redacting. The model's job
is narrower and more mechanical: pick which of ~6 read-only tools a
question maps to, and fill in that tool's actual parameters.

**What it needs to be good at:**

1. **Tool selection** over a small, closed set (`sessions_find`,
   `session_get`, `blame_find`, `usage_summary`, `pacing_window`,
   `coverage_stats`) — intent classification, not open-ended reasoning.
2. **Slot-filling into real parameter shapes**, not free text: "yesterday"
   → a `start_date`/`end_date` pair; "spacepilot" → something matching
   `extract_repository_or_workspace`'s output convention; "claude code" →
   the adapter id `claude_code`. This is normalization against a small,
   knowable vocabulary (the actual adapter ids `agentworth` already
   detects, the actual `ChunkKind`/`OutcomeKind` enum values), not
   creativity.
3. **Emitting well-formed tool-call JSON** the MCP client executes as-is.
4. **Knowing when to under-specify rather than guess** — a narrow,
   confidently-wrong filter that returns zero rows is worse than a broader
   query the model then narrows with a follow-up.

**Why a small model suffices:** this is intent classification plus slot
extraction over a schema with roughly six entries and enum-bounded
parameters — a well-understood, low-entropy task. It is not the same job as
writing or reviewing code, which is what the calling coding agent is
already doing. Verified: `mlx-lm` (the official LLM layer on top of Apple's
MLX) ships an OpenAI-compatible local server (`mlx_lm.server`, since
v0.18) and has working, if occasionally rough, tool-calling support across
its supported models; its own default chat model is a quantized 3B
(`mlx-community/Llama-3.2-3B-Instruct-4bit`), and the `mlx-community`
Hugging Face org hosts thousands of quantized models to choose from. A
quantized 3B fits comfortably in unified memory alongside whatever the
coding agent itself is running, which a much larger local model would not.

**What it should refuse to attempt:**

- **Upgrading evidence on its own.** `AGENTS.md`'s outcome hierarchy ranks
  "agent says done" as the weakest evidence tier for a reason. This model
  summarizing a `session_get` result should never claim a session
  "succeeded" beyond what `outcomes`/`score` actually say — it translates
  and relays, it doesn't grade.
- **Answering from its own training data instead of a tool call.** If a
  question maps to a real tool, it should call the tool, not answer from
  memory about what a typical Claude Code session looks like.
- **"Why" questions.** "What was I doing yesterday" is a `sessions_find`
  call. "Why did I decide to use snake_case" is not answerable by any tool
  here — it should say so, not synthesize a plausible-sounding answer from
  a session summary.
- **Silently picking `include_raw: true`.** Any tool call that would return
  unredacted content needs that to be an explicit, visible choice the human
  sees was made — not something the small model decides on its own because
  the question sounded like it needed detail.

**MLX versus alternatives:** MLX is the right verified starting point for
Apple Silicon specifically — native to the hardware, the `mlx-community`
model catalog is large and growing, and the tool-calling story, while not
flawless in every model/version combination, is real and improving.
CoreML was not investigated in enough depth to compare fairly here; it
would trade MLX's flexibility and fast-moving model catalog for tighter OS
integration, and that trade needs a real side-by-side before anyone
commits to it, not a single-paragraph verdict from this doc. Flagged
under Open questions.

**The same download problem shows up here too.** `mlx-lm`'s default model
"downloads automatically" the same way `fastembed`'s does — this is not a
separate policy question from the embedding-model one above, it's the same
question asked twice. Whatever consent gate gets built for the embedding
model download should cover this model download too, not get designed
twice.

**Could not verify:** a specific current-generation 2-5B model to
standardize on for tool-calling quality specifically. Search results
surfaced a mixture-of-experts example (3B active parameters out of a much
larger total) without a name or license precise enough to cite here. Don't
treat any specific model name in this space as settled — it needs a short,
real bake-off against `mcp-server.md`'s actual tool schema before picking
one, not a name pulled from a blog post.

## Decisions made here

- `mcp-server.md` ships first. This document does not gate anything on it.
- The existing chunking design (five `ChunkKind`s) is correct and doesn't
  need redesigning if embeddings ship.
- Keep the DIY SQLite brute-force vector store; don't adopt `sqlite-vec`
  unless scale forces it.
- The silent first-run network download inside `LocalEmbedder::new()` is a
  real gap against this project's stated invariants, not a hypothetical —
  it needs an explicit consent gate before this ships to anyone besides the
  person developing it locally today.
- Whatever consent mechanism gets built for the embedding model download
  should also cover the local-model-for-MLX download — one gate, not two.

## Open questions

- Does `fastembed` stay a default Cargo feature, given it triggers a
  network call with no prompt? This needs a human decision, not an
  engineering default.
- If bundling the ONNX model is preferred over runtime download, is the
  ~133MB (fp32) or ~32MB (int8) per-platform size increase acceptable
  against the existing `npx agentworth` / single-native-binary distribution
  story?
- Which 2-5B model to standardize on for the MLX tool-calling assistant —
  not settled here, needs a real bake-off.
- Does the MLX assistant ship inside `agentworth` itself, or as a separate
  opt-in companion process? Nothing read for this doc settled that.
- Eager re-embedding at the end of every rescan (simple, could slow scans
  down) versus lazy re-embedding of only changed sessions on the next
  explicit `agwt search` call (keeps scans fast, more moving parts) — worth
  deciding once the consent-gate question above is settled, since if the
  network/model gate blocks by default, eager embedding during every scan
  makes even less sense than doing it lazily on an explicit search call.
- CoreML as a real alternative to MLX — not compared here in enough depth
  to have an opinion worth trusting.
