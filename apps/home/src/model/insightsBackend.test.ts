import { describe, expect, it } from 'vitest';
import {
  DEFAULT_MODEL_SORT,
  sortModels,
  topMovers,
  verifiedRate,
} from './insights';
import {
  applyDimensionFilter,
  hasDimensionFilter,
  isBackendShape,
  mapInsights,
  type RawInsights,
} from './insightsBackend';
import backendFixture from '../mock/insights-backend-fixture.json';

/**
 * The deck's insights mapping, proved against the REAL backend payload shape
 * (the #191 Rust contract: agentworth_storage::insights, GET /api/insights).
 * The fixture's field names are transcribed from the Rust structs; its numbers
 * are synthetic demo data, never lifted from a real machine's index.
 */

const RAW = backendFixture as unknown as RawInsights;

const MAPPED = mapInsights(RAW);

describe('insights backend mapping (#191 payload -> #192 UI types)', () => {
  it('recognizes the backend shape and rejects the deck shape', () => {
    expect(isBackendShape(RAW)).toBe(true);
    expect(isBackendShape({ generated_at: 'x', current: { kpis: [] } })).toBe(false);
    expect(isBackendShape(null)).toBe(false);
  });

  it('hero tile: verified rate is KPI #0 with the % unit, same value the backend computed', () => {
    const hero = MAPPED.current.kpis.find((k) => k.key === 'verified');
    expect(hero).toBeDefined();
    expect(hero?.unit).toBe('%');
    expect(hero?.value).toBe(RAW.verified.share_pct);
    // the hero sparkline comes from series.verified_by_month, not invented
    expect(hero?.series).toEqual([168, 188]);
  });

  it('claimed and the mapped ladder agree with the backend verified share (denominator is the usable population)', () => {
    expect(MAPPED.current.claimed).toBe(RAW.population.usable_sessions);
    expect(verifiedRate(MAPPED.current)).toBeCloseTo(RAW.verified.share_pct, 2);
  });

  it('token fields scale exactly: tb fields are billions, kpi carries the B unit', () => {
    expect(MAPPED.current.models[0].tb).toBeCloseTo(21.3, 6);
    const tokensKpi = MAPPED.current.kpis.find((k) => k.key === 'tokens');
    expect(tokensKpi?.value).toBeCloseTo(94.1, 4);
    expect(tokensKpi?.unit).toBe('B');
    expect(MAPPED.current.by_adapter[0].tokens_tb).toBeCloseTo(57.3, 6);
  });

  it('computes the per-model cache-read share the backend does not ship', () => {
    expect(MAPPED.current.models[0].cr_perc).toBeCloseTo((4750000000 / 21300000000) * 100, 2);
  });

  it('structurally deferred metrics map to empty arrays when the turn blocks are stripped, never zero-filled', () => {
    const stripped: RawInsights = { ...RAW, friction: undefined, day_hour: undefined, vocabulary: undefined };
    const mapped = mapInsights(stripped);
    expect(mapped.current.friction).toEqual([]);
    expect(mapped.current.day_hour).toEqual([]);
    expect(mapped.current.vocabulary).toEqual([]);
  });

  it('synthesizes previous KPIs from the deltas block, so Δ works by key', () => {
    expect(MAPPED.previous).not.toBeNull();
    const verified = MAPPED.previous?.kpis.find((k) => k.key === 'verified');
    expect(verified?.value).toBe(RAW.deltas.verified_outcome_rate.previous);
    const sessions = MAPPED.previous?.kpis.find((k) => k.key === 'sessions');
    expect(sessions?.value).toBe(RAW.deltas.usable_sessions.previous);
    const tokens = MAPPED.previous?.kpis.find((k) => k.key === 'tokens');
    expect(tokens?.value).toBeCloseTo((RAW.deltas.token_burn.previous ?? 0) / 1e9, 4);
  });

  it('an all-time payload (filter.since null) maps to previous: null, honestly single-sided', () => {
    const allTime = { ...RAW, filter: { since: null, until: null }, deltas: { ...RAW.deltas, previous_window: null } };
    const mapped = mapInsights(allTime);
    expect(mapped.previous).toBeNull();
    expect(topMovers(mapped.current, null)).toContain('no previous window to compare');
  });

  it('the callout names the verified move from the deltas pair (mapped previous has no ladder rows)', () => {
    const line = topMovers(MAPPED.current, MAPPED.previous!);
    expect(line).toContain('verified outcomes up 10.8 pts (37.6%)');
  });

  it('model/repo sorting still runs on mapped rows', () => {
    const ids = sortModels(MAPPED.current.models, DEFAULT_MODEL_SORT).map((m) => m.model);
    expect(ids[0]).toBe('model-a');
  });

  it('dimension filters narrow per-row widgets only', () => {
    const f = { adapter: 'claude_code', model: null, repo: null };
    expect(hasDimensionFilter(f)).toBe(true);
    const narrowed = applyDimensionFilter(MAPPED.current, f);
    expect(narrowed.by_adapter.map((a) => a.adapter)).toEqual(['claude_code']);
    expect(narrowed.big_sessions.every((s) => s.adapter === 'claude_code')).toBe(true);
    // aggregates stay population-wide: never narrowed data we cannot derive
    expect(narrowed.claimed).toBe(MAPPED.current.claimed);
    expect(narrowed.ladder).toEqual(MAPPED.current.ladder);

    const repo = applyDimensionFilter(MAPPED.current, { adapter: null, model: null, repo: 'demo/webapp' });
    expect(repo.repos.every((r) => r.repo === 'demo/webapp')).toBe(true);
    expect(repo.models.map((m) => m.model)).toEqual(['model-a', 'model-b']);

    expect(applyDimensionFilter(MAPPED.current, { adapter: null, model: null, repo: null })).toBe(MAPPED.current);
  });

  it('mapper treats dates as the backend writes them: started_at becomes the date', () => {
    expect(MAPPED.current.big_sessions[0].date).toBe('2026-09-11');
    expect(MAPPED.current.big_sessions[0].repo).toBe('demo');
  });

  it('window echo: since falls back to data span, until prefers the echoed filter value', () => {
    expect(MAPPED.since).toBe('2026-08-24T10:47:34+00:00');
    expect(MAPPED.until).toBe('2026-09-23T10:47:34+00:00');
  });
});

