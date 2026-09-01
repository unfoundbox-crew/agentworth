# Market map: observability that opens PRs

Research date: 2026-09-01. Every claim below is dated to what the cited page said on or near that date — pricing and positioning in this space change often. Where I could not verify a claim from a primary source, I say so.

## The category, defined

A tool watches something (production traffic, a CI run, a static scan, a code diff, a dependency feed), decides something is wrong, and produces a code change. The dividing line that actually matters is not "has AI" — it's **who initiates the PR and from what signal**.

Three shapes show up in the market, and vendors blur them on purpose:

1. **Comments on a PR a human already opened.** Bugbot, Greptile, CodeRabbit, Ellipsis, Qodo PR-Agent. Zero autonomy over when to act — a human's `git push` is the trigger.
2. **Opens a PR from a scan or a feed, on its own schedule.** Renovate, Dependabot, Snyk Agent Fix, GitLab Duo Vulnerability Resolution, GitHub Copilot Autofix, DeepSource Autofix. The trigger is autonomous, but the signal is static — a dependency manifest, a SAST rule, a CodeQL alert. Nothing here has watched your app run.
3. **Opens a PR from a live production signal.** Sentry Seer (configurable), PostHog self-driving, TraceRoot, Arize AX Signal (Enterprise only). This is the smallest group and the one the question is really about.

Task-driven agents (Devin, Jules, Cursor Agent) don't belong in any of these rows — a human assigns the task, so there's no "observed something wrong" step. Investigation-only SRE agents (Datadog Bits AI, Cleric, Traversal) belong in a fourth, adjacent bucket: they find root cause and stop at a Slack message, no PR.

## Who opens PRs vs who only comments

| Tool | Opens a PR unprompted, from a signal | Only comments / suggests on a PR a human opened | Notes |
|---|---|---|---|
| Renovate / Dependabot | Yes | — | Feed = dependency registry + CVE DB, not runtime |
| Snyk Agent Fix (DeepCode AI Fix) | Yes | — | Feed = SAST findings |
| GitLab Duo Vulnerability Resolution | Yes | — | Feed = SAST findings, capped at 10 open MRs/project |
| GitHub Copilot Autofix (agentic) | Yes | — | Feed = CodeQL alerts; public preview since Jul 2026 |
| DeepSource Autofix | Yes | — | Feed = static analysis (5,000+ rules) |
| Sentry Seer (autofix) | Yes, when automation is enabled per-project | Also works on-demand | Feed = production error events (Sentry SDK) |
| PostHog self-driving | Yes | — | Feed = product analytics, error tracking, session replay, "scouts" |
| TraceRoot | Yes | — | Feed = LLM-judged production traces of *AI agent* apps specifically |
| Arize AX Signal | Yes, Enterprise tier only | Lower tiers: detection/report only | Feed = production LLM/agent traces |
| Blacksmith Codesmith Autofix | No — pushes fixes to an existing PR | Yes | Feed = failing CI checks + review comments on that PR |
| Vercel Agent | No — asks permission, patches in a sandbox | Yes, PR review comments + build-fix suggestions | Feed = deploy/build logs, runtime errors |
| Cursor Bugbot | — | Yes | Feed = PR diff only |
| Greptile | — | Yes | Feed = PR diff + full codebase context |
| CodeRabbit | — | Yes (one-click "commit suggestion" inside the PR, not a new PR) | Free for public repos |
| Ellipsis | — | Yes | $2M seed, YC W24, still operating as of Sep 2026 |
| Qodo PR-Agent (OSS) / Qodo platform | — | Yes | PR-Agent itself is Apache-2.0, free |
| Devin / Google Jules / Cursor Agent | Yes, but only after a human assigns a task | — | Not observation-triggered — excluded from the category proper |
| Datadog Bits AI SRE / Cleric / Traversal / Resolve AI | No PR found in my research | — | Investigation and root-cause only; Resolve AI's "graduated trust" remediation targets infra actions, not code PRs, as far as I could verify |
| Sweep | Deprecated | — | Original "issue → PR" GitHub bot retired; pivoted to a JetBrains IDE agent in 2026 |

