# AgentWorth — Decision & Status Inbox (`feat/agentworth-skill-and-receipts`)

Owner of this doc: this branch's verify-and-fix session, 2026-09-01. This branch
(PR #11) had no `DECISION-INBOX.md` before today — it predates today's session,
so this file is new, following the format used on `integrate/handoff-batch-1`.

Scope: PR #11 only. For the fleet-wide picture across all of today's branches,
see `integrate/handoff-batch-1`'s own `docs/DECISION-INBOX.md` (different
worktree, not touched from here).

## Where things stand

| Item | Status | Notes |
| --- | --- | --- |
| `cargo build --workspace` | Clean | 0 errors, 1 pre-existing benign warning (two `[[bin]]` targets sharing `main.rs`, not from this PR) |
| `cargo test --workspace --no-fail-fast` | Clean | 177 passed, 0 failed, 0 ignored, on lenovo |
| SKILL.md fabricated commands/flags | Fixed | See below |
| README.md missing `receipt` command | Fixed | Added a row, updated the export row |
| 4 pre-existing adapter test failures | Fixed | `pi`/`herdr`/`openclaw`/`goose` — unrelated to this PR's own diff, found while running the full suite |
| Staleness vs `integrate/handoff-batch-1` | Checked, no compile risk found | See below |
| Merge into `integrate/handoff-batch-1` | Not done from here, by design | The dispatch brief said the coordinating session does this, to avoid two sessions editing that worktree at once |

## What this PR adds (for context)

`SKILL.md` (new, 325 lines), `apps/cli/src/commands/receipt.rs` (new, 1259
lines: ANSI + SVG "Flight Receipt" rendering), wired into `agwt export
--format receipt|svg` and the new `agwt receipt <SESSION_ID>` command, plus
the Axum export route. Also `docs/SHOW_HN.md`, `docs/TRENDSHIFT.md` (GTM
copy), and a README rewrite.

## Bugs found and fixed

### 1. SKILL.md had a fully invented `agentworth gym` command

Zero implementation anywhere — confirmed with `grep -rniw "gym|chaos"
--include="*.rs" .` across the whole repo, zero hits. SKILL.md (this PR's own
new file) documented it as real: `agentworth gym --chaos-level 5 --scenario
flaky-tests`, with a full options table (`--chaos-level`, `--adapter`,
`--scenario`, `--format`). Removed the section and the matching "Agent Gym"
bullet in the activation list; put a real `agentworth receipt` section in its
place — the command this PR actually ships had no doc section at all before
this fix.

**Cross-repo note, not independently confirmed**: a same-day audit of the
`worldtrainer` repo found that `worldtrainer.xyz`'s quickstart page
references `npx agentworth gym --chaos-level 9` as a real command. That flag
name (`--chaos-level`) only ever existed in this PR's now-fixed SKILL.md — it
is not in agentworth's Rust code at all, on this branch or on main. The match
is specific enough (same invented command, same invented flag name) to be a
strong candidate for the source of that reference. This session did not
check the worldtrainer repo itself — different repo, out of scope here — so
treat this as a strong circumstantial match, not a confirmed causal link (no
check of commit dates or of which one copied which vs. both coming from the
same prompt/session).

### 2. Three more SKILL.md sections had invented flags on real commands

| Command | SKILL.md claimed | Real flags (`apps/cli/src/main.rs`) |
| --- | --- | --- |
| `scan` | `--format`, `--adapter`, `--limit` | `[PATHS]...`, `-f/--force`, `--json` only |
| `blunder` | `--model`, `--min-damage`, `--format` | `-t/--top`, `-s/--submit`, `--json` only |
| `audit` | `[SESSION_ID]` positional | no such argument — audits everything indexed, no per-session mode |
| `export` | `--exchange` | no such flag; also missing the new `receipt`/`svg` formats this PR itself adds |
| `serve` | default port `3030` | real default is `3000` (`apps/cli/src/server/mod.rs::DEFAULT_PORT`) |

Fixed all five against the real `Commands` enum. `docs/SHOW_HN.md` and
`docs/TRENDSHIFT.md` (same PR, same GTM batch) were checked too — their
command references are all real, nothing to fix there.

### 3. Real logic bug in 4 of 20 adapters' `detect()`, unrelated to this PR