describe('insights turn-data lane (human_turns blocks)', () => {
  it('maps filled day_hour / friction / vocabulary rows through to the UI types', () => {
    const filled: RawInsights = {
      ...RAW,
      deferred: RAW.deferred.filter(
        (d) => !['friction_triggers', 'time_of_day_histogram', 'vocabulary_mentions'].includes(d.dimension),
      ),
      day_hour: [{ dow: 4, hour: 15, turns: 12 }, { dow: 0, hour: 1, turns: 3 }],
      friction: [{ trigger: 'loop_interruption', turns: 7 }, { trigger: 'context_amnesia', turns: 2 }],
      vocabulary: [{ term: 'receipts', mentions: 148 }, { term: 'docir', mentions: 19 }],
    };
    const mapped = mapInsights(filled);
    expect(mapped.current.day_hour).toEqual([
      { dow: 4, hour: 15, sessions: 12 },
      { dow: 0, hour: 1, sessions: 3 },
    ]);
    expect(mapped.current.friction).toEqual([
      { trigger: 'loop_interruption', sessions: 7 },
      { trigger: 'context_amnesia', sessions: 2 },
    ]);
    expect(mapped.current.vocabulary).toEqual([
      { term: 'receipts', sessions: 148 },
      { term: 'docir', sessions: 19 },
    ]);
  });

  it('keeps the empty path for a payload before the turn lane: the mock fixture is pre-lane', () => {
    const fixture: RawInsights = JSON.parse(JSON.stringify(backendFixture));
    expect(fixture.day_hour).toBeUndefined();
    expect(fixture.friction).toBeUndefined();
    expect(fixture.vocabulary).toBeUndefined();
  });
});
