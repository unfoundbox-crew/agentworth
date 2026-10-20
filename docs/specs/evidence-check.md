# AgentWorth / Evidence: the PR check (slice 1 of the evidence layer)

Status: built, released in v0.1.30 (the `v0` Action tag and the Marketplace listing are pending): #231 (format and signing), #233 (create, hook, MCP), #232 (verify and renderer), and #234 (the Action, dogfood workflow and docs). Decisions in §7 are taken at the COS defaults. Proposed 17 Oct 2026. Owner: COS. Builds on v0.1.29 (`main` at `54d5fa3`).
Read-only research against a shallow clone of `unfoundbox-crew/agentworth`. Every repo fact below cites a path.

## 0. The one-line version

A developer pushes a branch. A pre-push hook signs a **Work Receipt** from the local index and pushes it as a git note. A GitHub Action named **AgentWorth / Evidence** verifies the signature, re-derives every rung it can see from CI (commits present, files overlap, CI green on head), and shows each commit's rung in three columns: **verified here**, **attested locally (signed)**, **claimed only**. Neutral by default. It never blocks a merge unless the repo opts in.

This is the L2 doorway `docs/specs/extensions.md` already ranked second ("the only one that reaches people who do not have AgentWorth installed"). That spec says the Action and the receipt format "do not" exist. This spec builds both, and amends one choice it made (transport: git notes, not the PR body; see §1).

## 1. The core constraint: the index is on a laptop, CI is not

`docs/specs/extensions.md` (L2) states the rule this design obeys: the Action "does not see the local index, cannot compute a rung from transcripts it has no access to, and must never be built in a way that tempts someone to upload one." `AGENTS.md` (Core invariants) adds "Never upload user data without explicit user action", and its Privacy section requires `select → redact → preview → explicit approval → export`.

So something small, derived and signed must travel from the laptop to the PR. Options weighed:

| Option | Survives forks | Pollutes diff | Binds to SHAs | Setup cost | Verdict |
| :-- | :-- | :-- | :-- | :-- | :-- |
| A. Signed receipt in **git notes** `refs/notes/agentworth`, pushed by the pre-push hook | yes (Action fetches notes from the head repo, which is public for a fork PR) | no | yes, a note hangs off a commit SHA | one hook line | **Default** |
| B. Signed receipt committed under `.agentworth/receipts/<id>.dsse.json` | yes, rides in the PR tree | yes, one file per push | indirectly (a receipt cannot name the commit that carries it, so it covers the commits before it) | none | **Fallback** (`--transport file`) |
| C. Receipt pasted in the PR body (the `extensions.md` choice) | yes | no | yes | manual paste, every push | Rejected for v1: anyone with write can edit it, 65,536-char body limit shared with prose, and it needs a human step on every push, which kills the one-minute setup |
| D. Workflow artifact uploaded from the laptop | no | no | weak | needs a token on the laptop | Rejected: an upload path from the laptop is exactly what `extensions.md` forbids |
| E. Action phones the laptop, or a hosted relay | n/a | n/a | n/a | n/a | Never. `docs/specs/README.md` and `extensions.md` close on "no accounts, no telemetry, no sync" |

**Decision (COS default): A, with B as a flag.** The verifier reads both, so a team can switch without touching the workflow.

Mechanics of A:

1. `archie repo receipt create --push` runs inside the existing pre-push hook (`PRE_PUSH_HOOK`, `apps/cli/src/commands/suspect.rs:617`). The hook already "exits 0 no matter what it finds"; that stays true.
2. It resolves the range the same way `repo suspect` does (`resolve_range`, `suspect.rs:238`: upstream, then `origin/main`, then last N), builds one receipt covering those commits, signs it, and writes it with `git notes --ref=agentworth add -f <tip-sha>`.
3. It pushes `refs/notes/agentworth` before the hook returns, so the note lands before the commits and before the PR workflow starts. Concurrent writers: fetch the remote notes ref, `git notes --ref=agentworth merge -s cat_sort_uniq`, push, retry once. A guard env var (`AGENTWORTH_RECEIPT_PUSH=1`) stops the nested push from re-running the hook.
4. We never add notes to `remote.<name>.push`: once a push refspec is configured, a bare `git push` stops pushing the current branch. That would be a silent behavior change.
5. Rebase or squash before merge changes SHAs. The old note simply stops matching and the check says "no receipt covers this commit" until the next push re-signs. Unknown, never clean (the rule `suspect.rs` already states in its header).

