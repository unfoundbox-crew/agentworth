import { useCallback, useRef, useState } from 'react';
import { SessionList, type ShellNav } from './shell/SessionList';
import { InspectorPane } from './shell/InspectorPane';
import { Rail, type RailViewId } from './shell/Rail';
import { CoveragePane } from './shell/CoveragePane';
import { ArchaeologyPane } from './shell/ArchaeologyPane';
import { ErrorBoundary } from './components/ErrorBoundary';
import { useArchiveKeys } from './useArchiveKeys';
import './archive-shell.css';
import './panes.css';
import './list.css';
import './inspector.css';
import './trajectory.css';

export interface ArchiveProps {
  /** Session id from `/home/s/<id>` (P1 route). Null = list only. */
  sessionId: string | null;
  /** Navigate to an app-relative path (`/s/<id>` or `/`). */
  onNavigate: (appPath: string) => void;
}

/**
 * Archive phase — session list + inspector inside the deck shell, plus the
 * P3 rail views that used to live only on `apps/dashboard`.
 *
 * P3 (this slice): Coverage + Archaeology are live. Overview / Exports rail
 * buttons are present for parity with the dashboard rail and show an honest
 * placeholder until the next implement PR (FleetStrip/VerdictBoard share and
 * ExportModal). Command palette stays follow-up. Do not delete dashboard;
 * do not flip serve root (see `docs/specs/app-merge-p3-parity.md`).
 */
export function Archive({ sessionId, onNavigate }: ArchiveProps) {
  const [liveTail, setLiveTail] = useState(false);
  const [trajectoryFocused, setTrajectoryFocused] = useState(false);
  const [activeView, setActiveView] = useState<RailViewId>('sessions');
  const navRef = useRef<ShellNav | null>(null);
  const inspectorRegionRef = useRef<HTMLDivElement>(null);

  const exitTrajectoryFocus = useCallback(() => {
    if (!trajectoryFocused) return false;
    setTrajectoryFocused(false);
    return true;
  }, [trajectoryFocused]);

  const focusInspector = useCallback(() => {
    inspectorRegionRef.current?.focus();
  }, []);

  useArchiveKeys({
    exitTrajectoryFocus,
    navRef,
    focusInspector,
  });

  return (
    <div className="shell-root archive-phase enter" data-testid="archive-phase">
      <div className="archive-topbar">
        <span className="archive-topbar-title">Archive</span>
        <div className="archive-topbar-spacer" />
        <button
          type="button"
          className="livetail-btn"
          aria-pressed={liveTail}
          onClick={() => setLiveTail((v) => !v)}
          title="Live tail (stream not yet wired in the deck archive)"
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            appearance: 'none',
            border: '0.5px solid var(--mv-border)',
            borderRadius: 6,
            background: liveTail ? 'var(--mv-accent-soft)' : 'transparent',
            color: 'var(--mv-muted)',
            fontSize: 10,
            padding: '4px 8px',
            cursor: 'pointer',
          }}
        >
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: liveTail ? 'var(--mv-success)' : 'var(--mv-faint)',
            }}
          />
          Live Tail
        </button>
        <span className="archive-topbar-hint">
          <kbd>j</kbd>/<kbd>k</kbd> move · <kbd>/</kbd> filter · <kbd>esc</kbd> leave
        </span>
      </div>

      <div className="shell-body">
        <Rail activeView={activeView} onSelect={setActiveView} />

        {activeView === 'sessions' ? (
          <>
            {!trajectoryFocused && (
              <div className="list-region">
                <ErrorBoundary label="Session list">
                  <SessionList
                    selectedId={sessionId}
                    onSelect={(id) => onNavigate(`/s/${encodeURIComponent(id)}`)}
                    registerNav={(nav) => {
                      navRef.current = nav;
                    }}
                    liveTail={liveTail}
                  />
                </ErrorBoundary>
              </div>
            )}

            <div className="inspector-region" ref={inspectorRegionRef} tabIndex={-1}>
              <ErrorBoundary label="Session inspector">
                <InspectorPane
                  sessionId={sessionId}
                  liveTail={liveTail}
                  trajectoryFocused={trajectoryFocused}
                  onToggleTrajectoryFocus={() => setTrajectoryFocused((v) => !v)}
                />
              </ErrorBoundary>
            </div>
          </>
        ) : activeView === 'coverage' ? (
          <div className="archive-view-fill">
            <ErrorBoundary label="Coverage">
              <CoveragePane />
            </ErrorBoundary>
          </div>
        ) : activeView === 'archaeology' ? (
          <div className="archive-view-fill">
            <ErrorBoundary label="Archaeology">
              <ArchaeologyPane />
            </ErrorBoundary>
          </div>
        ) : (
          <div className="archive-view-fill">
            <div className="view-region">
              <div className="shell-inspector-empty">
                {activeView === 'overview'
                  ? 'Overview (VerdictBoard / FleetStrip) lands in the next P3 implement PR — Coverage and Archaeology are live on this rail.'
                  : 'Exports lands in the next P3 implement PR — pick Coverage or Archaeology on the rail, or return to Sessions.'}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
