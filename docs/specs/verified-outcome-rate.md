# Verified outcome rate

Status: proposed, measured 2026-09-02.

## The one-line version

Of the sessions that claimed done, what share left evidence — per model, per
adapter, per repo, with the sample size printed next to every row.

## The problem, stated by the person who has it

I run a lot of sessions. I have no idea which of them actually worked.

Every harness tells me a session finished. None of them tell me whether the
finish was real. So the only quality signal I have is a feeling, and a feeling
does not tell me to stop using a model on a repo where it keeps failing.

I do not want a leaderboard against other people. I want one number about my
own work that moves when I change something.

## The measurement

Everything below comes from a copy of the local index, `sessions` table.
10,329 rows total, 2,960 of them non-stub (`total_events > 1 AND
total_tokens > 0`). The copy was written by a pre-0.1.13 binary, so it has no
`compaction_count` column and predates #68 — 1,323 of the 10,329 rows resolve
to `plugins/cache` and a fresh full scan now prunes them. Every count below is
non-stub only.

The ladder is already stored, one row per session, as the highest rung reached:

    1 done_claimed   2 artifact_changed   3 test_or_build_passed
    4 commit_observed   5 ci_or_deployment_verified

"Claimed done" is the denominator: any session with a non-null
`primary_outcome`. "Verified" is rung 3 or higher — something ran and passed.

```sql
WITH r AS (
  SELECT j.value AS model,
    CASE s.primary_outcome
      WHEN 'done_claimed' THEN 1 WHEN 'artifact_changed' THEN 2
      WHEN 'test_or_build_passed' THEN 3 WHEN 'commit_observed' THEN 4
      WHEN 'ci_or_deployment_verified' THEN 5 END AS rung
  FROM sessions s, json_each(s.models_used) j
  WHERE s.primary_outcome IS NOT NULL
    AND s.total_events > 1 AND s.total_tokens > 0)
SELECT model, COUNT(*) n, SUM(rung>=3) verified,
       ROUND(100.0*SUM(rung>=3)/COUNT(*),1) pct
FROM r GROUP BY model HAVING n >= 20 ORDER BY pct DESC;
```

**Own average: 68.4%** — 1,002 verified of 1,464 sessions that claimed done.

By model. 37 of 43 models fell under the floor and are not shown:

| model | n | verified | rate |
| :--- | ---: | ---: | ---: |
| claude-fable-5 | 210 | 162 | 77.1% |
| claude-opus-5 | 537 | 409 | 76.2% |
| claude-opus-4-8 | 195 | 146 | 74.9% |
| claude-sonnet-5 | 531 | 319 | 60.1% |
| claude-haiku-4-5 | 33 | 18 | 54.5% |
| deepseek-v4-flash-free | 20 | 9 | 45.0% |

By adapter. Only two clear the floor, and that is the finding:

| adapter | n | verified | rate |
| :--- | ---: | ---: | ---: |
| claude_code | 1,426 | 983 | 68.9% |
| opencode | 38 | 19 | 50.0% |

Seven other adapters hold 3,869 rows between them and produce 49 outcomes
total. `codex`, `cursor`, `hermes`, `pi` and `gemini` produce zero. The rate is
not low for those adapters — it is undefined, because nothing detects an
outcome there at all. See `capability-matrix.md`.

By repo, using `extract_repository_or_workspace` (worktree suffix pruned).
22 of 37 repos fell under the floor:

| repo | n | verified | rate |
| :--- | ---: | ---: | ---: |
| video/frontend | 35 | 35 | 100.0% |
| upscaler/frontend | 129 | 113 | 87.6% |
| mvec/engine | 84 | 71 | 84.5% |
| apps/vibelaunch | 239 | 178 | 74.5% |
| code/motionvector | 251 | 164 | 65.3% |
| upscaler/backend | 211 | 98 | 46.4% |
| katana/video | 30 | 12 | 40.0% |
| tinkers/blog | 20 | 5 | 25.0% |

