import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { Archive } from '../archive/Archive';

/**
 * Archive phase smoke: empty inspector prompt + list chrome render without a
 * live API. fetch is stubbed so useSessions settles as an error/empty rather
 * than hanging the static render (useEffect does not run under
 * renderToStaticMarkup — we only assert the synchronous empty inspector).
 */

describe('deck archive phase', () => {
  it('renders archive chrome and empty-inspector prompt when no session is selected', () => {
    const html = renderToStaticMarkup(
      <Archive sessionId={null} onNavigate={() => {}} />
    );
    expect(html).toContain('data-testid="archive-phase"');
    expect(html).toContain('Archive');
    expect(html).toContain('Select a session on the left');
  });

  it('shows the selected session id in the inspector header once chosen', () => {
    // Without effects the inspector still paints the header shell for a
    // selected id (loading state); assert the id is visible in markup.
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve({ ok: false, status: 404 }))
    );
    const html = renderToStaticMarkup(
      <Archive sessionId="sess_demo_1" onNavigate={() => {}} />
    );
    expect(html).toContain('sess_demo_1');
    vi.unstubAllGlobals();
  });
});