Verified from primary sources (product docs, pricing pages, or GitHub): Renovate/Dependabot, Sentry Seer, PostHog, GitHub Copilot Autofix, GitLab Duo, Snyk, Cursor Bugbot, CodeRabbit, Qodo, TraceRoot's own README, Blacksmith's own docs. Not independently verified beyond search-result summaries: Arize AX Signal's Enterprise gating, Resolve AI's remediation scope, Cleric's write-access claim — these come from vendor blog posts and comparison sites, not something I fetched and read myself.

## What is the observation source

This is the axis every listicle skips, and it's the one that decides what kind of fix is even possible.

| Source | What it can catch | What it can't | Who uses it |
|---|---|---|---|
| Dependency/CVE feed | Known-vulnerable or outdated packages | Anything about your own code's behavior | Renovate, Dependabot |
| Static analysis / SAST | Pattern-matchable bugs and vulns in source | Anything that only shows up at runtime | Snyk, GitLab Duo, DeepSource, Copilot Autofix, Sourcery |
| PR diff | Style, obvious logic errors, missing tests, in the changed lines | Anything outside the diff, anything needing production context | Bugbot, Greptile, CodeRabbit, Ellipsis, Qodo |
| CI failure / build log | Compile errors, failing tests, broken builds | Bugs that pass CI but break in production | Blacksmith Codesmith, Vercel Agent (build-fix side) |
| Production error / trace | Actual runtime failures, with a real stack trace and real inputs | Anything that hasn't happened yet; needs volume to be confident | Sentry Seer, PostHog, Arize AX, TraceRoot, Vercel Agent (runtime side) |
| Product/session analytics | User-facing symptoms with no matching error (confusing UI, drop-off) | Root cause — has to be inferred, not observed directly | PostHog scouts specifically |
| Agent session trajectory (what was tried, what failed, what was retried, what was claimed done without evidence) | Which of your AI-generated changes are process-suspect before anyone hits the bug | The actual root cause of a given bug — a trajectory shows failed attempts, not the correct fix | Nobody but AgentWorth, as far as this research found |

The pattern: every vendor with an "observe → fix" product picked exactly one source, and the source is why their fixes look the way they do. SAST tools fix syntactic patterns. Sentry and PostHog fix things that actually broke for a user. Nobody in this list reads the record of *how the code was written* — only records of *how it behaves* or *how it's structured*.

## Business model and open-core line

