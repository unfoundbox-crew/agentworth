/**
 * The ONE mapping layer between the real backend payload (#191, Rust
 * `agentworth_storage::insights`, served at GET /api/insights) and the deck's
 * UI types (#192, ../insights.ts). The Rust output is the wire contract the
 * MCP surface shares, so it is never renamed — everything adapts here.
 *
 * Rules this mapping lives by:
 * - A number the backend does not produce is never invented. Structurally
 *   deferred metrics (friction, hour-of-day heatmap, repeated vocabulary)
 *   arrive as empty UI arrays and the widgets say so honestly; the payload's
 *   own `deferred` list names the gap.
 * - `previous` is synthesized from `deltas` (the backend computes exactly two
 *   windows for the five headline KPIs; it carries no per-rung previous
 *   payload). When a dimension filter narrows the window, Δ hides instead of
 *   comparing a filtered number against an unfiltered base.
 * - Unit conversions are exact scale shifts only: `_tb` fields are tokens in
 *   billions (Rust total_tokens / 1e9), percentages stay percentages.
 */

// types only: this module is imported for its functions by ./insights.ts, so keep the
// cycle purely type-level (erased at compile time).
import type { Insights, InsightsFilter, InsightsWindow, OutcomeKind } from './insights';

/* ---------- the raw backend shape (field names transcribed from #191's Rust structs) ---------- */

/** Rust `InsightsDeltaMetric`: numbers as json values, `null` when undefined on a window. */
export interface RawDeltaMetric {
  current: number | null;
  previous: number | null;
  delta: number | null;
  delta_pct: number | null;
  reason?: string | null;
}

export interface RawLadderRow {
  outcome: string;
  sessions: number;
  total_tokens: number;
}

export interface RawModelRow {
  model: string;
  sessions: number;
  total_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  input_tokens: number;
  output_tokens: number;
}

/** The full serde payload of GET /api/insights, verbatim from agentworth_storage::insights. */
export interface RawInsights {
  schema_version: number;
  generated_at: string;
  window: { sessions: { min_started_at: string | null; max_started_at: string | null } };
  filter: { since: string | null; until: string | null };
  deltas: {
    window: { since: string; until: string };
    previous_window: { since: string; until: string } | null;
    usable_sessions: RawDeltaMetric;
    verified_outcome_rate: RawDeltaMetric;
    tool_calls_per_turn_strict: RawDeltaMetric;
    token_burn: RawDeltaMetric;
    friction_rate: RawDeltaMetric;
  };
  population: {
    sessions_raw: number;
    sessions_conversation_multi_event: number;
    sessions_excluded_pre_2020: number;
    sessions_with_tool_inventory: number;
    usable_sessions: number;
  };
  volume: {
    usable_sessions: number;
    tool_calls_witnessed: number;
    human_turns_index_proxy: number;
    assistant_messages: number;
    total_tokens: number;
  };
  by_adapter: { adapter: string; sessions: number; tool_calls: number; human_turns: number; input_output_tokens: number }[];
  ladder: RawLadderRow[];
  verified: { sessions: number; total_tokens: number; share_pct: number };
  no_evidence_small_sessions: number;
  calls_per_turn: { strict: number | null; heavy_session_average: number | null };
  calls_per_turn_by_adapter: { adapter: string; calls_per_turn: number; tool_calls: number; human_turns: number }[];
  turn_buckets: { bucket: string; label: string; sessions: number }[];
  file_modifications: {
    total: number;
    sessions_with_file_mods: number;
    by_action: { key: string; count: number }[];
    worktrees: { total: number; sessions: number };
  };
  top_repos: { repo: string; file_touches: number; sessions: number }[];
  top_sessions: {
    session_id: string;
    adapter: string;
    started_at: string;
    total_tokens: number;
    total_events: number;
    tool_calls: number;
    human_turns: number;
    duration_hours: number;
    source_path: string;
    primary_outcome: string | null;
    repo_label: string;
  }[];
  session_size_buckets: { bucket: string; label: string; sessions: number }[];
  models: RawModelRow[];
  models_totals: { total_tokens: number; cache_read_tokens: number; cache_read_share_pct: number };
  series: {
    monthly_by_adapter: { month: string; adapter: string; sessions: number }[];
    sessions_by_month: { month: string; sessions: number }[];
    daily: { date: string; sessions: number }[];
    outcome_by_month: { month: string; outcome: string; sessions: number }[];
    verified_by_month: { month: string; sessions: number }[];
  };
  tool_buckets: { shell_edit: number; file_edit: number; read_explore: number; web_browser: number; agent_task: number; other: number };
  tool_buckets_detail: { key: string; count: number }[];
  coverage_flags: { dimension: string; status: string; signal: string }[];
  deferred: { dimension: string; reason: string }[];
}

