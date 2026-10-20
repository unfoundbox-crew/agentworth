### AgentWorth / Evidence

**Highest evidence rung: 5, CI verified** (head a1b2c3d, 3 of 3 checks passed)

Highest attested rung: 3, tests passed locally (signed)

Receipt e69b9f1b · signed by @example-dev (GitHub signing key SHA256:bbXp...) · git note · 2 sessions · claude_code, codex

| Commit | Session | Verified here | Attested locally (signed) | Claimed only |
| :-- | :-- | :-- | :-- | :-- |
| a1b2c3d Fix scan path isolation | claude_code · 1fdeeda2 | ● 5 CI 3/3 on this SHA · ● 4 commit in PR · ● 2 files match diff (1/1) | ● 3 `cargo test` exit 0 · test authorship unknown | ○ 1 completion claim, no exit code |
| 9e8f7a6 Add receipt schema | codex · b2bed7c2 | ● 4 commit in PR · ● 2 files match diff (1/2) · → head CI 3/3 covers final tree | · | ○ 2 claimed files not in diff (1) |
| 5d4c3b2 Bump deps | · | ● 4 commit in PR · → head CI 3/3 covers final tree | · | · |
| fedcba9 Tweak \| the \*docs\* \<br\> | · | · | · | unattributed: no receipt covers this commit |

**Claims vs evidence**
- Evidence, re-derived on this runner: 3 commits named by a signed receipt are in this PR; 2 of 3 claimed files appear in their diffs; head CI 3/3 green.
- Attested, signed but not re-runnable here: 1 local test or build run exited 0.
- Claims, not evidence: 1 completion claim with no exit code; 1 claimed file not in the diff; 1 commit with no receipt.

Risk notes: 1 loop alert, 0 unresolved loops, 0 governor halts; risk unknown for 1 session.

<sub>What this check cannot see: transcripts never leave the author's machine. A receipt is the author's machine attesting to itself; only the "Verified here" column is checked by this runner. Install: `npx -y agentworth@latest repo receipt init`.</sub>