**Repo spreads wider than model.** 75 points between the best and worst repo,
32 between the best and worst model, at the same n floor. Whatever
drives the outcome rate here, it is more about the codebase than the model.
That is the opposite of what a model leaderboard would suggest, and it is the
reason `group_by` defaults to `repo`.

## The MCP tool

    outcome_rate(group_by, since?, until?, min_n?, include_stubs?)

| Param | Type | Default |
| :--- | :--- | :--- |
| `group_by` | `model` \| `adapter` \| `repo` | required, no default |
| `since`, `until` | RFC 3339 | whole index |
| `min_n` | integer | 20 |
| `include_stubs` | boolean | false |

Returns:

```json
{ "group_by": "repo", "min_n": 20,
  "window": {"since": "2026-08-03T00:00:00Z", "until": "2026-09-02T...Z"},
  "baseline": {"n": 923, "verified": 644, "rate": 0.698},
  "rows": [{"key": "upscaler/frontend", "n": 129, "verified": 113,
            "rate": 0.876, "delta_vs_baseline": 0.178,
            "rungs": {"1": 2, "2": 14, "3": 61, "4": 40, "5": 12}}],
  "suppressed_groups": 22,
  "receipt": {"session_ids": ["…", "…"], "counted_at": "2026-09-02T…Z",
              "index_last_session_at": "2026-09-01T18:58:12Z",
              "db_path": "~/.agentworth/agentworth.db"}}
```

`baseline` is always the caller's own rate over the same window. There is no
other-people comparison, now or later — the index is one machine's and a
cross-user number would be a different product.

`receipt` carries the session ids behind each row (capped at 50 per row, with
`truncated`), the time the count was taken, and the newest session in the
index. A rate computed against a stale index is a wrong rate; printing
`index_last_session_at` lets the caller notice without asking.

**The "I don't know" case is a real return value, not an error.** When a group
falls under `min_n`, it is counted in `suppressed_groups` and omitted. When a
group has rows but zero non-null outcomes — every adapter except `claude_code`
and `opencode` today — the row comes back as
`{"key": "codex", "n": 0, "rate": null, "reason": "no_outcome_detection"}`.
A null rate reads as "this adapter's outcomes are not parsed"; a 0.0 rate would
read as "this adapter always fails". They are not the same claim.

## New work

`SessionFilter` has no `repo` field and no aggregate query. Three things:

1. `Storage::get_outcome_rate(group_by, window, min_n) -> Vec<OutcomeRateRow>`.
   One `GROUP BY` for `adapter`, a `json_each` join for `model`, and a Rust
   post-group on `extract_repository_or_workspace` for `repo` — the same
   fetch-then-filter shape `sessions_find` already uses.
2. A rung ordering exported from `agentworth-outcomes` so the CASE ladder is
   defined once, not re-typed in SQL. `confidence_for_outcome` is already
   duplicated in `blind_spots.rs` and `recall.rs`; do not make it three.
3. The tool itself in `apps/cli/src/mcp/`.

## Deliberately not built

- **No pass/fail verdict.** The tool returns a rate and a baseline. Whether
  74% is good is not something the data knows.
- **No cross-user comparison.** Not deferred — excluded.
- **No auto-picked model.** `questions.md` is right that one developer's habits
  cannot support "model X executes better". The per-model table above is a
  description of this machine, and the tool says so.
- **No UI.** The tool ships first and alone.

## Sequencing

1. The rung ordering, shared. Cheap, and it deletes duplication.
2. `get_outcome_rate` plus tests over a fixture index.
3. The MCP tool.
4. Only then, if it gets used, a dashboard row.

## Open questions

- Is 20 the right floor? It was picked to hide noise, not measured.
- Should a session with no detected outcome count as a failure or be excluded?
  Excluded here — 1,496 non-stub sessions have no outcome at all, and folding
  those into a denominator would halve every rate for a parsing reason.
- Does the rate mean anything once compaction is in the index? A session
  compacted eight times and one that never compacted are averaged together
  today.
