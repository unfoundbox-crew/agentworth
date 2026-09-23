/**
 * Wire contract for GET /api/insights?since=&until= — the numbers behind the
 * deck's insights page. snake_case, matching every other CLI-server route
 * (apps/cli/src/server/routes.rs). The Rust lane lands the endpoint in
 * parallel; this file is the contract to build against. If the real route
 * renames a field, change it here and nowhere else.
 *
 * Contract notes, so the mock and the real endpoint produce the same shape:
 * - ONE response carries the selected window (`current`) and the matching
 *   previous window (`previous`, same shape or null). Δ and sparklines render
 *   from that pair; nothing is computed against a second request.
 * - Every magnitude is a plain number, units in the field name
 *   (`_tb` = trillions of tokens as billions, `_perc` = percent).
 * - `ladder` uses the six OutcomeKinds, any order; `day_hour` may omit empty
 *   cells. Thin coverage says so honestly — never zero-filled.
 *
 * Vocabulary (apps/home/DESIGN.md): rung 0 is "unflown" — absence of
 * evidence, never danger — and colour is spent only where earned.
 */

export const LADDER = [
  { outcome: 'no_outcome_evidence', label: 'unflown' },
  { outcome: 'done_claimed', label: 'said' },
  { outcome: 'artifact_changed', label: 'diff' },
  { outcome: 'test_or_build_passed', label: 'test ok' },
  { outcome: 'commit_observed', label: 'commit' },
  { outcome: 'ci_or_deployment_verified', label: 'ci' },
] as const;

export type OutcomeKind = (typeof LADDER)[number]['outcome'];

export const FRICTION_LABEL: Record<string, string> = {
  loop_interruption: 'loop interrupted',
  autonomy_nudge: 'nudged to continue',
  hallucination_pushback: 'hallucination pushback',
  sarcasm_cynicism: 'sarcasm / cynicism',
  context_amnesia: 'context amnesia',
  sandbox_error: 'sandbox error',
};

/** One window of insight over the local index. */
export interface InsightsWindow {
  /** Count of sessions with any outcome claim (the denom for the hero tile). */
  claimed: number;
  kpis: {
    key: string;
    label: string;
    value: number;
    /** Suffix shown after the number ("%" optional). */
    unit?: string;
    /** Sessions/day (or per-window buckets) for the tile sparkline. Optional —
     * absent means no proven series, so no sparkline is drawn. */
    series?: number[];
  }[];
  ladder: { outcome: OutcomeKind; sessions: number; tb: number }[];
  friction: { trigger: string; sessions: number }[];
  day_hour: { dow: number; hour: number; sessions: number }[];
  by_adapter: { adapter: string; sessions: number; tokens_tb: number }[];
  models: {
    model: string;
    sessions: number;
    tb: number;
    cr_perc: number;
    it: number;
    ot: number;
  }[];
  buckets: { label: string; sessions: number }[];
  repos: { repo: string; sessions: number; events: number }[];
  vocabulary: { term: string; sessions: number }[];
  big_sessions: {
    adapter: string;
    date: string;
    tokens: number;
    events: number;
    repo?: string;
  }[];
}

export interface Insights {
  generated_at: string;
  since: string;
  until: string;
  current: InsightsWindow;
  previous: InsightsWindow | null;
}

/** The server answers with a window it cannot resolve yet, or an empty window. */
export type InsightsStatus = 'loading' | 'run_in_progress' | 'error' | 'ready';

export interface InsightsState {
  status: InsightsStatus;
  data: Insights | null;
}

export const WEBS = ['7d', '30d', '90d', 'all'] as const;
export type WebDuration = (typeof WEBS)[number];

export interface InsightsFilter {
  /** Preset key; the client turns it into since/until. "all" sends nothing and
   * the server opens its full span. */ web: WebDuration;
  adapter: string | null;
  model: string | null;
  repo: string | null;
}

export const DEFAULT_FILTER: InsightsFilter = { web: '30d', adapter: null, model: null, repo: null };

/** since = now minus days; queries were moved off client clocks only for preset math. */
export function sinceFor(web: WebDuration, nowMs: number): string | null {
  if (web === 'all') return null;
  const days = web === '7d' ? 7 : web === '30d' ? 30 : 90;
  return new Date(nowMs - days * 24 * 60 * 60 * 1000).toISOString();
}

export function serializeFilter(f: InsightsFilter): string {
  const since = sinceFor(f.web, Date.now());
  const params = new URLSearchParams();
  if (since) params.set('since', since);
  params.set('until', new Date().toISOString());
  if (f.adapter) params.set('adapter', f.adapter);
  if (f.model) params.set('model', f.model);
  if (f.repo) params.set('repo', f.repo);
  const qs = params.toString();
  return qs ? `?${qs}` : '';
}