/* ---------- dev-only warnings: at most once per key ---------- */

const warned = new Set<string>();

/** Vite sets import.meta.env.DEV at build time; cast keeps tsc strict without vite/client types. */
function envDev(): boolean {
  const env = (import.meta as unknown as { env?: { DEV?: boolean } }).env;
  return !!env?.DEV;
}

function warnOnce(key: string, message: string): void {
  if (warned.has(key)) return;
  warned.add(key);
  if (envDev()) console.warn(`[insights-backend] ${key}: ${message}`);
}

/* ---------- scale helpers ---------- */

/** Rust `total_tokens` (tokens) → UI `_tb` billions. Exact scale shift, no rounding drift. */
export function toB(tokens: number): number {
  return tokens / 1e9;
}

/** Percent share that needs computing rather than trusting the backend to have it. */
function share(part: number, whole: number): number {
  if (whole === 0) return 0;
  // Same 4-dp rounding the Rust side uses (round4) *100 → pct with 2 dp of the pct.
  return Math.round((part / whole) * 1_000_000) / 10_000;
}

function num(v: unknown, fallback: number): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

function optNum(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

/* ---------- mapping ---------- */

/**
 * The payload's `verified` rate citizens: rung-4+ ladder rows. Backend
 * `verified.share_pct` uses the same three rungs over the same population, so
 * the mapped ladder and the mapped hero KPI agree by construction.
 */
export function verifiedRungCount(ladder: RawLadderRow[]): number {
  return ladder
    .filter((r) => r.outcome === 'commit_observed' || r.outcome === 'test_or_build_passed' || r.outcome === 'ci_or_deployment_verified')
    .reduce((s, r) => s + r.sessions, 0);
}

function mapCurrent(raw: RawInsights): InsightsWindow {
  const deferredNames = new Set(raw.deferred.map((d) => d.dimension));

  const kpis: InsightsWindow['kpis'] = [
    { key: 'verified', label: 'verified outcome rate', value: num(raw.verified.share_pct, 0), unit: '%', series: raw.series.verified_by_month.map((m) => m.sessions) },
    { key: 'sessions', label: 'sessions', value: raw.population.usable_sessions, series: raw.series.daily.map((d) => d.sessions) },
    { key: 'tool_calls', label: 'tool calls', value: raw.volume.tool_calls_witnessed },
    { key: 'turns', label: 'human turns', value: raw.volume.human_turns_index_proxy },
    { key: 'tokens', label: 'token burn', value: toB(raw.volume.total_tokens), unit: 'B' },
  ];

  const ladder = raw.ladder.map((r) => {
    if (!['no_outcome_evidence', 'done_claimed', 'artifact_changed', 'test_or_build_passed', 'commit_observed', 'ci_or_deployment_verified'].includes(r.outcome)) {
      warnOnce('ladder', `unmapped ladder rung "${r.outcome}" — rendered as data; UI only styles the six known rungs`);
    }
    return { outcome: r.outcome as OutcomeKind, sessions: r.sessions, tb: toB(r.total_tokens) };
  });

  // friction and the hour×day heatmap are structurally deferred by the index
  // (payload.deferred names them); empty, never zero-filled.
  if (deferredNames.has('friction_triggers')) warnOnce('friction', 'index defers friction classification — friction widgets stay empty, never zero-filled');
  if (deferredNames.has('time_of_day_histogram')) warnOnce('day_hour', 'index stores one started_at per session — the hour×day heatmap stays empty, never zero-filled');
  if (deferredNames.has('vocabulary_mentions')) warnOnce('vocabulary', 'per-turn text is not indexed — repeated vocabulary stays empty');

  return {
    claimed: raw.population.usable_sessions,
    kpis,
    ladder,
    friction: [],
    day_hour: [],
    by_adapter: raw.by_adapter.map((a) => ({ adapter: a.adapter, sessions: a.sessions, tokens_tb: toB(a.input_output_tokens) })),
    models: raw.models.map((m) => ({
      model: m.model,
      sessions: m.sessions,
      tb: toB(m.total_tokens),
      // per-model cache-read share: the backend ships tokens, share is exact arithmetic
      cr_perc: share(m.cache_read_tokens, m.total_tokens),
      it: toB(m.input_tokens),
      ot: toB(m.output_tokens),
    })),
    buckets: raw.session_size_buckets.map((b) => ({ label: b.label, sessions: b.sessions })),
    repos: raw.top_repos.map((r) => ({ repo: r.repo, sessions: r.sessions, events: r.file_touches })),
    vocabulary: [],
    big_sessions: raw.top_sessions.map((s) => ({
      adapter: s.adapter,
      date: s.started_at.slice(0, 10),
      tokens: s.total_tokens,
      events: s.total_events,
      repo: s.repo_label || undefined,
    })),
  };
}

/**
 * The backend computes the previous window only for the five headline KPIs
 * (`deltas.*.previous`), never as a full window payload. The mapped `previous`
 * is therefore kpis-only: the KPI strip gets real Δ by key; widgets that need
 * per-rung previous data render honestly without it.
 */
function mapPrevious(raw: RawInsights): InsightsWindow | null {
  if (!raw.deltas.previous_window) return null;
  const d = raw.deltas;
  const prevUsable = optNum(d.usable_sessions.previous);
  const emptyArray = [] as never[];
  const kpis: InsightsWindow['kpis'] = [];
  const verifiedPrev = optNum(d.verified_outcome_rate.previous);
  if (verifiedPrev != null) kpis.push({ key: 'verified', label: 'verified outcome rate', value: verifiedPrev, unit: '%' });
  if (prevUsable != null) kpis.push({ key: 'sessions', label: 'sessions', value: prevUsable });
  const tokenPrev = optNum(d.token_burn.previous);
  if (tokenPrev != null) kpis.push({ key: 'tokens', label: 'token burn', value: toB(tokenPrev), unit: 'B' });
  return {
    claimed: prevUsable ?? 0,
    kpis,
    ladder: emptyArray,
    friction: emptyArray,
    day_hour: emptyArray,
    by_adapter: emptyArray,
    models: emptyArray,
    buckets: emptyArray,
    repos: emptyArray,
    vocabulary: emptyArray,
    big_sessions: emptyArray,
  };
}

/** Shape check at the JSON boundary: a payload missing its spine is an error, not a guess. */
export function isBackendShape(json: unknown): json is RawInsights {
  const v = json as RawInsights | null;
  return !!v
    && typeof v.schema_version === 'number'
    && typeof v.generated_at === 'string'
    && !!v.window && !!v.window.sessions
    && !!v.deltas && !!v.deltas.usable_sessions
    && Array.isArray(v.ladder)
    && !!v.verified
    && Array.isArray(v.top_sessions)
    && Array.isArray(v.models);
}

/**
 * The single entry: real backend payload → deck `Insights`. Throws on a
 * payload without its spine (same discipline the real route's own docs state).
 */
export function mapInsights(raw: RawInsights): Insights {
  if (!raw || typeof raw !== 'object') throw new Error('insights shape did not match the contract');
  if (!Array.isArray(raw.ladder) || !raw.verified || !raw.population) {
    throw new Error('insights shape did not match the contract');
  }
  const since = raw.filter.since ?? raw.window.sessions.min_started_at ?? '';
  // The echoed `filter.until` is null for a `since`-only query whose horizon the
  // backend materialized in deltas.window; prefer the echoed value, fall back.
  const until = raw.filter.until || raw.deltas.window.until || raw.window.sessions.max_started_at || '';
  const allTime = !raw.filter.since;
  return {
    generated_at: raw.generated_at,
    since,
    until,
    current: mapCurrent(raw),
    previous: allTime ? null : mapPrevious(raw),
  };
}

/* ---------- client-side dimension narrowing ---------- */

/**
 * The backend reads only `since`/`until` (#191's contract, shared with MCP).
 * adapter/model/repo narrowing therefore happens here, on per-row widgets the
 * backend tags per entity. Aggregates (hero rate, ladder, heatmap, KPI strip
 * values) stay population-wide — narrowed numbers we cannot derive would be
 * fabricated, so they are not.
 */
export function applyDimensionFilter(win: InsightsWindow, f: Pick<InsightsFilter, 'adapter' | 'model' | 'repo'>): InsightsWindow {
  if (!f.adapter && !f.model && !f.repo) return win;
  return {
    ...win,
    by_adapter: f.adapter ? win.by_adapter.filter((a) => a.adapter === f.adapter) : win.by_adapter,
    models: f.model ? win.models.filter((m) => m.model === f.model) : win.models,
    repos: f.repo ? win.repos.filter((r) => r.repo === f.repo) : win.repos,
    big_sessions: f.adapter
      ? win.big_sessions.filter((s) => s.adapter === f.adapter)
      : f.repo
        ? win.big_sessions.filter((s) => s.repo === f.repo)
        : win.big_sessions,
  };
}

export function hasDimensionFilter(f: Pick<InsightsFilter, 'adapter' | 'model' | 'repo'>): boolean {
  return !!(f.adapter || f.model || f.repo);
}
