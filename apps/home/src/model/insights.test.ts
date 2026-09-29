import { describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_FILTER,
  DEFAULT_MODEL_SORT,
  fetchDrill,
  modelSortToggle,
  pctDelta,
  serializeDrill,
  serializeFilter,
  sinceFor,
  sortModels,
  topMovers,
  verifiedRate,
  type InsightsFilter,
  type InsightsWindow,
} from './insights';

const WIN: InsightsWindow = {
  claimed: 100,
  kpis: [
    { key: 'verified', label: 'verified outcome rate', value: 30, unit: '%' },
    { key: 'sessions', label: 'sessions', value: 482, series: [9, 12, 14, 11, 17] },
  ],
  ladder: [
    { outcome: 'no_outcome_evidence', sessions: 30, tb: 1 },
    { outcome: 'done_claimed', sessions: 10, tb: 0.2 },
    { outcome: 'artifact_changed', sessions: 20, tb: 3 },
    { outcome: 'test_or_build_passed', sessions: 20, tb: 4 },
    { outcome: 'commit_observed', sessions: 12, tb: 38 },
    { outcome: 'ci_or_deployment_verified', sessions: 8, tb: 46 },
  ],
  friction: [{ trigger: 'loop_interruption', sessions: 60 }],
  day_hour: [],
  by_adapter: [{ adapter: 'claude_code', sessions: 90, tokens_tb: 80 }],
  models: [
    { model: 'claude-sonnet-5', sessions: 50, tb: 20, cr_perc: 20, it: 3, ot: 70 },
    { model: 'claude-opus-5', sessions: 20, tb: 70, cr_perc: 26, it: 1, ot: 79 },
  ],
  buckets: [],
  repos: [],
  vocabulary: [],
  big_sessions: [],
};

const PREV: InsightsWindow = {
  ...WIN,
  claimed: 100,
  kpis: [
    { key: 'verified', label: 'verified outcome rate', value: 15, unit: '%' },
    { key: 'sessions', label: 'sessions', value: 400 },
  ],
  ladder: [
    { outcome: 'no_outcome_evidence', sessions: 80, tb: 1 },
    { outcome: 'done_claimed', sessions: 5, tb: 0.2 },
    { outcome: 'artifact_changed', sessions: 5, tb: 3 },
    { outcome: 'test_or_build_passed', sessions: 5, tb: 4 },
    { outcome: 'commit_observed', sessions: 5, tb: 38 },
    { outcome: 'ci_or_deployment_verified', sessions: 5, tb: 46 },
  ],
  friction: [{ trigger: 'loop_interruption', sessions: 100 }],
  models: [
    { model: 'claude-sonnet-5', sessions: 50, tb: 80, cr_perc: 20, it: 3, ot: 70 },
    { model: 'claude-opus-5', sessions: 20, tb: 10, cr_perc: 26, it: 1, ot: 79 },
  ],
};

describe('insights contract math (deck-insights lane)', () => {
  it('pct Delta math', () => {
    expect(pctDelta(110, 100)).toBeCloseTo(10, 6);
    expect(pctDelta(50, 100)).toBeCloseTo(-50, 6);
    expect(pctDelta(10, 0)).toBeNull();
    expect(pctDelta(10, null)).toBeNull();
  });

  it('window preset math', () => {
    const now = Date.UTC(2026, 8, 23, 12, 0, 0);
    expect(sinceFor('all', now)).toBeNull();
    expect(sinceFor('7d', now)).toBe(new Date(now - 7 * 86400000).toISOString());
    expect(sinceFor('90d', now)).toBe(new Date(now - 90 * 86400000).toISOString());
  });

  it('default filter is 30d, no dimension filters', () => {
    expect(DEFAULT_FILTER).toEqual({ web: '30d', adapter: null, model: null, repo: null });
  });

  it('verified rate counts test-or-stronger over claimed', () => {
    // test+commit+ci = 20+12+8 = 40 of 100 claimed
    expect(verifiedRate(WIN)).toBeCloseTo(40, 6);
    expect(verifiedRate(PREV)).toBeCloseTo(15, 6);
    expect(verifiedRate({ ...WIN, claimed: 0 })).toBe(0);
  });

  it('topMovers names verified-outcome moves, friction and the heaviest model', () => {
    const line = topMovers(WIN, PREV);
    // rate goes 15 -> 40, friction 100 -> 60 = -40%, cur burn: opus 70 of 90
    expect(line).toContain('verified outcomes up 25.0 pts');
    expect(line).toContain('friction −40.0%');
    expect(line).toContain('claude-opus-5 is 78% of burn');
  });

  it('topMovers stays honest with no previous window', () => {
    expect(topMovers(WIN, null)).toContain('no previous window to compare');
  });

  it('models sort by any column and toggle direction', () => {
    expect(sortModels(WIN.models, DEFAULT_MODEL_SORT).map((m) => m.model)).toEqual(['claude-opus-5', 'claude-sonnet-5']);
    const flipped = modelSortToggle(DEFAULT_MODEL_SORT, 'tb');
    expect(sortModels(WIN.models, flipped).map((m) => m.model)).toEqual(['claude-sonnet-5', 'claude-opus-5']);
    expect(sortModels(WIN.models, modelSortToggle(flipped, 'sessions'))[0].sessions).toBe(50);
  });
});

describe('slice-and-drill lane: serialize + drill scraper', () => {
  it('serializeFilter sends adapter/model/repo as real server params alongside the window', () => {
    const f: InsightsFilter = { web: '7d', adapter: 'claude_code', model: 'model-a', repo: 'demo/webapp' };
    const qs = serializeFilter(f);
    expect(qs).toContain('adapter=claude_code');
    expect(qs).toContain('model=model-a');
    expect(qs).toContain('repo=demo%2Fwebapp');
    expect(qs).toContain('since=');
  });

  it('serializeFilter with no dimensions sends no filter keys (contract unchanged)', () => {
    const qs = serializeFilter(DEFAULT_FILTER);
    expect(qs).not.toContain('adapter');
    expect(qs).not.toContain('model=');
    expect(qs).not.toContain('repo=');
  });

  it('serializeDrill carries the same slice plus view/key, no window loss', () => {
    const qs = serializeDrill(
      { web: '7d', adapter: 'codex', model: null, repo: null },
      { view: 'ladder', key: 'commit_observed' },
    );
    expect(qs).toContain('view=ladder');
    expect(qs).toContain('key=commit_observed');
    expect(qs).toContain('adapter=codex');
    expect(qs).toContain('since=');
  });

  it('fetchDrill rejects a payload without its spine instead of guessing', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({ count: 1 }) });
    vi.stubGlobal('fetch', fetchMock);
    await expect(
      fetchDrill(DEFAULT_FILTER, { view: 'ladder', key: 'commit_observed' }),
    ).rejects.toThrow('did not match the contract');
    // The URL is what the backend route expects.
    expect(String(fetchMock.mock.calls[0][0])).toMatch('/api/insights/drill?');
    expect(String(fetchMock.mock.calls[0][0])).toContain('view=ladder&key=commit_observed');
    vi.unstubAllGlobals();
  });
});