/**
 * One GET to /api/insights with the current global filter. Errors throw; the
 * caller decides how they surface as designed panels.
 */
export async function fetchInsights(
  filter: InsightsFilter = DEFAULT_FILTER,
): Promise<Insights> {
  const response = await fetch(`/api/insights${serializeFilter(filter)}`, {
    headers: { accept: 'application/json' },
  });
  if (response.status === 404) throw new Error('insights endpoint is not on this build yet');
  if (!response.ok) throw new Error(`insights returned HTTP ${response.status}`);
  return validateShape(await response.json());
}

/**
 * The real endpoint is a parallel lane; until field names settle, a response
 * missing its spine is an error, not a half-rendered guess.
 */
function validateShape(parsed: Partial<Insights>): Insights {
  if (!parsed || !parsed.current || !Array.isArray(parsed.current.kpis)) {
    throw new Error('insights shape did not match the contract');
  }
  return parsed as Insights;
}

export function isRealInsights(data: Insights | null): data is Insights {
  return !!data && Array.isArray(data.current.kpis) && data.current.kpis.length > 0;
}

/* ---------- numbers ---------- */

/** Percent change current vs previous; null says "no base to compare" (prev 0/unknown). */
export function pctDelta(current: number, previous: number | null | undefined): number | null {
  if (previous == null || previous === 0) return null;
  return ((current - previous) / previous) * 100;
}

/** Share of sessions with any verified evidence (test ok or stronger). The thesis. */
export function verifiedRate(win: InsightsWindow): number {
  if (win.claimed === 0) return 0;
  const verified = win.ladder
    .filter((r) => r.outcome === 'test_or_build_passed' || r.outcome === 'commit_observed' || r.outcome === 'ci_or_deployment_verified')
    .reduce((sum, r) => sum + r.sessions, 0);
  return (verified / win.claimed) * 100;
}

/** One line, biggest movers vs the previous window, for the callout strip. */
export function topMovers(cur: InsightsWindow, prev: InsightsWindow | null): string {
  if (!prev) return `${cur.claimed.toLocaleString()} sessions in this window · no previous window to compare yet`;
  const parts: string[] = [];

  const rate = verifiedRate(cur);
  const prevRate = verifiedRate(prev);
  const rateMove = rate - prevRate;
  if (Math.abs(rateMove) >= 0.05) {
    parts.push(
      `verified outcomes ${rateMove > 0 ? 'up' : 'down'} ${Math.abs(rateMove).toFixed(1)} pts (${rate.toFixed(1)}%)`,
    );
  }

  const frac = cur.friction.reduce((s, r) => s + r.sessions, 0);
  const prevFrac = prev.friction.reduce((s, r) => s + r.sessions, 0);
  const fricMove = frac > 0 ? pctDelta(frac, prevFrac || null) : null;
  if (fricMove != null && Math.abs(fricMove) >= 1) {
    parts.push(`friction ${fricMove > 0 ? '+' : '−'}${Math.abs(fricMove).toFixed(1)}%`);
  }

  const zero = { model: '', tb: 0 };
  const biggest = cur.models.reduce((a, m) => (m.tb > a.tb ? m : a), zero);
  const totalTokens = cur.models.reduce((a, m) => a + m.tb, 0);
  if (biggest.model && totalTokens > 0) {
    const share = (biggest.tb / totalTokens) * 100;
    if (share >= 20) parts.push(`${biggest.model} is ${share.toFixed(0)}% of burn`);
  }

  return parts.length > 0 ? parts.join(' · ') : 'steady vs the previous window';
}

/* ---------- models table ---------- */

export type ModelSortKey = 'sessions' | 'tb' | 'cr_perc';
export interface ModelSort {
  key: ModelSortKey;
  dir: 1 | -1;
}
export const DEFAULT_MODEL_SORT: ModelSort = { key: 'tb', dir: -1 };

export function modelSortToggle(prev: ModelSort, clicked: ModelSortKey): ModelSort {
  return prev.key === clicked ? { key: clicked, dir: (prev.dir * -1) as 1 | -1 } : { key: clicked, dir: -1 };
}

/** Pure reduce + pure sort, so the header arrows and the order are both testable. */
export function sortModels(models: Insights['current']['models'], sort: ModelSort) {
  const value = (m: Insights['current']['models'][number]) =>
    sort.key === 'tb' ? m.tb : sort.key === 'sessions' ? m.sessions : m.cr_perc;
  return [...models].sort((a, b) => (value(a) - value(b)) * sort.dir);
}