| Company | Free / OSS layer | Paid layer | Where the line falls | Self-hostable |
|---|---|---|---|---|
| Renovate (Mend) | Full engine, AGPL-3.0 self-host; hosted Community app free | Mend Enterprise | Support, org-wide policy/governance, not the fix logic itself | Yes |
| Dependabot | Entirely free, built into GitHub | None | No paid tier at all | No (GitHub-hosted only) |
| Sentry | BSL core (converts to Apache-2.0 after 36 months); self-hostable | Seer is closed-source, $40/active contributor/month add-on on top of a paid plan | AI layer is 100% proprietary; core error tracking is source-available | Core: yes. Seer: no |
| PostHog | Core platform MIT-licensed (posthog-foss is the fully-stripped fork); self-driving in open beta | $15 per PR opened by a scout, first 3/month free, refund if you reject it, $150 default spend cap | Usage-metered on the AI action itself, not a seat fee | Core: yes. Self-driving: cloud-only as far as I found |
| TraceRoot | Apache-2.0 core, "additional Enterprise features under a separate license" | Enterprise tier (unspecified in docs) | Docs don't spell out exactly what's Enterprise-only yet — company is ~7 months old | Yes, three documented deployment modes |
| Blacksmith | None | Entirely paid; usage-based at $1.75 per "agent compute unit" (~1 min of frontier-model thinking time), $100 session cap | No free/OSS layer at all | No — hosted only |
| GitHub Copilot Autofix | CodeQL scanning free on public repos | Agentic autofix draws down Copilot "AI credits" (usage-based) | Static-alert autofix is free; the agentic version that opens PRs is metered | No |
| GitLab Duo | Core GitLab has a free tier | Vulnerability Resolution requires Duo Enterprise, $39/user/month, Ultimate-tier only | Whole AI layer gated behind the top subscription tier | Yes (self-managed GitLab) |
| Snyk | Free tier, 100 tests/month | Team $25/dev/month, Ignite $1,260/dev/year | Autofix is not on the free tier | No |
| Cursor Bugbot | — | Usage-based since Jun 2026, ~$1–1.50/review, billed against Cursor plan usage | Bundled into Cursor's IDE subscription | No |
| Greptile | — | $30/dev/month base, includes 50 reviews, $1/review after | No free tier | No |
| CodeRabbit | Unlimited on public repos, no card required | $24–30/dev/month (Essentials), $48–60/dev/month (Team), custom Enterprise (adds self-hosting) | Self-hosting is an Enterprise-only unlock | Enterprise only |
| Ellipsis | Free, unlimited on public repos | $20/dev/month private | Straightforward public/private split | No |
| Qodo | PR-Agent (the original engine) is Apache-2.0, fully free, BYO API key | Qodo platform: $0 (30 PRs/mo), $15–30/user paid tiers, Enterprise | The open engine and the commercial product are two different codebases now, not a tiered version of one | PR-Agent: yes. Qodo: no |
| Vercel Agent | Included, no seat license | Bundled into Pro/Enterprise Vercel plans | Not sold separately | No |
| Arize AX | Free: 25K spans/mo; Pro $50/mo | PR-opening ("managed agents") is Enterprise-only | The PR-writing capability itself is the paywall, not just volume | No |

## Funding and scale, where public

| Company | Stage / round | Valuation | Headcount / scale | Verified how |
|---|---|---|---|---|
| Cognition (Devin) | $1B+ raised May 2026, led by Lux Capital / General Catalyst | $25–26B | $492M ARR (annualized), 89% of Cognition's own code written by Devin | Multiple press outlets, consistent numbers — not fetched from Cognition directly |
| PostHog | $194M total raised; $75M Series E, Oct 2025 | $1.4B | 213 employees (Jul 2026); $57.5M ARR (Feb 2026) | Search aggregators (Sacra, Tracxn) — not a primary filing |
| Blacksmith | $45M Series B, Aug 2026, led by Peak XV | $550M | 6,000+ companies using it (per their own announcement) | Blacksmith/PR Newswire press release |
| Resolve AI | ~$190M total, $1.5B valuation after Apr 2026 extension | $1.5B | Founded by ex-Splunk observability SVP/architect | Comparison-site summaries — treat as unconfirmed |
| Traversal | $48M Series A | Not found | Cites American Express as a customer (32% MTTR reduction claim, unverified independently) | Company's own blog post — self-reported |
| Cleric | $9.8M seed | Not found | — | Comparison-site summary |
| Arize | $131M total raised | Not found | Operating since 2020 | Search aggregator |
| Qodo (CodiumAI) | $40M Series A, Sep 2024 | Not found | — | Search aggregator |
| Greptile | $30M total ($25M Series A Sep 2025 + $4.1M seed) | $180M | — | TechCrunch-style coverage, consistent across sources |
| Sentry | Not researched — well-established, profitable-adjacent company; Seer is a new line, not a separate funded entity | — | — | — |
| TraceRoot | ~$500K–1M, pre-seed, YC S25 | Not found | 1–10 employees | YC company page + funding aggregators |

Read the whole table as "search-result strength," not audited fact. The dollar figures for well-covered rounds (Cognition, Blacksmith, PostHog) showed up consistently across multiple independent outlets, which is the closest this kind of research gets to verification without a primary filing. The smaller, newer companies (TraceRoot, Cleric, Traversal) have thinner coverage and I'm reporting what's out there, not confirming it.

## What nobody is doing

Three gaps stood out across roughly twenty products:

