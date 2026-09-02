# CLI grammar, completions, and the cockpit

Status: sections 1, 2 and 4(1) built 2026-09-02 in #118. Section 3 (the cockpit) and
section 4(3) (`window receipt`) are not built.

Thirty-two top-level commands is a list, not a grammar. Nobody can guess
`blind-spots` or `cache-doctor` from having used `handoff`. The fix is
noun-then-verb: four nouns carry everything that acts on indexed data, and
the handful of commands that act on the machine stay top-level.

Three pieces, in order: the grammar, shell completions over it, and a TUI
that is the same grammar with a cursor.

## 1. The grammar

Every old spelling stays as a hidden alias for two releases. Hidden means
it runs and is left out of `--help`.

### Nouns

| Today | New | Note |
| :--- | :--- | :--- |
| `traces` | `session list` | |
| `inspect <id>` | `session show <id>` | |
| `export <id>` | `session export <id>` | |
| `receipt <id>` | `session receipt <id>` | |
| `handoff [id]` | `session handoff [id]` | |
| `forgotten [id]` | `session forgotten [id]` | |
| `loose-ends [id]` | `session loose-ends [id]` | |
| `asks` | `session asks` | lands with #97 |
| `cache-doctor <id>` | `session cache <id>` | |
| `bisect <id>` | `session bisect <id>` | |
| `search <query>` | `session search <query>` | |
| `recall <query>` | `session recall <query>` | |
| `audit` | `session audit` | machine-wide, over every session |
| `blunder` | `session blunder` | |
| `autopsy` | `session autopsy` | |
| `watch` | `session watch` | |
| `blind-spots` | `session list --unproven` | a filter, not a command |
| `threat-digest` | `session risk` | |
| `matrix` | `agent list` | |
| — | `agent show <adapter>` | new: one adapter's coverage detail |
| `blame <file>` | `repo blame <file>` | |
| `pr-blame` | `repo pr-blame` | |
| `suspect` | `repo suspect` | |
| `blunder-blame` | `repo blunder-blame` | home is arguable, see §5 |
| — | `repo list` | new: indexed repos, cheap, the cockpit needs it |
| `usage` | `stats usage` | period rollups |
| `usage --pacing` | `window show` | the rolling window is its own noun |
| — | `window list` | new: recent windows |
| — | `stats outcomes` | new: `outcome_rate` gets a CLI surface |

### Top-level

`scan`, `stats`, `serve`, `mcp`, `doctor`, `docs`, `config`, `version`,
`update`, `completions`, `merge` — unchanged except the last three.

`docs` and `completions` are new. `docs` prints the generated reference.
`merge` stays top-level because it acts on the index itself, not on any
noun; whether an `index` noun should exist is open.

### Every show-style verb behaves the same

`session show|export|receipt|handoff|forgotten|loose-ends|asks|cache|bisect`
all take one session and all resolve it identically:

- **Prefix match.** Any unique prefix of a session ID. Ambiguous prefix
  exits 2 with the candidates.
- **`--last`** — newest session for this directory's repository.
- **`--current`** — same thing, said on purpose. `--last` is the alias.
- **Omitted, on a TTY** — the picker opens.
- **Omitted, not a TTY** — exit 2, print the list to stdout. A script gets
  data and a non-zero code, never a hung prompt.

`repo` verbs resolve a repository the same way, defaulting to the current
directory.

### One vocabulary with MCP

The MCP tools are already noun-verb. Where the two sides disagree, MCP
moves, because the CLI name is what a person types and remembers.

| MCP today | MCP after | Matches |
| :--- | :--- | :--- |
| `sessions_find` | `session_list` | `session list` |
| `session_get` | `session_show` | `session show` |
| `blame_find` | `repo_blame` | `repo blame` |
| `usage_summary` | `stats_usage` | `stats usage` |
| `pacing_window` | `window_show` | `window show` |
| `coverage_stats` | `agent_list` | `agent list` |
| `outcome_rate` | `stats_outcomes` | `stats outcomes` |
| `carry_forward` | `session_carry_forward` | — |
| `forgotten_context` | `session_forgotten` | `session forgotten` |
| `suspect_commits` | `repo_suspect` | `repo suspect` |

`session_handoff` and `session_asks` already match and do not move. Old
tool names stay registered and hidden for two releases, same as the CLI
aliases — a registered client should not break on a rename.

## 2. Completions

`archie completions <shell>` writes a static script covering commands, flags
and fixed value lists. Verified on docs.rs 2026-09-02: `clap_complete`
4.6.9, static generation through the `aot` module (`Generator`, `Shell`,
`generate`).

Dynamic values come from `clap_complete`'s `env` module — `CompleteEnv`,
behind the crate's `unstable-dynamic` feature, with per-argument
`ArgValueCandidates` / `ArgValueCompleter` from the `engine` module. The
binary answers completion requests itself; there is no hand-rolled
`__complete` subcommand. The documented install lines, verbatim from the
crate:

```
# bash
echo "source <(COMPLETE=bash archie)" >> ~/.bashrc
# zsh
echo "source <(COMPLETE=zsh archie)" >> ~/.zshrc
# fish
echo "COMPLETE=fish archie | source" >> ~/.config/fish/completions/archie.fish
```

The crate warns that shell code and binary must match, so re-source on
upgrade rather than committing a generated file to a dotfile repo.

| Value | Completer queries |
| :--- | :--- |
| session id | newest 50 sessions, labelled with repo and prompt preview |
| repo | distinct repos in the index |
| adapter | the adapter registry — static, no query |
| model | distinct models in the index |
| format, period, class, severity, kind | existing `value_parser` lists |

**100 ms per Tab.** One indexed `SELECT` with a `LIMIT`, on a read-only
connection, no scan and no network. A missing, locked or empty index
returns no candidates rather than blocking — a slow Tab is worse than a
Tab that offers nothing.

## 3. The cockpit

`archie` with no arguments on a TTY opens a full-screen reader over the same
data. Not a TTY, or `--plain`, or `TERM=dumb`: print the overview and exit
0.

| Screen | Shows |
| :--- | :--- |
| overview | what `archie stats` prints, plus the current window |
| sessions | `session list`, with a cursor |
| one session | `session show`, and its handoff, asks and forgotten sections |
| agents | `agent list` |
| repos | `repo list` |
| windows | `window list` |

Keys: `j`/`k` and arrows move, `Enter` drills in, `Esc` goes back, `/`
filters the current list, `1`-`6` jump to a screen, `h` handoff, `a` asks,
`f` forgotten, `r` receipt, `?` help, `q` quits.

**Out of scope, permanently:** no chat, no model calls, no editing, no
writes of any kind. The cockpit reads the index; `scan` and `config` stay
CLI commands.

**One rendering path.** Every screen composes strings from
`apps/cli/src/ui/views.rs`. The TUI adds a viewport, a cursor and key
handling — nothing else. The binding rule: no view function may exist that
only the TUI calls. If the cockpit can show it, `archie` can print it.

Dependency, verified on docs.rs 2026-09-02: `ratatui` 0.30.2, default
`crossterm` backend. Added only when the cockpit ships; the grammar PR
adds `clap_complete` alone.

## 4. Sequencing

**(1) Grammar, aliases, completions and a regenerated reference, in one
PR.** The rename touches every dispatch arm in `app.rs`, so it must land
after the open CLI branches, not under them — rebasing four branches onto
a rename costs more than waiting for them. Completions ride along because
a completion script generated before the rename is wrong the moment it
lands. Rebase on: **#97** (`feat/asks`), plus the picker, stats-from-index
and utf8-hotfix branches, and the reference-docs branch that generates
`docs/REFERENCE.md`.

**(2) The cockpit.** It needs the noun tree to exist and needs a view
function for every screen. Building it first would mean inventing both
inside the TUI, which is how the rendering rule gets broken.

**(3) `window receipt`.** Waits on spec G's P1 (`fanout_reads`,
`repeat_check`). There is nothing to put on a window receipt until the
efficiency detector produces it.

## 5. Open questions

Four of these had to be answered to build #118. What was decided, and what is still open:

| Question | Answered in #118 |
| :--- | :--- |
| Where does `blunder-blame` live? | `repo blunder-blame`, as mapped above. File-first is its trusted direction. |
| Should an `index` noun exist? | Still open. `merge` stayed top-level and `--db-path` stayed global. |
| Two releases of hidden MCP aliases, or one? | Two, the same as the CLI aliases — one removal date (`v0.1.18`) is easier to keep than two. But they are **not hidden**: MCP has no unlisted-but-callable tool, and rmcp's `disable_route` takes a tool out of `call` as well as out of `list_all`. The old names stay listed, each described as a deprecated alias, and are left out of the generated reference. |
| Should a bare `archie` open the cockpit? | Untouched — the cockpit is not built. |
| Is 100 ms per Tab real? | Now measured, but only against a fixture index of a dozen sessions (`apps/cli/tests/completion_budget.rs`). Nothing has been timed against a few thousand. |

### Still open

- Should an `index` noun exist (`index merge`, `index path`, `index
  prune`)? That would take `merge` off the top level and give `--db-path`
  a home.
- Should `archie` alone open the cockpit in its first release, or should it
  need `archie tui` until the TUI has been used for a week? A bare `archie`
  that suddenly takes over the terminal is a surprise.
- Does the Tab budget hold against an index of a few thousand sessions?
  The completers read the newest 50 rows off `idx_sessions_started_at` and
  derive repos and models from a bounded 400-row slice, so the shape is
  right; only the small case has been timed.
