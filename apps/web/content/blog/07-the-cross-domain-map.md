---
title: The Cross-Domain Map
slug: the-cross-domain-map
date: 2026-09-07
description: A thesis about three tools that each write down what happened but share no field to join on, and the return path we built for Archie so it can finally ask "this task, on that machine, produced this output."
tags: [thesis, loop, spacepilot, motionvector]
author: AgentWorth
---

Three tools. Three records. No shared field. That is the whole first plate
of the Cross-Domain Map, a thesis written about these three tools, and it names the gap we spent
the last stretch of work closing.

AgentWorth answers "which model, and how hard?" SpacePilot answers "where
does it run, and what's the real cost?" MotionVector answers "what's allowed
to change the document?" Each already writes its own record. The thesis's
diagnosis: "The missing piece is not the record. It is that the three
records share no field. Each was designed on its own and each is good. But
you cannot ask 'this task, on that machine, produced this output' — because
no two of them name the same thing." (Plate I, page 1)

<figure>
  <img src="/blog-figures/cross-domain-map.webp" alt="Plate I of the thesis: three tool cards for AgentWorth, SpacePilot, and MotionVector, each with the question it answers, what it actually runs, and the record it writes, joined by a boxed line reading that the three records share no field." width="1448" height="1027" loading="lazy" />
  <figcaption><span class="fig-tag">Plate I</span>Three tools, three good records, no field that joins them.</figcaption>
</figure>

## Efference copy

Plate III is the one that gave us the mechanism, not just the diagnosis:
"Send the command *and* a copy of what you predict it will feel like.
Subtract one from the other. That difference does two jobs at once." (Plate
III, page 2) In the body, that copy is the efference copy: the motor
command goes to the muscles and, at the same time, to a forward model that
predicts the sensation it will cause. In a system, the analogue is a typed
operation sent for execution and a `support_set` of what it should leave true.

The thesis names the failure when that copy is missing as the same failure
in both: "delusion of control: the person can no longer tell their own
action from an external one, so their own movements feel authored by
someone else. That is exactly what a multi-agent system suffers without
`support_set` — an agent cannot tell whether the state it relied on changed
because of its own commit or another's." (Plate III, page 2) An agent that
can't tell its own edit from another's can't trust anything it reads back —
it re-verifies what it already knows, or trusts what already moved.

## What we built

The spec calls this Loop. It reuses the harness's own hooks as the efference
copy and reafference the thesis describes, rather than inventing a new
channel:

| Hook | What it carries |
| :--- | :--- |
| `PreToolUse` | the copy of the command: tool, input, the paths Archie predicts it will touch |
| `PostToolUse` | the reafference: what came back |
| `Stop` | the compare — `git status` and HEAD against every path this session predicted since its last `Stop` |
| `Read` (`support_set`) | the paths this session actually read, hashed |
| drift (`session_drift`) | which of those hashes changed, and which session's `Stop` moved them |

A changed path nobody in the session predicted is what the spec calls
exafference: the world moved on its own. Archie names the session that
predicted it, if one did, or says plainly that no agent on the machine
claimed it — the spec's own term, not the thesis's plate. Full detail is in
`docs/specs/loop.md`.

## The shared field

Archie doesn't ask any of the three tools to change first. `run_id`,
SpacePilot's own join key, is minted by the thing that runs and lands in the
tool result an agent already sees — Archie just indexes it. Where a record
has no id, like MotionVector's `Receipt v1`, Archie anchors on its `sha256`
instead, and the machine fingerprint is the same `host_fingerprint`
algorithm SpacePilot already uses. No product had to change first, because
the transcript was already carrying the field — nobody had indexed it yet.

We've been saying it to ourselves more plainly than that: this is Archie
getting a soul and a heartbeat.

## What stays true

Loop changes what Archie sees, not what it does with it. It still never
uploads anything, still works fully offline through the spool, still never
calls a model, and the hook can never block or fail the agent it's watching
— `async: true`, exit 0, always. Turn it on with `archie hook print claude`,
paste the output into your hooks config, and restart. Archie starts seeing
commands as they run instead of reading them off the tape afterward.

```
npx -y agentworth@latest scan
```
