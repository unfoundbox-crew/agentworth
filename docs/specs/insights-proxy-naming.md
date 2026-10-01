# Insights proxy naming (session user_messages ≠ human_turns)

Status: built 2026-10-01 (honesty row #7). Companion to the human-turn lane
(`crates/adapters/src/human_turns/`) and `/api/insights`
(`crates/storage/src/insights.rs`).

## The problem

Several `/api/insights` fields summed `sessions.user_messages_count` but were
named as if they counted the `human_turns` table:

| Old JSON field | Actual SQL |
| :--- | :--- |
| `volume.human_turns_index_proxy` | `SUM(user_messages_count)` |
| `by_adapter[].human_turns` | per-adapter `SUM(user_messages_count)` |
| `calls_per_turn_by_adapter[].human_turns` | same (calls/turn denominator) |
| `top_sessions[].human_turns` | that session's `user_messages_count` |

On a measured Codex slice, session `user_messages` can be thousands while
`human_turns` rows from `~/.codex/history.jsonl` are hundreds (SPEC #216 /
#224). Colliding names made the proxy look like the table.

The real `human_turns` table feeds `day_hour`, `friction`, and `vocabulary`
only.

## Contract (schema_version 3)

| Honest JSON field | Deprecated alias (still emitted) |
| :--- | :--- |
| `volume.session_user_messages` | `volume.human_turns_index_proxy` |
| `by_adapter[].user_messages` | `by_adapter[].human_turns` |
| `calls_per_turn_by_adapter[].user_messages` | `…[].human_turns` |
| `top_sessions[].user_messages` | `…[].human_turns` |

Semantics are unchanged — only names. Prefer the honest fields; aliases are
for one tip cycle so existing readers do not break silently.

`INSIGHTS_SCHEMA_VERSION` is **3**.

## Non-goals

- Changing the SQL or the `human_turns` ingest lane.
- Removing aliases in the same PR that introduces the honest names.
- Renaming `day_hour` / `friction` / `vocabulary` (those already read the
  real table).

## Related

- Honesty matrix / detected vs indexed: `docs/capability-matrix.md`
- Codex human turns: `docs/specs/codex-human-turns.md`
