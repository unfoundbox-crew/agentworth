# Decisions

What we settled and why. Newest first. A decision lives here, not in a chat log.

Each entry: what we decided, who decided, and the reason — because the reason
is what tells a later reader whether the decision still applies.

---

## 2026-09-01

### AgentWorth is local-only. There is no hosted dashboard, ever.

**Saurabh.** It reads coding-agent session logs off the user's own disk — their
prompts, their code, their secrets. A hosted dashboard means shipping that to a
server, which is the one thing this tool must not do.

This is a privacy line, not a scaling decision, so "just for teams" and "opt-in
sync" do not get around it. No accounts, no auth, no telemetry, no cloud sync,
no share-this-trace that uploads anything. The landing page's "100% offline /
zero telemetry" is a promise, not a feature bullet.

### Do not build toward worldtrainer inside AgentWorth.

**Saurabh.** Selling traces and trajectories to RL and AI labs is a separate
business at worldtrainer.xyz, and it only happens *if agentworth.dev takes off*.
Until then it is premature.

So: no upload plumbing, no marketplace hooks, no schema fields added in
anticipation, no "we'll need this later" abstractions. That includes the outcome
evidence schema — when it gets specced, spec it for local forensics, not resale.
The two pull in different directions.

### The dashboard is a viewer, plus one rescan button.

**Saurabh.** The CLI is where you act; the dashboard is where you look. Every
endpoint stays GET. The single exception is a rescan button, because re-reading
the disk is the round-trip you would otherwise hit constantly with the dashboard
already open.

Not built: scan-with-progress, redact-from-UI, export as a POST job. Those need
job state, progress, error and undo handling, and a web UI that mutates local
files. If we ever want them, that is a real design task, not a button.

### Marketing and dashboard are two builds, not one.

One Vite bundle was doing both jobs: `deploy-pages.yml` published it to the
public site, and `main.rs` served the same `apps/web/dist` from
`agentworth serve`. So the public site carried the whole dashboard and its API
client (404ing on every `/api/*` call), and the local binary opened a marketing
page on 127.0.0.1.

Now: `apps/web` is marketing only and deploys to Pages. `apps/dashboard` is the
local app and is what the CLI serves. `packages/ui` holds what they genuinely
share — the `--mv-*` tokens, ThemeToggle, useTheme, icons. No npm workspaces; a
Vite alias is enough.

Follows from local-only: the dashboard only ever ships inside the local binary,
so it has no reason to be on the public web at all.

### Real paths, not hash routing.

The link is part of the product — a shareable trace pasted into Slack should not
read `#/s/abc`. This cost nothing: the Axum server already serves `index.html` as
a history fallback for unmatched non-`/api` paths (`static_files.rs`).

Session ids are safe to put in URLs. Checked all 20 adapters: 18 derive the id
from the file stem, `cline` uses the parent folder, `gemini` walks path
components. All deterministic across a re-index, so deep links hold — but the
*shape* is not uniform, so encode them rather than assuming a UUID charset.

### No polling fallback for live tail.

There is no streaming endpoint yet, so the live-tail toggle renders an honest
"awaiting stream" placeholder and nothing else.

A poll would flatten a sequence of events into a state diff — it can say a rung
landed but not *when*, and for a product about evidence arriving, the timing is
the signal. It would also be absurd: the Rust process already knows the instant
a session file changes; the browser asking forty times a minute throws that away.

SSE is queued on the backend, sharing one `notify` → broadcast layer with
`agentworth watch` rather than two file watchers.

### The rail shows everything the CLI shows.

**Saurabh:** the dashboard should let you see everything you can see on the CLI.
So the rail items are real views, not stubs — Sessions, Overview
(VerdictBoard + CacheCliffWidget), Coverage (CoverageMatrix), Archaeology
(ArchaeologyPanel), Exports. Each already existed as a component; they were just
stacked as sections of one scrolling page.

Settings was dropped. There is nothing to configure, and an empty settings page
is worse than no settings page.

### The outcome ladder is one structure, not five badges.

Node size decreases and fill increases going *up* the rungs, so the loudest claim
with the least evidence (rung 1, done_claimed) is the largest and hollowest — the
hardest to ignore. The connecting spine runs solid through rungs a machine can
check and turns dashed exactly at the 2→1 boundary, where the only remaining
evidence is the agent's own word.

Captions show only what the schema carries. `OutcomeEvidence` is currently
`{kind, summary, confidence}` — no commit SHAs, test counts or CI run ids exist —
so missing values render as em-dashes rather than invented text.

**Open, not a defect:** nobody has specced structured evidence yet. This repo is
days old. When someone does, the structured facts are what make a claim
re-checkable later.

### WebMCP: `navigator.modelContext` only.

The meme-network bridge logged that its tools were active on
`navigator.modelContext` **and** `document.modelContext`. Only the first was
true — across 422 lines the second string appeared once, inside that log line.

Removed the claim rather than implementing it. WebMCP is `navigator.modelContext`,
a per-Navigator singleton (W3C Community Group Draft, 23 April 2026; Chrome 149
origin trial, Edge 147). `document.modelContext` is not in the spec, and building
toward a made-up surface is worse than a false log.