## 2. Work Receipt v1

### 2.1 Envelope and statement

A Work Receipt is an **in-toto Statement v1** inside a **DSSE** envelope (both public specs: `github.com/in-toto/attestation/spec/v1`). We do not invent an envelope. That keeps it verifiable by stock in-toto tooling and lets slice 2 hand the same statement to `actions/attest`.

```jsonc
// DSSE envelope (what is stored in the note / file)
{
  "payloadType": "application/vnd.in-toto+json",
  "payload": "<base64 of the canonical Statement bytes>",
  "signatures": [{ "keyid": "SHA256:<openssh fingerprint>", "sig": "<base64 ed25519 over DSSE PAE>" }]
}
```

```jsonc
// Statement (decoded payload). EXAMPLE DATA.
{
  "_type": "https://in-toto.io/Statement/v1",
  "subject": [
    { "name": "commit", "digest": { "gitCommit": "a1b2c3d4e5f6..." } },
    { "name": "commit", "digest": { "gitCommit": "9e8f7a6b5c4d..." } }
  ],
  "predicateType": "https://agentworth.dev/work-receipt/v1",
  "predicate": {
    "receipt_version": 1,
    "generator": { "name": "archie", "version": "0.1.30" },
    "created_at": "2026-10-20T09:41:00Z",
    "range": { "base": "5d4c3b2a...", "head": "a1b2c3d4...", "commits": 3, "rule": "upstream..HEAD" },
    "attribution": { "rule": "anchored-blame/v1", "window_hours": 24, "unanchored_rows": 0 },
    "sessions": [
      {
        "ref": "3f9a1c0e8b7d6a54",
        "adapter": "claude_code",
        "models": ["claude-opus-4-5"],
        "started_at": "2026-10-20T08:02:00Z",
        "ended_at": "2026-10-20T09:30:00Z",
        "tokens_total": 1840000,
        "spend_micro_usd": null,
        "highest_rung": 3,
        "rungs": [
          { "rung": 3, "kind": "test_or_build_passed", "source": "shell_exit_0", "command_class": "cargo test",
            "confidence_bp": 8500, "test_origin": "unknown", "event_seq": 412, "verified_locally": true },
          { "rung": 1, "kind": "done_claimed", "source": "assistant_claim", "command_class": null,
            "confidence_bp": 3500, "test_origin": null, "event_seq": 455, "verified_locally": false }
        ],
        "risk": { "demoted_claims": 0, "loop_alerts": 1, "unresolved_loops": 0, "recoveries": 2, "governor_halts": 0, "known": true }
      }
    ],
    "commits": [
      { "sha": "a1b2c3d4e5f6...", "sessions": ["3f9a1c0e8b7d6a54"], "files": ["crates/core/src/turns.rs"], "anchor": "absolute" },
      { "sha": "9e8f7a6b5c4d...", "sessions": [], "files": [], "anchor": null }
    ],
    "index": { "last_session_at": "2026-10-20T09:30:00Z", "parser_version_min": 7 },
    "redaction": { "policy": "publication/v1", "fields_dropped": ["source_path", "db_path", "prompt_preview", "commands", "quotes"] }
  }
}
```

### 2.2 Field sources (REUSE first)

| Field | Comes from | Path |
| :-- | :-- | :-- |
| `subject[]`, `range` | `resolve_range` + `read_commits` | `apps/cli/src/commands/suspect.rs:238`, `:289` |
| `commits[].sessions/files/anchor` | anchored blame join | `Storage::blame_for_repo`, `crates/storage/src/lib.rs:4453`; rule table in `crates/storage/src/anchoring.rs` |
| `rungs[]` kind/rank | `OutcomeKind`, `outcome_rank` | `crates/schema/src/event.rs:20`, `crates/outcomes/src/outcome.rs:429` |
| `verified_locally` | ground-truth re-check (`git cat-file -e`, mtimes, exit codes) | `crates/outcomes/src/verify.rs:125` (`verify_outcomes`), `:316` (`verify_commit_observed`) |
| `test_origin` | `TestProvenance` / `TestOrigin` (held-out gate) | `crates/schema/src/event.rs:66`, `crates/outcomes/src/outcome.rs:447` |
| `risk.*` | `session_risk` counters | `crates/storage/src/lib.rs:1191` |
| `risk.governor_halts` | `governor_events` (thrash-halt and spend brake) | `crates/storage/src/lib.rs:1315`, `crates/loop/src/gate.rs` |
| `tokens_total`, `spend_micro_usd` | flight-receipt extraction, per-model pricing | `extract_flight_data`, `apps/cli/src/commands/receipt.rs:120`; `estimate_total_cost_from_per_model_usage` in `crates/storage` |
| "proven" threshold | `EVIDENCE_FLOOR = 3` | `crates/storage/src/lib.rs:363` |

