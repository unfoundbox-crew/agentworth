# Roadmap: release status and remaining work

Status: 2026-10-08. Checked against main `e159dd1`, release tag `v0.1.28`
(`aa7e851`, published 2026-09-30), and the local worktrees. The initial audit found no open PRs. Release
preparation is now [draft PR #229](https://github.com/unfoundbox-crew/agentworth/pull/229),
targeting main. Main CI run [36834340036](https://github.com/unfoundbox-crew/agentworth/actions/runs/36834340036)
passed on `e159dd1`. This proves main, not the prepared release branch.

## Next release: v0.1.29

Prepare a patch release from the 17 main commits after `v0.1.28`. No P4 work
is required for this release. The dashboard stays at `/`; the deck stays at
`/home/`, with archive as a phase.

| Scope on main | Evidence | Release consequence |
| :--- | :--- | :--- |
| Dashboard restore control and Live Tail | #210, #213 | Restore remains visible when collapsed; updates stream over SSE |
| Deck P3 parity and archive Live Tail | #218, #219, #222 | Overview, Coverage, Archaeology, Exports, palette, and live updates in the deck |
| Shared shell components | #223, #225 | Both explorers use shared cache, export, verdict, and fleet components |
| Codex human turns | #216, #224 | Scan reads Codex history for insights heatmap, friction, and vocabulary |
| Wake repository alias | #227 | Existing sessions remain discoverable after the repository rename |
| Insights and matrix honesty | #228 | Schema 3 distinguishes session user messages from human turns; coverage separates detected from indexed |

Release preparation lives on `codex/release-v0.1.29`. Changelog, version
pins, lockfiles, spec status, and release instructions are updated there.
Local validation on 2026-10-08 found and fixed an additional scan bug:
explicit session paths also ingested unrelated machine-global human-turn
sources. Explicit-path and custom-adapter scans now leave that global lane
untouched. A real CLI fixture proves scoped/forced isolation, normal
machine ingestion, and preservation of already-ingested turn rows/version.
The grammar test now uses its synthetic home for child commands too.

## Local release validation, 2026-10-08

Saurabh explicitly authorized local builds for this release. All checks below
ran in the release worktree, with a separate Cargo target directory. Smoke
checks used synthetic histories and a separate fixture database. The
installed binary and real index were not replaced.

| Check | Result |
| :--- | :--- |
| Rust workspace tests, including doc tests | 1,193 passed; 0 failed; 0 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| Optimized native build | Passed; macOS arm64; all three names report 0.1.29 |
| Dashboard, home, marketing builds | Passed; marketing prerender checked 64 routes |
| Home tests / dashboard typecheck | 143 passed / passed |
| npm launcher / DSH plugin tests | 52 passed / 6 passed |
| Version pins, lockfiles, generated reference | Consistent; regenerated with the candidate binary |
| Release binary embedding and HTTP smoke | Dashboard/home HTML and JS match their dist builds; home deep links, stats, insights schema 3 pass |
| npm wrapper smoke | Runs the candidate binary and reports 0.1.29 |
| `doctor --self-test` on multilingual fixture index | 9 pass, 1 expected compaction skip; no slow/fail steps |

Marketing download-count lookups were unavailable in the sandbox; the build
handled them as unavailable and completed. Cross-platform packaging, signed
release assets, registry publication, and clean-install CI smoke tests remain
release-workflow checks. Local success does not stand in for those checks.

The first PR CI run used Rust 1.99 while local clippy used 1.97.1. CI found
five unnecessary closure borrows in existing wake redaction; the release
branch removes them without changing redaction behavior. All 28 wake tests
and local clippy pass after the correction. PR #229 carries the current CI
results. Saurabh approved merge of PR #229 and publication of v0.1.29 on
2026-10-08: "go ahead", in reply to the merge/tag/release-verification plan.
Release workflow verification remains pending.

## Release publication checklist

1. Run candidate CI after the approved branch push. Local validation above
   passed; CI must confirm the published commit before merge and tagging.
2. Get Saurabh's approval before publishing the branch and PR to this public
   repository. No open train PR exists; the September 19 and September 23
   train branches are already ancestors of main. Confirm the release PR base
   when publishing; do not reuse those branches.
3. Get COS / Saurabh's approval to merge the reviewed release PR into main.
4. Confirm the changelog date, then tag the approved merge commit `v0.1.29` only after CI passes. A tag push
   publishes GitHub assets and npm packages; publication needs approval.
5. Verify the release workflow and clean-install smoke tests. Marketing
   deployment is a separate Vercel operation.

## Local worktree and branch audit, before cleanup

| Worktree / branch | Tip | Finding |
| :--- | :--- | :--- |
| Shared checkout / main | `e159dd1` | User edit in `apps/web/package.json` adds a packageManager field; excluded from release preparation |
| `app-merge/archive-live-tail` | `aa7798d` | Clean; patch-equivalent change is merged as #222 |
| `feat/codex-human-turns` | `4aa6912` | Clean; main already has Codex ingest, linear turn-link repair, UUID suffix extraction, and UTF-8 regression coverage |
| `app-merge/shell-extract-export-cache` | `9560ec3` | Clean; main already has extracted components, lucide-react peer, and Vite peer resolution |

`git cherry` reports the Codex and shell follow-ups as unique commits, but
inspection of main confirms their functional outcomes are present. Commit
identity alone does not make them release gaps. No cherry-picks were needed.

Saurabh approved local cleanup on 2026-10-08 and waived the absent Git-flow
file for that cleanup. The three clean worktrees in the table were removed,
including their ignored build output. Their original local branches remain.
Another 49 local branch refs were deleted after verifying every tip is an
ancestor of main `e159dd1`. No remote refs were changed. Main, the release
worktree, and all branches with unique history remain. The shared checkout's
packageManager edit was left untouched.

## Remaining product work

| Priority | Item | Current state / next decision |
| :--- | :--- | :--- |
| P1 | P3 dogfood | Implementation merged; this audit has no confirmed dogfood acceptance. Record the receipt before requesting P4 |
| P1 | Capability / outcome measurements | Capability matrix remains a dated 2026-09-30 measurement. Refresh after relevant parser or predicate changes; do not present old counts as current |
| P2 | Done gate | `done-gate.md` proposed; `session_gate` not implemented |
| P2 | Efficiency receipts | `fanout_reads`, `repeat_check`, and `window receipt` remain proposed |
| P2 | Agent bus | Messages, claims, and send/inbox/ack remain proposed |
| P2 | Machine memory | `memory.md` proposed; build ordering depends on the bus |
| P2 | Repo-filter completeness | MCP uses 4x overfetch for repo filters; a fixed factor can still miss matches. Other calls do not all pay that factor |
| P2 | Belief checks | `claim_check` remains proposed |
| P3 | Default semantic search | fastembed is off by default. Enabling ONNX builds and the first-run model download requires a product/distribution decision |
| P3 | Desktop / voice / aggregate exports | Conditional work; retain each spec's preconditions |
| Blocked | App-merge P4/P5 | Needs P3 dogfood and explicit COS relaunch. Do not delete dashboard or flip root |

## Closed work from the September 12 roadmap

Plugin packaging (#133, #134), adapter work (#167), landing/blog work
(#135, #138), model-download notice (#169), and the matrix correction
(#215) are merged. The old token budgets were not checked against the index
in this audit and are removed. New implementation estimates must use the
machine's measured session data, as required by AGENTS.md.

The spec index status table is current for this audit. Its older sequencing
prose and the decision inbox's historical log are context, not release gates.