Not part of this PR's diff (`crates/adapters` isn't touched by it) and
already present at this branch's base commit (`e1970a6`, current `main`) —
but it made `cargo test --workspace` fail before ever reaching this PR's own
tests (cargo stops at the first failing crate unless you pass
`--no-fail-fast`, which the first run here didn't). Fixed it directly rather
than reporting the suite red, same call this repo has made on every other
branch today.

Root cause: `pi`/`herdr`/`openclaw`/`goose`'s `detect()` only checked whether
the *passed* custom path itself was named after the adapter
(`ends_with(".pi")`, `contains("openclaw")`, etc.), with no fallback to look
one level down. The other 16 adapters (e.g. `manus`, `kimi`, `qwen`) already
handle this: check the direct hit first, then if the custom path is a
directory, check `custom.join(".manus")`-style candidates before giving up.
Each of the four adapters' own unit tests builds exactly that shape — a
tempdir root passed as `custom_paths`, with the real session file nested one
level down (`.pi/tasks/`, `.config/goose/sessions/`, etc.) — so the tests
were failing for a real reason, not flakiness: `assert!(detection.is_present)`
was deterministically false, on every run, on any machine. Confirmed by
reading `detect()`, not by rerunning and hoping.

This is also a real gap for actual users, not just a test artifact: running
`agentworth scan ~/some/repo` would silently get no detection from these 4
adapters even when `~/some/repo/.pi/` genuinely exists, while the other 16
adapters would find the same shape fine.

Fix: added the same "check the direct hit, then check one level down"
fallback these 4 adapters were missing, scoped to each adapter's own known
subdirectory names (already declared in that same file's `candidate_roots()`
— no new names invented). Deliberately did not add the further `WalkDir`
substring-scan fallback that `manus`/`kimi`/`qwen` also carry: skipped it for
`pi` specifically because `contains("pi")` on an unbounded walk is a
2-letter, very generic substring with real false-positive risk, and left the
other three at the same conservative scope for consistency across the four
fixes.

## Staleness vs `integrate/handoff-batch-1`

Merge-base between this branch and `integrate/handoff-batch-1` is `e1970a6`
— this branch's own direct parent commit. So this branch predates every fix
that landed on the integration branch today (OutcomeKind casing fix,
ModelSwitch across 20 adapters, per-model attribution, SSE endpoint, secret
detector, threat digest, blunder-blame bridge, and more) — none of that is
in this branch.

Checked whether `receipt.rs` depends on anything that changed shape, by
diffing the integration branch's tip (`482976d`) against `e1970a6` for every
symbol `receipt.rs` actually touches — read-only `git show`/`git diff`
against commit refs from this worktree, never checked out or edited the
integration worktree:

| Symbol `receipt.rs` uses | Changed on integration branch? |
| --- | --- |
| `OutcomeKind` (5 variants) | No — identical |
| `EventPayload::{AssistantMessage,ModelInvocation,ToolResult,ShellCommand,Error}` | No — identical shapes. One new variant (`ModelSwitch`) was added elsewhere, but `receipt.rs` matches these with wildcard arms, so an added variant doesn't break it |
| `TraceScore` (5 dimension fields + composite) | No — same fields. The new per-model attribution work added fields to the scoring crate's other types, not to `TraceScore` itself, and `receipt.rs` only reads fields (never destructures the whole struct), so additions elsewhere don't break it |
| `Scanner::new`, `.load_trace`, `Storage::open_default`, `.open_path`, `RecoveryDetector::new`, `.detect_recoveries`, `estimate_tokens_cost_usd` | No — all unchanged signatures. `estimate_tokens_cost_usd` now delegates internally to a new `estimate_model_tokens_cost_usd(None, ...)`, but its own signature and return value are identical |
| `primary_outcome` string casing (the actual PascalCase/snake_case bug fixed on the integration branch today) | Changed, but doesn't reach `receipt.rs` — it matches the `OutcomeKind` Rust enum directly, never the stored string |

No breaking renames found in anything this PR's code actually calls. A real
merge into `integrate/handoff-batch-1` will still hit ordinary line-level
conflicts in `apps/cli/src/main.rs`, `apps/cli/src/server/routes.rs`, and
`apps/cli/src/commands/mod.rs` — the integration branch has changed all
three files for unrelated features — but nothing found that looks like a
deeper API mismatch needing more than normal conflict resolution.

## Commits on this branch (fix-forward, not pushed)

| Commit | What |
| --- | --- |
| `db06270` | Original PR commit (pre-existing, untouched) |
| `4df56f1` | `fix(adapters): recurse into custom_paths for pi/herdr/openclaw/goose detection` |
| `e29955a` | `docs(skill): fix fabricated CLI commands/flags in SKILL.md, document receipt` |
| (this commit) | `docs: record PR #11 verification findings` |

## Recommendation

Ready to merge into `integrate/handoff-batch-1` as far as this PR's own
content goes: build clean, 177/177 tests passing on lenovo, no fabricated
docs left, no known API mismatch with the integration branch. The merge
itself will need normal conflict resolution in the three CLI files listed
above — not attempted from here, per the task boundary.
