import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { DEFAULT_FILTER, type Insights, type InsightsWindow } from '../model/insights';
import { InsightsBody, ExploreDrawer } from './Insights';

/**
 * The insights dashboard, downstream of a mock /api/insights payload
 * (insights-fixture.json shapes it). Renders through renderToStaticMarkup
 * like every other deck test — no DOM needed. Names and numbers are
 * synthetic: demo-* repos, plausible magnitudes, nothing lifted from a
 * machine's real index.
 */

const WIN: InsightsWindow = {
  claimed: 482,
  kpis: [
    { key: 'verified', label: 'verified outcome rate', value: 31.4, unit: '%' },
    { key: 'sessions', label: 'sessions', value: 482, series: [9, 12, 14, 11, 17, 15, 13] },
    { key: 'events', label: 'events', value: 31422 },
    { key: 'turns', label: 'turns', value: 5873 },
    { key: 'tokens', label: 'tokens', value: 94.1, unit: 'B' },
    { key: 'friction', label: 'friction rate', value: 18.6, unit: '%' },
  ],
  ladder: [
    { outcome: 'no_outcome_evidence', sessions: 161, tb: 1.8 },
    { outcome: 'done_claimed', sessions: 41, tb: 0.4 },
    { outcome: 'artifact_changed', sessions: 88, tb: 3.1 },
    { outcome: 'test_or_build_passed', sessions: 52, tb: 4.2 },
    { outcome: 'commit_observed', sessions: 107, tb: 38.6 },
    { outcome: 'ci_or_deployment_verified', sessions: 33, tb: 46 },
  ],
  friction: [{ trigger: 'loop_interruption', sessions: 61 }],
  day_hour: [{ dow: 0, hour: 10, sessions: 12 }],
  by_adapter: [{ adapter: 'claude_code', sessions: 268, tokens_tb: 57.3 }],
  models: [{ model: 'claude-sonnet-5', sessions: 190, tb: 21.3, cr_perc: 22.4, it: 3.1, ot: 71.2 }],
  buckets: [{ label: '< 10M', sessions: 214 }],
  repos: [{ repo: 'demo/webapp', sessions: 88, events: 6211 }],
  vocabulary: [{ term: 'receipt', sessions: 148 }],
  big_sessions: [{ adapter: 'claude_code', date: '2026-09-11', tokens: 517143256, events: 6109, repo: 'demo/webapp' }],
};

const DATA: Insights = {
  generated_at: '2026-09-23T10:47:34+00:00',
  since: '2026-08-24T10:47:34+00:00',
  until: '2026-09-23T10:47:34+00:00',
  current: WIN,
  previous: { ...WIN, claimed: 404, kpis: [{ key: 'verified', label: 'verified outcome rate', value: 26.8, unit: '%' }, ...WIN.kpis.slice(1).map((k) => ({ ...k, value: k.value * 0.9 }))] },
};

function body(overrides: Partial<Parameters<typeof InsightsBody>[0]> = {}) {
  return (
    <InsightsBody
      data={DATA}
      filter={DEFAULT_FILTER}
      onFilter={() => {}}
      detailsOpen={false}
      onToggleDetails={() => {}}
      drawerOpen={false}
      onToggleDrawer={() => {}}
      {...overrides}
    />
  );
}

describe('deck insights (default view, mock render)', () => {
  it('hero KPI leads: verified outcome rate, thesis in one tile', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('verified outcome rate');
    expect(html).toContain('hero · product thesis');
    expect(html).toContain('31.4');
  });

  it('delta against the previous window renders next to the value', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('vs previous window');
  });

  it('primary row: evidence-ladder funnel + heatmap axes are present', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('evidence ladder');
    expect(html).toContain('when this machine works');
    expect(html).toContain('hours 00-23 (utc)');
  });

  it('every number gets a so-what line', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('rungs are evidence, not success');
    expect(html).toContain('title=');
  });

  it('secondary widgets stay collapsed by default, drawer hidden', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('more details +');
    expect(html).not.toContain('friction by trigger');
    expect(html).not.toContain('models · sortable');
    expect(html).not.toContain('top repos');
  });

  it('more-details reveals the secondary row', () => {
    const html = renderToStaticMarkup(body({ detailsOpen: true, onToggleDetails: () => {}, drawerOpen: false, onToggleDrawer: () => {} }));
    expect(html).toContain('models · sortable');
    expect(html).toContain('friction by trigger');
    expect(html).not.toContain('top repos');
  });

  it('explore drawer toggles open inside details', () => {
    const drawer = renderToStaticMarkup(<ExploreDrawer win={WIN} open={false} onToggle={() => {}} />);
    expect(drawer).toContain('explore · deep rows');
    const open = renderToStaticMarkup(<ExploreDrawer win={WIN} open onToggle={() => {}} />);
    expect(open).toContain('top repos');
    expect(open).toContain('biggest sessions');
    expect(open).toContain('repeated vocabulary');
  });
});

describe('deck insights (empty + thin states)', () => {
  it('an empty window says so instead of drawing zero tiles', () => {
    const emptyData: Insights = {
      ...DATA,
      current: { ...WIN, claimed: 0, kpis: [], ladder: [], friction: [], day_hour: [] },
      previous: null,
    };
    const html = renderToStaticMarkup(
      <InsightsBody
        data={emptyData}
        filter={{ ...DEFAULT_FILTER, adapter: 'codex' }}
        onFilter={() => {}}
        detailsOpen={false}
        onToggleDetails={() => {}}
        drawerOpen={false}
        onToggleDrawer={() => {}}
      />,
    );
    expect(html).toContain('no numbers in this window');
    expect(html).not.toContain('hero · product thesis');
  });

  it('a missing sparkline series does not fake one', () => {
    const thin: Insights = {
      ...DATA,
      current: {
        ...WIN,
        kpis: [{ key: 'sessions', label: 'sessions', value: 5 }],
      },
    };
    const html = renderToStaticMarkup(
      <InsightsBody
        data={thin}
        filter={DEFAULT_FILTER}
        onFilter={() => {}}
        detailsOpen={false}
        onToggleDetails={() => {}}
        drawerOpen={false}
        onToggleDrawer={() => {}}
      />,
    );
    expect(html).toContain('sessions');
    expect(html).not.toContain('<polyline');
  });
});
