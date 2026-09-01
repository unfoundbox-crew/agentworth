# Archie — the memory agents do not have

Status: proposed. This is the umbrella the other specs serve.

## The one-line version

Every coding agent starts blank. AgentWorth is the only thing that persists
across all of them, so it can answer what no single harness can: what happened
before, whether it actually worked, and what to do now.

## Why "AnythingLLM for coding agents" undersells it

That framing is retrieval — chat with your documents. This is three things
retrieval is not.

**It has evidence, not just text.** A RAG system over session logs can tell you
what an agent *said*. This can tell you whether the commit landed, whether the
tests ran, whether CI went green. The difference between "I finished that" and
"that is finished" is the entire product.

**It spans harnesses.** Claude Code sees Claude Code. Cursor sees Cursor. A
question like "when did I last touch this file, in anything" has no owner today.
Twenty adapters means there is exactly one place that view can exist.

**It is prescriptive, not only recall.** Retrieval answers what happened.
This can answer what to do now, because it knows the shape of sessions that
went well and the shape of ones that did not.

## The real framing

Models have no memory. What looks like memory is a transcript re-sent every
turn, and compaction shreds it — measured elsewhere in these specs at roughly a
third of one percent surviving each round, eight rounds deep in one real
session.

So every agent is amnesiac by construction, and the current workaround is a
human writing handoff files by hand, every day, so the next session knows what
the last one did.

**Archie is that job, automated.** An agent asks rather than a human writes.

That is why this is bigger than search: search helps a human find something.
This gives the *agent* continuity it structurally cannot have on its own.

## What it looks like

Cmd-K, already built. Type a question. Archie answers.

    ⌘K  ────────────────────────────────────────────────
        what did I decide about the outcome enum?
        when did I last touch static_files.rs?
        why is this session going badly?
        what should I do now?
    ──────────────────────────────────────────────────────

Four kinds of question, and they need different machinery:

| Kind | Example | Answered by |
| :--- | :--- | :--- |
| Structured | when did I last touch this file | SQL. `blame` already does it |
| Semantic | when did we talk about this | embeddings. `agwt search` already ships |
| Evidential | did that actually land | the outcome ladder |
| Prescriptive | what should I do now | patterns across sessions — see `questions.md` |

Most questions are the first kind. That is the argument in `local-search.md`
for shipping structured querying before leaning on similarity, and it still
holds — but the fourth question in that list is the one nobody else can answer,
and it is the reason to build this at all.

## What already exists

More than expected. This is assembly, not invention.

| Piece | State |
| :--- | :--- |
| ⌘K palette | built, in the dashboard shell |
| Semantic search | `agwt search` ships today; `crates/storage/src/{chunker,embedder,vector}` |
| Evidence ladder | built |
| File-to-session attribution | `agwt blame` |
| Archie as a character | `ArchieMascot.tsx` exists on the marketing site |
| MCP server | specced, not built — `mcp-server.md` |
| Local model to route questions | specced, not built — `local-search.md` |

The gap is a router: something that takes a sentence, decides which of the four
kinds it is, calls the right tool, and answers in one line. A 2-5B local model
is enough for that, because the tools do the reasoning and the model only does
the translation.

## The part worth getting right

**Answer in one line, with a receipt.** Not a paragraph, not a chat transcript.

    > did the enum fix ship?
      No. Merged to integrate/handoff-batch-1, not main. 10,188 rows
      still PascalCase.                          [session a3f2, 2 hours ago]

The receipt is the product. Anything can generate a confident sentence; this
one can point at the session it came from, which is the whole reason to trust
it.

**Say "I don't know" cleanly.** A memory layer that confabulates is worse than
no memory layer, because it is trusted by an agent that cannot check. If no
session matches, say so.

## Explicitly not this

- Not a chat interface. One question, one answer, one receipt.
- Not a code assistant. It answers about your history, not about your code.
- Not cloud. Everything stays local, and the same rules as the rest of the
  product apply.

## Sequencing

1. MCP server — the tools, callable by anything. This alone is useful: any
   agent can query without a UI at all.
2. Wire ⌘K to those tools with literal commands. No model, no natural language.
   Proves the tools answer real questions.
3. Add the router. Only once there is something to route to.

Do not start at step 3. A natural language box in front of tools that do not
exist yet is a demo, not a product.

## Open questions

- Is a 2-5B model good enough at routing, or does the fallback to explicit
  commands need to be first-class rather than a fallback?
- Should Archie answer for other agents over MCP before it answers for a human
  in the UI? The agent is arguably the more valuable user.
- What is the honest failure mode when the index is stale — refuse, or answer
  with a timestamp and let the reader judge?
