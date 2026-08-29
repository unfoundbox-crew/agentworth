Yes. GitHub Pages is perfect for V0.

### Landing page structure

```text
┌───────────────────────────────────────────────┐
│ AGENTWORTH                                   │
│ Your agents left receipts.                   │
│                                               │
│ $ npx agentworth                             │
│                                               │
│ [GitHub]   [Docs]                            │
└───────────────────────────────────────────────┘

          ↓ live terminal demo

Scanning ~/.claude...
Scanning ~/.codex...
Scanning ~/.gemini...

Found:
8.4B tokens
4,281 sessions
17 agents
1,337 verified outcomes

Useful work:
unfortunately, some.

────────────────────────────────────────────────

WHAT IS AGENTWORTH?

Your coding agents have quietly accumulated
gigabytes of JSONL.

AgentWorth turns that mess into:

✓ sessions
✓ token usage
✓ failures + recoveries
✓ model switches
✓ tests/builds/diffs
✓ verified outcomes
✓ weird archaeological discoveries

────────────────────────────────────────────────

YOUR AGENT ARCHAEOLOGY

MOST EXPENSIVE UNSOLVED TASK
"center this div"

Tokens       18.3M
Models       7
Time         6h 42m
Outcome      unresolved

────────────────────────────────────────────────

LOCAL MEANS LOCAL

your machine
    ↓
AgentWorth
    ↓
SQLite
    ↓
you

Nothing uploads.

[View source →]

────────────────────────────────────────────────

SUPPORTED

Claude Code · Codex · Gemini CLI · OpenCode
More via open adapters.

────────────────────────────────────────────────

OPEN SOURCE

Rust core.
Local-first.
Adapter-driven.
No account required.

$ npx agentworth

[Star on GitHub]
```

### Visual personality

Think **XKCD × terminal × old Unix utility × modern developer tool**.

* white/off-white background
* black typography
* monospace heavily
* almost no gradients
* thin borders
* terminal animations
* tiny hand-drawn/XKCD-ish annotations
* red/green only for failures/success
* absurd real stats as the visual content

### Hero copy candidates

Best default:

> **Your agents left receipts.**
> Find out what Claude, Codex, Gemini and friends actually did.

Alternatives:

> **Carbon dating your AI exhaust.**

> **8 billion tokens later, did anything work?**

> **See what the machines have been doing in `~/.config`.**

### Important

Don’t explain TokenBid/data-selling yet.

AgentWorth's landing page should sell exactly one thing:

> **Run this weird open-source scanner and discover something about your own agent history.**

That is enough for V0.
