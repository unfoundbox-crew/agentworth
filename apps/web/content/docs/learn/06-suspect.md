---
title: "Suspect commits"
description: "Before you push: which commits on this branch came from a session that never proved anything. A list and a prompt, never a patch."
---

You merge work you did not watch. Subagents write most of it, and by the time
you look, the diff reads fine and the session behind it is 20,000 events you
will never read.

`agentworth suspect` does not review the diff. It asks a different question:
which of these commits were written by a session that never got above rung 2?

## How a commit gets flagged

For each commit on the branch, take its changed paths, find blame rows whose
file path matches and whose timestamp falls in a window before the commit, then
look at that session's outcome rung. Below rung 3 — nothing ran and passed —
the commit is flagged.

Output is a list of commits with their session ids, and a copyable prompt.
Never a diff. A trajectory says the session was sloppy; it does not say what the
sloppy code does wrong, and a stack trace still writes the better patch.

## Path anchoring, and why it matters

The first version matched a blame path by suffix. Against 104 commits of this
repo's own `origin/main` it flagged 17. Nine of the first ten checked by hand
were false: `file_modifications.file_path` is not always absolute, so a
`.gitignore` from an unrelated checkout matched this repo's `.gitignore`.

Anchoring the match to the session's own repository fixes it:

| | naive suffix | anchored |
| :--- | ---: | ---: |
| Attributed | 59 | 44 |
| Flagged | 17 | 1 |
| Flag rate of attributed | 28.8% | 2.3% |

All ten flags anchoring removed were false. The one flag it kept was right.
Over the last 60 commits both rules flag zero — every session behind them
reached rung 3 or higher. A true and boring answer is the correct one.

## The pre-push hook

`--hook` prints a ready-to-install pre-push script. **It never blocks a push.**
It runs `--quiet`, which prints only the copyable prompt, and only when
something is actually suspect. A silent hook means a clean branch.

## Choosing the range

`--since` takes a date or a git ref, and defaults to the branch's upstream, then
`origin/main`. `--branch` defaults to HEAD. `--base` names the diff target
separately when you want it different from `--since`. `--repo` points at a
checkout other than the current directory.

Over MCP the same report is `suspect_commits`.

## What to run

```bash
agentworth suspect
agentworth suspect --since origin/main
agentworth suspect --repo ~/code/example --branch feat/x
agentworth suspect --hook
agentworth suspect --quiet
agentworth suspect --json
```
