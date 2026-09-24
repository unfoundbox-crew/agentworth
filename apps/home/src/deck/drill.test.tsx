import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { DrillDrawer, LadderFunnel } from './Insights';
import { type DrillResult } from '../model/insights';
import { DEFAULT_FILTER, fetchDrill } from '../model/insights';

/**
 * The drill-through surface (#196). The wire shape here is what curl
 * /api/insights/drill returns after the Rust lane landed it — same object the
 * endpoint (InsightsDrill struct) serializes, synthetic data.
 */
const REAL_DRILL: DrillResult = {
  view: 'ladder',
  key: 'commit_observed',
  count: 2,
  rows: [
    {
      session_id: 'sess-a',
      adapter: 'claude_code',
      repo_label: 'demo/webapp',
      started_at: '2026-09-20T10:00:00Z',
      primary_outcome: 'commit_observed',
      total_tokens: 4_300_000_000,
      total_events: 12,
    },
    {
      session_id: 'sess-b',
      adapter: 'codex',
      repo_label: 'demo/engine',
      started_at: '2026-09-18T10:00:00Z',
      primary_outcome: 'commit_observed',
      total_tokens: 1_000,
      total_events: 3,
    },
  ],
};

describe('insights drill-through', () => {
  it('the drill drawer renders the real payload rows, stats-shaped only', () => {
    const html = renderToStaticMarkup(
      <DrillDrawer
        drill={{ view: 'ladder', key: 'commit_observed' }}
        result={{ ...REAL_DRILL, of: 'ladder|commit_observed' }}
        error={null}
        loading={false}
        onClose={() => {}}
      />,
    );
    expect(html).toContain('2 session(s)');
    expect(html).toContain('demo/webapp');
    expect(html).toContain('4.30B tok');
    // no transcript text, no raw paths: never present by construction
    expect(html).not.toContain('.jsonl');
    expect(html).not.toContain('/Users');
  });

  it('an empty cut says so, it never renders a zero lying quietly', () => {
    const html = renderToStaticMarkup(
      <DrillDrawer
        drill={{ view: 'day_hour', key: '2-14' }}
        result={{ view: 'day_hour', key: '2-14', count: 0, rows: [], of: 'day_hour|2-14' }}
        error={null}
        loading={false}
        onClose={() => {}}
      />,
    );
    expect(html).toContain('0 session(s)');
    expect(html).toContain('no sessions on this cut');
  });

  it('fetchDrill asks the real endpoint with the drill key and the active slice', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(REAL_DRILL),
    });
    vi.stubGlobal('fetch', fetchMock);
    const data = await fetchDrill(
      { ...DEFAULT_FILTER, adapter: 'codex' },
      { view: 'day_hour', key: '2-14' },
    );
    expect(data.count).toBe(2);
    const url = String(fetchMock.mock.calls[0][0]);
    for (const part of ['view=day_hour', 'key=2-14', 'adapter=codex']) {
      expect(url).toContain(part);
    }
    vi.unstubAllGlobals();
  });

  it('the funnel rows are genuinely drill-enabled: each rung carries a drill button and the callout names the cut', () => {
    const html = renderToStaticMarkup(
      <LadderFunnel
        win={{
          claimed: 10,
          kpis: [],
          ladder: [{ outcome: 'commit_observed', sessions: 2, tb: 0.1 }],
          friction: [],
          day_hour: [],
          by_adapter: [],
          models: [],
          buckets: [],
          repos: [],
          vocabulary: [],
          big_sessions: [],
        }}
        onDrill={() => {}}
      />,
    );
    expect(html).toContain('drill into commit: 2 sessions');
    expect(html).toContain('rows drill through');
  });
});
