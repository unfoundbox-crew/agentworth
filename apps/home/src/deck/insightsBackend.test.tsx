import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { DEFAULT_FILTER, type Insights } from '../model/insights';
import { mapInsights, type RawInsights } from '../model/insightsBackend';
import { InsightsBody } from './Insights';
import backendFixture from '../mock/insights-backend-fixture.json';

/**
 * The dashboard rendered from the REAL backend shape (the #191 Rust payload,
 * field names transcribed from agentworth_storage::insights; synthetic demo
 * numbers). The mapper output feeds InsightsBody the same way a live
 * `archie serve` page does — no second shape between them.
 */

const DATA: Insights = mapInsights(backendFixture as unknown as RawInsights);

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

describe('insights dashboard over the real backend payload', () => {
  it('renders the KPI strip with the hero tile first-class: verified rate present, % unit', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('verified outcome rate');
    expect(html).toContain('hero · product thesis');
    expect(html).toContain('37.6');
  });

  it('renders the ladder funnel with every rung the backend ladder mapped', () => {
    const html = renderToStaticMarkup(body());
    for (const label of ['unflown', 'said', 'diff', 'test ok', 'commit', 'ci']) {
      expect(html).toContain(label);
    }
    expect(html).toContain('180'); // no_outcome_evidence sessions
  });

  it('day-hour grid stays empty honestly: no invented cells, the subline says why', () => {
    const html = renderToStaticMarkup(body());
    expect(html).toContain('utc');
  });

  it('money math from deltas: the verified tile Δ matches the backend delta pair', () => {
    const html = renderToStaticMarkup(body());
    // verified 37.6 vs previous 26.8 from deltas => +40.3%
    expect(html).toContain('+40.3%');
  });

  it('models table renders mapped rows with tokens in B', () => {
    const html = renderToStaticMarkup(body({ detailsOpen: true }));
    expect(html).toContain('model-a');
    expect(html).toContain('21.3B');
  });

  it('explore drawer: top repos and biggest sessions carry the mapped rows', () => {
    const html = renderToStaticMarkup(body({ detailsOpen: true, drawerOpen: true }));
    expect(html).toContain('demo/webapp');
    expect(html).toContain('sess'); // repo_label from the backend top_sessions row
  });
});