**1. Nobody fixes based on how the code was written, only on how it behaves or is structured.** Every autofix product in this research keyed off one of: a dependency version, a static-analysis rule, a PR diff, a CI failure, or a production error. None of them read the record of the agent session that produced the code — the retries, the abandoned approaches, the point where an agent said "done" without running a test. That data source doesn't appear anywhere in this map.

**2. Nobody spans coding harnesses.** Every review/autofix tool here is scoped to one repo and one signal type. None of them aggregate across Claude Code, Cursor, Codex, and a dozen other CLIs to ask "which of my AI-authored commits, across every tool my team used this week, are the suspect ones." That's a different question than "review this PR" or "fix this error," and nothing in this list answers it.

**3. Nobody gates autonomy on verified evidence the way an outcome ladder does.** Sentry's automation trigger is a model-scored "fixability" number. PostHog's is a scout's own judgment plus a refund if you didn't like the result. Nobody in this research ties the decision to auto-open a PR to an evidence chain like "tests actually ran and passed" versus "the agent claimed it passed." That's a narrower, more mechanical kind of gate than anything found here — closer to CI-verification than to LLM self-assessment.

## Is this space crowded or early

Crowded at the edges, thin in the middle. PR-comment tools (Bugbot, Greptile, CodeRabbit, Ellipsis, Qodo) are a saturated, commoditizing market — five products doing close to the same thing, fighting on price and bug-catch-rate benchmarks. Static-scan autofix (Snyk, GitLab, Copilot Autofix, DeepSource) is mature, vendor-owned, and mostly a checkbox feature bundled into existing AppSec or platform products, not a standalone bet. Boring prior art (Renovate, Dependabot) has been fully solved and given away free for years — that's the proof this shape of product *can* work at massive scale, just not that it's monetizable on its own.

The genuinely early, genuinely small group is "opens a PR from a live production signal, on its own initiative": Sentry Seer, PostHog self-driving, TraceRoot, Arize AX Signal. All four either shipped or moved into general availability within the last 6–9 months as of this research date. TraceRoot is pre-seed. PostHog's self-driving mode is in open beta. Sentry gates automatic PR-opening behind a fixability score and per-project opt-in, which reads as the company not yet trusting the mechanism to run unsupervised by default. This sub-category is early, not crowded — but it sits inside markets (error tracking, product analytics) that are themselves extremely crowded and well-funded, so a new entrant isn't competing on "does an autofix product exist" so much as "can you out-observe Sentry or PostHog on their own turf."

## Does AgentWorth's observation source produce fixes nobody else can produce

AgentWorth's source is genuinely unclaimed. Nothing found in this research reads agent trajectory data — the sequence of what an agent tried, what it retried, what it gave up on, what it marked "done" without evidence. That's a real gap, not a marketing gap.

But a trajectory log answers a different question than a stack trace does. A production error tells you *what broke and where* — line, input, call stack. A trajectory tells you *how suspiciously the code was written* — an agent that ran the same failing command five times, or claimed success without running tests, is a signal about process risk, not a pointer to the bug itself. Knowing a session was sloppy doesn't tell you what the sloppy code actually does wrong; you'd still need a stack trace, a failing test, or a static-analysis hit to generate the fix. Trajectory data is upstream of the signal every other tool in this market already uses, not a replacement for it.

So the honest shape of a paid layer here is not "our intelligence observes, then opens PRs" in the TraceRoot sense — AgentWorth doesn't watch a running system, it watches how the system was built. The defensible version is narrower: **use trajectory data to triage, not to fix.** Flag which merged, AI-authored commits are process-suspect — no test-run evidence, a claimed-done with no verification, a session that visibly fought the same error repeatedly — before a bug report or a production error ever fires, and route those specific commits into a downstream fixer (Sentry, a static scanner, or a human) that has the signal needed to actually generate a correct patch. That's a real, differentiated product: nobody else in this market can point at "this PR's own authoring process was risky" as a reason to look twice. It is not the same claim as "we can write a better fix than a production stack trace can," and the honest answer is that trajectory data alone probably can't back that stronger claim.
