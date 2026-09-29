import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { dispatch } from '../model/store';
import { plain } from '../model/theme';
import type { Direction, HomeEnv, ServerFrame } from '../protocol';
import { Strips } from './Strips';
import { Seats } from './Seats';
import { Course } from './Course';
import { Dock } from './Dock';
import { PanelState } from './PanelState';
import { ConnectionBar } from './ConnectionBar';

const ENV: HomeEnv = {
  cwd: '/repo',
  repo: '/repo',
  harnesses: [{ id: 'claude', label: 'Claude', bin: 'claude' }],
  herdr: 'ok',
  budgetDefaultTokens: 5_000_000,
};

function hello(): ServerFrame {
  return { t: 'hello', protocol: 2, personas: [], spaces: [], directions: [], env: ENV };
}

const DIR: Direction = {
  id: 'd1',
  goal: 'ship it',
  area: '/repo',
  done: 'test',
  reached: null,
  budgetTokens: 5_000_000,
  spentTokens: 0,
  riders: [],
  state: 'riding',
  exception: null,
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

describe('deck panel states (visual polish lane)', () => {
  it('shared PanelState renders loading with a status role', () => {
    const html = renderToStaticMarkup(<PanelState kind="loading" title="loading the course" />);
    expect(html).toContain('role="status"');
    expect(html).toContain('loading the course');
  });

  it('shared PanelState renders error with an alert role and honest copy', () => {
    const html = renderToStaticMarkup(
      <PanelState kind="error" title="could not reach the fleet" hint="your riders keep working · retry" />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain('could not reach the fleet');
  });

  it('shared PanelState renders empty with honest copy, never a blank hole', () => {
    const html = renderToStaticMarkup(
      <PanelState kind="empty" title="no evidence yet" hint="stops land here when a rider earns a rung" />,
    );
    expect(html.length).toBeGreaterThan(20);
    expect(html).toContain('no evidence yet');
  });

  it('strips shows a designed empty state when there are no directions', () => {
    dispatch({ type: 'frame', frame: hello() });
    const html = renderToStaticMarkup(
      <Strips directions={[]} personas={{}} theme={plain} selectedId={null} onSelect={() => {}} />,
    );
    expect(html).toContain('no directions yet');
  });

  it('strips marks the selected strip so keyboard users know where they are', () => {
    const html = renderToStaticMarkup(
      <Strips directions={[DIR]} personas={{}} theme={plain} selectedId="d1" onSelect={() => {}} />,
    );
    expect(html).toContain('aria-current="true"');
  });

  it('seats shows a designed empty state when no riders are seated', () => {
    const html = renderToStaticMarkup(<Seats personas={[]} theme={plain} mute={new Set()} solo={new Set()} />);
    expect(html).toContain('no riders seated');
  });

  it('seats exposes solo/mute as pressed toggle buttons with labels', () => {
    dispatch({ type: 'frame', frame: hello() });
    const html = renderToStaticMarkup(
      <Seats
        personas={[
          {
            id: 'p1', role: 'executor', kind: 'codex', agentName: 'codex-build',
            paneId: 'w:p', workspaceId: 'w', cwd: '/repo', presence: 'working',
            title: 'builds', revision: 1,
          },
        ]}
        theme={plain}
        mute={new Set(['p1'])}
        solo={new Set()}
      />,
    );
    expect(html).toContain('aria-pressed="true"');
    expect(html).toContain('aria-label=');
  });

  it('course shows a designed empty state when no direction is selected', () => {
    const html = renderToStaticMarkup(
      <Course direction={undefined} stops={{}} artifacts={{}} theme={plain} personas={{}} mute={new Set()} solo={new Set()} />,
    );
    expect(html).toContain('select a direction');
    expect(html.length).toBeGreaterThan(40);
  });

  it('dock disables its input when no direction is selected', () => {
    const html = renderToStaticMarkup(<Dock direction={undefined} personas={[]} theme={plain} ref={{ current: null }} />);
    expect(html).toContain('no direction selected');
    expect(html).toContain('disabled');
  });

  it('dock names its timing toggle as pressed buttons', () => {
    const html = renderToStaticMarkup(<Dock direction={DIR} personas={[]} theme={plain} ref={{ current: null }} />);
    expect(html).toContain('aria-pressed=');
  });

  it('connection bar renders connecting and reconnecting states with live roles', () => {
    const connecting = renderToStaticMarkup(<ConnectionBar connection="connecting" />);
    expect(connecting).toContain('connecting');
    expect(connecting).toContain('role="status"');
    const closed = renderToStaticMarkup(<ConnectionBar connection="closed" />);
    expect(closed).toContain('reconnecting');
    expect(closed).toContain('role="alert"');
    const open = renderToStaticMarkup(<ConnectionBar connection="open" />);
    expect(open).toBe('');
  });
});