We deliberately do **not** build on `repo pr-blame` (`apps/cli/src/commands/pr_blame.rs:63`): it joins through `find_sessions_for_blame` without the anchoring rule, which is the suffix-join failure `anchoring.rs` documents (16 of 17 false flags on `Cargo.lock`). The receipt uses the anchored join only.

### 2.3 Canonical JSON

- Keys sorted, no insignificant whitespace, UTF-8, no floats anywhere (confidence in basis points, money in micro-USD). This is RFC 8785 (JCS) compatible without needing a float serializer.
- `receipt_id = sha256(canonical Statement bytes)`, shown in the check and accepted by `archie repo receipt show <id>` locally.
- DSSE signs the exact payload bytes, so verifiers never re-canonicalize to check a signature. Canonical form matters for `receipt_id` stability and golden tests.
- Size cap 64 KiB per receipt; over the cap, drop `files[]` first and set `truncated: true`.

### 2.4 Signing: recommend a local ed25519 key, optionally linked to GitHub

| Option | Identity | Offline | Privacy | Friction | Verdict |
| :-- | :-- | :-- | :-- | :-- | :-- |
| **Archie-owned ed25519 key** at `~/.agentworth/keys/receipt_ed25519` (OpenSSH format, 0600), created on first `init` | integrity always; identity when the public key is also added as the author's **GitHub SSH signing key** | yes | nothing leaves the machine | zero, plus one optional `gh ssh-key add --type signing` line | **v1** |
| Reuse `~/.ssh/id_ed25519` | same as above, and often already registered | yes | same | passphrase and agent handling, key reuse across purposes | later flag |
| Sigstore keyless (cosign / gitsign) from the laptop | OIDC identity in a public transparency log | no | publishes the signer's email to Rekor, which collides with `AGENTS.md`'s "personal information" rule | browser OIDC flow | Rejected for the laptop |
| GitHub artifact attestations (`actions/attest`, Sigstore via the workflow's OIDC) | "this verdict was produced by this workflow" | n/a (CI side) | no personal data | needs `id-token: write`, `attestations: write` | **Slice 2**: CI countersigns its verdict |

How identity works without any service: GitHub's public endpoint `GET /users/{username}/ssh_signing_keys` lists a user's signing keys, readable by anyone. The Action fetches them for the PR author and each commit author's login (cap 10 logins), and reports one of:

- **Signed by @login**: signature valid and the key is one of that user's GitHub signing keys.
- **Signed, key not linked**: signature valid, key unknown to GitHub. Integrity holds, identity does not.
- **Signature invalid**: every rung in the receipt drops to "claimed".

Threat model, said plainly in the check footer: a receipt is the author's machine attesting to itself. A determined author can sign a fabricated receipt with their own key. That is why "attested locally" is a separate column from "verified here", and why only CI-visible facts ever reach the verified column.

Crypto lives in a new small crate (`crates/attest`) so the dependency (`ed25519-dalek`, not in `Cargo.lock` today) stays out of the scanner. OpenSSH public keys are parsed by hand (`ssh-ed25519` + 32 bytes), no `ssh-keygen` subprocess.

### 2.5 Privacy and redaction rules

Default receipt contents obey `AGENTS.md` Privacy ("must avoid leaking ... repository names, absolute paths, organization names, personal information").

**Never in a receipt:** prompts, assistant text, thinking, tool output, code, diffs, raw command lines, `source_path`, `db_path` (today's `SuspectReceipt` carries it, `suspect.rs:93`, so it is dropped in the projection), `prompt_preview`, `best_apology_quote` and every other free-text field of `FlightReceiptData` (`receipt.rs:50`), hostnames, usernames, emails, branch names, raw session IDs.

**Allowed:** commit SHAs and repo-relative paths **only where they already appear in that commit's own diff** (intersection computed locally), adapter and model names, rung kinds, integer counts, timestamps rounded to the minute, and `command_class` from a fixed allowlist (`cargo test`, `cargo build`, `npm test`, `pnpm test`, `pytest`, `go test`, `make test`, `gh pr create`, ...). Anything else becomes `other_test_command`.

**Session refs:** `ref = hex(sha256("agentworth-session-ref/v1" ‖ signer_pubkey ‖ session_id))[:16]`. Stable per signer, not reversible from a UUID's entropy, and `archie session show --ref <ref>` maps it back on the author's machine.

**Spend:** `spend_micro_usd` is `null` unless `--include-spend` (decision row 4). Tokens are included.

**Fail closed:** after building the predicate, run `Redactor::for_publication()` (`crates/redaction/src/redactor.rs:45`) over the serialized JSON. If it changes a single byte, a free-text leak got into a structured field: abort, sign nothing, print the field. Every display string in the check is generated by the verifier from enums, never copied from the receipt.

**Consent:** `select → redact → preview → explicit approval → export` maps to: the range is the selection; the projection plus redactor is redaction; `archie repo receipt create` on a TTY prints the exact JSON and a "what this reveals" list (preview); the first export per repository asks y/N and records consent in `~/.agentworth/config.toml` (approval); the hook path exports only when consent is recorded and otherwise prints one line and exits 0.

## 3. What the check shows

### 3.1 Rungs re-derived in CI

| Rung | In CI the Action can see | Column it lands in |
| :-- | :-- | :-- |
| 5 CI or deployment verified | check runs and commit statuses on the head SHA (excluding itself), via `gh api repos/{o}/{r}/commits/{sha}/check-runs` and `/status` | **Verified here**, for the head commit; earlier commits show "head CI covers final tree" |
| 4 Commit observed | the receipt's SHA is in `base..head` of this PR | **Verified here**, only for commits a receipt covers |
| 2 Artifact changed | receipt `files[]` ⊆ `git diff-tree --name-only <sha>` | **Verified here** on overlap; mismatch demotes to claimed |
| 3 Test or build passed | not re-runnable from a receipt | **Attested locally** if signed and `verified_locally`; shows `test_origin` (agent-chosen tests are flagged, matching the held-out rule at `outcome.rs:447`) |
| 1 Done claimed | never evidence | **Claimed only** |

Anything with `verified_locally: false`, an invalid signature, or a SHA outside the PR is "claimed only". Commits with no receipt are "unattributed: no receipt covers this commit" (unknown, never clean).

**Highest evidence rung** (the headline) = the highest rung in the *Verified here* column across receipt-covered commits. **Highest attested rung** is shown beside it, smaller.

### 3.2 Conclusion policy

| Situation | Conclusion |
| :-- | :-- |
| Every PR commit covered by a valid receipt signed by a linked key, and head CI green | `success` |
| Anything else: no receipts, partial coverage, unlinked key, CI pending or red, invalid signature | `neutral` (invalid signature gets a warning title) |
| Opt-in only: `fail-on: invalid-signature`, `fail-on: below-rung:3`, `fail-on: unattributed` | `failure` |

The job step itself always exits 0. CI pending: the Action polls check runs on the head SHA up to `wait-for-ci` (default 10 min), then reports "CI pending" and updates on the next `synchronize`. A `workflow_run` mode (runs after named workflows complete, in the base repo's context) is documented for teams that want the rung-5 result without polling.

### 3.3 Mock of the check output (EXAMPLE DATA, not a real PR)

**Title:** `Rung 5 · CI verified · 2 of 3 commits carry a signed receipt`
**Conclusion:** neutral

```markdown
### AgentWorth / Evidence
> EXAMPLE DATA

**Highest evidence rung: 5, CI verified** (head a1b2c3d, 3 of 3 checks passed)
Highest attested rung: 3, tests passed locally (signed)
Receipt 7c41e0a9 · signed by @saurabh (GitHub signing key SHA256:Xq3v…) · 2 sessions · claude_code, codex

| Commit | Session | Verified here | Attested locally (signed) | Claimed only |
| :-- | :-- | :-- | :-- | :-- |
| a1b2c3d Fix scan path isolation | claude_code · 3f9a1c0e | ● 5 CI 3/3 on this SHA · ● 2 files match diff (4/4) | ● 3 `cargo test` exit 0 · tests chosen by agent | · |
| 9e8f7a6 Add receipt schema | codex · 77c1b2aa | ● 4 commit in PR · ● 2 files match diff (2/3) | ○ none | ◌ 1 "all tests pass", no exit code captured |
| 5d4c3b2 Bump deps | · | · | · | unattributed: no receipt covers this commit |

**Claims vs evidence**
- Evidence, re-derived on this runner: 2 commits named by the receipt are in this PR; 6 of 7 claimed files appear in their diffs; head CI 3/3 green.
- Attested, signed but not re-runnable here: 1 local test run exited 0 (agent-chosen tests).
- Claims, not evidence: 1 completion claim with no exit code; 1 claimed file not in the diff.

Risk notes: 1 loop alert, 0 unresolved loops, 0 governor halts.

<sub>What this check cannot see: transcripts never leave the author's machine. A receipt is the author's machine attesting to itself; only the "Verified here" column is checked by this runner. Install: `npx -y agentworth@latest repo receipt init`.</sub>
```

On a PR with no receipts at all, the check still runs: "0 of 4 commits carry a receipt · head CI 3/3 green" plus the one-line install. That turns every reviewer into a potential user.

Display glyphs stay inside the set `ci.yml` enforces for `install.sh` (● ○ · → plus box drawing).

## 4. Components (REUSE / EXTEND / NEW)

| # | Component | Tag | Path | Note |
| :-- | :-- | :-- | :-- | :-- |
| 1 | Evidence ladder enum and rank | REUSE | `crates/schema/src/event.rs:20`, `crates/outcomes/src/outcome.rs:429` | rung numbers 1 to 5 unchanged |
| 2 | Outcome detector and ground-truth verify | REUSE | `crates/outcomes/src/outcome.rs:57`, `crates/outcomes/src/verify.rs:125` | sets `verified_locally` |
| 3 | Anchored blame join | REUSE | `crates/storage/src/lib.rs:4453`, `crates/storage/src/anchoring.rs` | only join the receipt trusts |
| 4 | Range resolution and ref validation | REUSE | `apps/cli/src/commands/suspect.rs:166`, `:238`, `:289` | same range the hook already uses |
| 5 | Session risk counters | REUSE | `crates/storage/src/lib.rs:1191` | |
| 6 | Governor and thrash-halt events | REUSE | `crates/storage/src/lib.rs:1315`, `crates/loop/src/gate.rs` | counts only |
| 7 | Held-out test provenance | REUSE | `crates/schema/src/event.rs:66`, `crates/outcomes/src/outcome.rs:447` | flags agent-chosen tests |
| 8 | Publication redactor | REUSE | `crates/redaction/src/redactor.rs:45` | fail-closed check |
| 9 | Per-model pricing for optional spend | REUSE | `estimate_total_cost_from_per_model_usage`, `crates/storage` | `$/verified` math unchanged |
| 10 | Release tarballs + `.sha256` | REUSE | `.github/workflows/release.yml:127` | Action installer verifies the checksum |
| 11 | npm launcher (`agentworth`, `archie`, `agwt` bins) | REUSE | `packages/agentworth/package.json` | `npx` path in README |
| 12 | Commit attribution core | EXTEND | `apps/cli/src/commands/suspect.rs:422` | factor `attribute_commits()` out of `compute_suspect_commits` so it returns all attributed commits; `repo suspect` keeps filtering to suspects |
| 13 | Flight receipt extraction | EXTEND | `apps/cli/src/commands/receipt.rs:120` | add a privacy-safe `WorkSession` projection; SVG card untouched |
| 14 | Pre-push hook | EXTEND | `apps/cli/src/commands/suspect.rs:617` | one added line `archie repo receipt create --push --quiet \|\| true`; still exits 0 |
| 15 | `repo` noun | EXTEND | `apps/cli/src/app.rs:553` (`RepoCommand`) | add `Receipt { init, create, verify, show, key }` |
| 16 | MCP server | EXTEND | `apps/cli/src/mcp/server.rs` (next to `repo_suspect`, `:1067`) | add `repo_receipt` (preview only, never signs or pushes) |
| 17 | Generated reference docs | EXTEND | `docs/REFERENCE.md`, `docs/reference.json` (gate in `.github/workflows/ci.yml`) | regenerate |
| 18 | CI | EXTEND | `.github/workflows/ci.yml` | job that runs the Action against a fixture repo |
| 19 | Release | EXTEND | `.github/workflows/release.yml` | move the `v0` action tag on each release |
| 20 | L2 extension spec | EXTEND | `docs/specs/extensions.md` | amend transport (notes, not PR body); mark L2 built |
| 21 | README install path | EXTEND | `README.md` (Quickstart table) | §5 below |
| 22 | Work Receipt types + canonical JSON | NEW | `crates/schema/src/work_receipt.rs` | |
| 23 | Signing crate (ed25519, DSSE PAE, OpenSSH key parse) | NEW | `crates/attest/` | new workspace member |
| 24 | `archie repo receipt create/show/key/init` | NEW | `apps/cli/src/commands/work_receipt.rs` | notes/file transport, consent, preview |
| 25 | `archie repo receipt verify` (offline) + markdown renderer | NEW | `apps/cli/src/commands/receipt_verify.rs` | takes `--ci ci.json --keys keys.json`; no network in the binary |
| 26 | Composite Action | NEW | `action.yml` at repo root + `action/*.sh` | install, fetch notes, collect CI and keys with `gh api`, run verify, post check |
| 27 | Dogfood workflow | NEW | `.github/workflows/agentworth-evidence.yml` | every AgentWorth PR shows the check |
| 28 | JSON Schema | NEW | `docs/schemas/work-receipt-v1.json`, served at `agentworth.dev/schemas/work-receipt/v1.json` via `apps/web/public` | |
| 29 | Spec in repo | NEW | `docs/specs/evidence-check.md` + row in `docs/specs/README.md` | |
| 30 | Learn page + demo | NEW | `apps/web/content/docs/learn/10-evidence-check.md`, `docs/dogfood/evidence-demo.tape` (vhs) | |

**Counts: REUSE 11 · EXTEND 10 · NEW 9.**

### 4.1 CLI

```
archie repo receipt init   [--write]            # prints workflow YAML + hook; --write writes them and records consent
archie repo receipt create [--base REF] [--push] [--transport notes|file] [--include-spend] [--json] [--quiet]
archie repo receipt show   <receipt-id|sha>     # decode + local re-check, maps session refs back
archie repo receipt key    [--github]           # prints pubkey; --github prints the `gh ssh-key add --type signing` line
archie repo receipt verify --base SHA --head SHA [--ci ci.json] [--keys keys.json] [--format md|check-json]
```

`init` follows the existing rule that hooks are printed, never silently written (`HookCommand` "Never writes a settings file", `app.rs:455`; `repo suspect --hook` prints). `--write` is the explicit user action.

### 4.2 GitHub Action

- Lives at the root of `unfoundbox-crew/agentworth` so Marketplace traffic stars the main repo. GitHub requires one root `action.yml` per listed repo and a unique name; proposed name **AgentWorth Evidence**. Users pin `@v0`.
- Composite steps: install archie from the release tarball for `x86_64-unknown-linux-gnu` and verify its `.sha256` (cache by version) → `git fetch <head repo> refs/notes/agentworth:refs/notes/agentworth` → collect check runs/statuses and signing keys with `gh` (preinstalled on hosted runners) → `archie repo receipt verify` → `POST /repos/{o}/{r}/check-runs` named `AgentWorth / Evidence` with `output.title/summary` → mirror to `$GITHUB_STEP_SUMMARY`.
- Permissions: `contents: read`, `checks: write`, `pull-requests: read`. Fork PRs get a read-only token, so check-run creation returns 403; the Action falls back to the step summary and a notice, and docs show the `workflow_run` mode for forks. It never uses `pull_request_target` and never executes code from the PR.
- Inputs: `wait-for-ci` (600s), `fail-on` (empty), `transport` (auto), `archie-version` (matches tag).

### 4.3 MCP

`repo_receipt` (NEW tool, EXTEND server): returns the receipt the next push would sign plus the predicted check table, so an agent can say "this PR will show rung 3 attested, rung 1 claimed; run the tests" before it pushes. Read-only, no signing, no network. Same parameter shape as `repo_suspect`.

### 4.4 Docs and demo GIF

- 45-second vhs recording (`docs/dogfood/evidence-demo.tape`): agent session in a fixture repo → `git push` prints "receipt 7c41e0a9 signed, 2 sessions, rung 3 attested" → cut to the PR Checks tab showing the table → one commit with "claimed only" highlighted.
- One static PNG of the check for README and the Show HN post, taken from the dogfood workflow on a real AgentWorth PR, not a mock.

## 5. One-minute setup (README path)

```bash
# 1. In your repo, once (prints the workflow and the hook; --write writes both)
npx -y agentworth@latest repo receipt init --write

# 2. Commit the workflow it wrote, then push as usual
git add .github/workflows/agentworth.yml && git commit -m "Add AgentWorth evidence check" && git push

# 3. Open a PR. "AgentWorth / Evidence" appears under Checks.
```

The workflow it writes, also copy-pasteable on its own:

```yaml
name: AgentWorth
on: pull_request
permissions:
  contents: read
  checks: write
  pull-requests: read
jobs:
  receipt-verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: unfoundbox-crew/agentworth@v0
```

Optional, one line, for "Signed by @you": `archie repo receipt key --github` prints the `gh ssh-key add --type signing` command.

Maintainers can add only the workflow. Contributors without AgentWorth still get a check ("no receipt") with the install line, which is the distribution loop.

Budget: `init` to first check under 60 s on a repo with an existing index; the hook adds under 2 s per push (incremental scan scoped to sources touched in the window, then build and sign). Both measured in the test plan before launch.

## 6. Scope, tests, and PR plan

### 6.1 Out of scope for slice 1

Deploy and durability rungs (rung 6+, "still green after N days", revert detection). Sigstore or `actions/attest` countersigning (slice 2). Agent Trace line-level export (complementary: Agent Trace says who wrote lines; a Work Receipt says what was proven; a `--agent-trace` export is later). PR comments (check only). GitLab or other forges. Org dashboards, Factory Floor style rollups. Windows runners. Reusing `~/.ssh` keys or ssh-agent. Required-check enforcement presets. Changing how any rung is detected today.

### 6.2 Test plan

- Golden: canonical bytes and `receipt_id` for fixed fixtures; DSSE PAE vectors cross-checked against the in-toto reference implementation.
- Tamper: flip one payload byte, swap a subject SHA, re-sign with another key: each lands in the right state (invalid / not linked / claimed).
- Privacy: build receipts from the multilingual corpus (`apps/cli/tests/fixtures/gen_multilingual_corpus.py`) plus a fixture seeded with `sk-` keys, emails, `/Users/` and `/home/` paths. Assert none appear; assert the fail-closed redactor check trips when a test injects free text.
- Rung matrix: real temp git repos (the pattern in `apps/cli/src/commands/suspect/tests.rs`) for no receipt, partial coverage, rebased SHAs, files mismatch, CI pending, CI red, CI green, fork PR (403 path).
- Multi-harness: one commit touched by Claude and Codex sessions; both refs appear and the highest rung wins per commit.
- Hook: never exits non-zero, including when archie is missing, consent is absent, or the notes push fails; nested push guard works.
- Action e2e: CI job runs the composite action against a fixture repo with `act`-style inputs and snapshots the check JSON; a live sandbox PR before tagging `v0`.
- Performance: hook wall time on the real index (target under 2 s), measured and recorded in the spec.

### 6.3 PR plan for Dev (4 PRs)

Estimates are engineer-days, not measured token budgets. `docs/ROADMAP.md` requires measured estimates, so Dev re-estimates from the index after PR 1.

| PR | Contents | Depends on | Size |
| :-- | :-- | :-- | :-- |
| 1 | `crates/schema/src/work_receipt.rs`, new `crates/attest`, JSON Schema, golden and tamper tests | nothing | ~2 days |
| 2 | `attribute_commits()` factor-out, `WorkSession` projection, `repo receipt create/show/key/init`, consent + preview, notes and file transport, pre-push hook line, MCP `repo_receipt`, privacy tests | 1 | ~3 days |
| 3 | `repo receipt verify` (offline), rung re-derivation, conclusion policy, markdown + check JSON renderer, snapshot tests | 1 | ~2 days, parallel with 2 |
| 4 | `action.yml` + scripts, dogfood workflow, CI e2e job, `v0` tag step, README, learn page, demo tape, `extensions.md` amendment, spec status row | 2, 3 | ~2 days |

Target: all four merged and `v0.1.30` released with the Marketplace listing by Fri 30 Oct 2026, leaving November for the launch.

## 7. Decisions

| # | Decision | Status | Ask | COS-default |
| :-- | :-- | :-- | :-- | :-- |
| 1 | Receipt transport | Decided: COS default | Git notes (`refs/notes/agentworth`) or committed `.agentworth/receipts/` file as default? | Git notes default, file as a flag |
| 2 | Signing | Decided: COS default | Archie-owned ed25519 key with optional GitHub signing-key link, or Sigstore keyless now? | Local ed25519 now; Sigstore only for CI countersigning in slice 2 |
| 3 | Where the Action lives | Decided: COS default | `action.yml` at the root of the main repo (stars land on agentworth), or a separate `agentworth-action` repo? | Main repo root, listed as "AgentWorth Evidence", pinned `@v0` |
| 4 | Dollars in public receipts | Decided: COS default | Include API-equivalent spend by default, or tokens only with `--include-spend`? | Tokens only by default |
| 5 | Conclusion policy | Decided: COS default | Neutral by default, green only when fully covered and CI green, red only via opt-in `fail-on`? | Yes, as specified in §3.2 |
| 6 | Ship target | Decided: COS default | 4 PRs, `v0.1.30` plus Marketplace listing by 30 Oct, dogfooded on AgentWorth's own PRs? | Yes; Show HN only after the dogfood screenshot is real |

## 8. Implementation notes (where the build differs from the text above)

- Rungs are capped at 3 per outcome kind in a receipt so large sessions stay under the 64 KiB cap.
- The hook line is `archie repo receipt create --push --quiet --remote "${1:-origin}" || true`, so notes go to the remote being pushed.
- Consent is recorded by `archie repo receipt init --write`, not by a y/N prompt on the first `create`; without it `create` prints the preview only.
- `archie repo receipt show` maps session refs back to local sessions; there is no `archie session show --ref`.
- The Action also takes `archie-path` and `github-token` inputs. `archie-path` is how the dogfood workflow runs the Action with archie built from the same commit (`uses: ./`); a release cannot be used there because the Action and `repo receipt verify` change together.
- The Action leaves out every check run in its own check suite when reading head CI, so the job it runs in, and sibling jobs in the same workflow, never count toward rung 5. Put the Action in its own workflow (as the generated `agentworth.yml` does).
- Release binaries are installed for Linux X64 runners only, cached under `RUNNER_TOOL_CACHE` by version.
- `release.yml` moves `v0` on each stable release only when `v0` already exists; the first `v0` is created by a person with the Marketplace listing.
- Action e2e runs in `cargo test` (`apps/cli/tests/action_e2e.rs`): the real script against fixture repositories with a file-backed stand-in for `gh`, snapshotting the posted check JSON. A separate `Action scripts` CI job runs shellcheck.

## Sources

Repo (shallow clone of `main` at `54d5fa3`, 8 Oct 2026): every path cited above. External, checked 17 Oct 2026: in-toto attestation spec v1 (`github.com/in-toto/attestation/tree/main/spec/v1`: statement, envelope, digest_set `gitCommit`); GitHub REST "List SSH signing keys for a user" (`docs.github.com/en/rest/users/ssh-signing-keys`, "accessible by anyone"); GitHub artifact attestations (`docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations`, `actions/attest@v4`, permissions `id-token: write`, `attestations: write`); GitHub Marketplace publishing requirements (`docs.github.com/en/actions/how-tos/create-and-publish-actions/publish-in-github-marketplace`); Agent Trace v0.1.0 (`github.com/cursor/agent-trace`, `agent-trace.dev`).
