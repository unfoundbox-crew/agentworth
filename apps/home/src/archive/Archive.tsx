import { useCallback, useEffect, useRef, useState } from 'react';
import { SessionList, type ShellNav } from './shell/SessionList';
import { InspectorPane } from './shell/InspectorPane';
import { Rail, type RailViewId } from './shell/Rail';
import { CoveragePane } from './shell/CoveragePane';
import { ArchaeologyPane } from './shell/ArchaeologyPane';
import { OverviewPane } from './shell/OverviewPane';
import { ExportsPane } from './shell/ExportsPane';
import { CommandPalette } from './shell/CommandPalette';
import { ErrorBoundary } from './components/ErrorBoundary';
import { useArchiveKeys } from './useArchiveKeys';
import './archive-shell.css';
import './panes.css';
import './widgets.css';
import './list.css';
import './inspector.css';
import './trajectory.css';

export interface ArchiveProps {
  /** Session id from `/home/s/<id>` (P1 route). Null = list only. */
  sessionId: string | null;
  /** Navigate to an app-relative path (`/s/<id>` or `/`). */
  onNavigate: (appPath: string) => void;
}

const TOAST_DURATION_MS = 1800;

/**
 * Archive phase — session list + inspector inside the deck shell, plus the
 * P3 rail views (Overview / Coverage / Archaeology / Exports) and command
 * palette. Shared twins live in `packages/shell` (TrajectoryScrubber,
 * OutcomeLadder). Do not delete `apps/dashboard`; do not flip serve root
 * (see `docs/specs/app-merge-p3-parity.md`).
 */
export function Archive({ sessionId, onNavigate }: ArchiveProps) {
  const [liveTail, setLiveTail] = useState(false);
  const [trajectoryFocused, setTrajectoryFocused] = useState(false);
  const [activeView, setActiveView] = useState<RailViewId>('sessions');
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const navRef = useRef<ShellNav | null>(null);
  const inspectorRegionRef = useRef<HTMLDivElement>(null);
  const toastTimerRef = useRef<number | null>(null);

  const exitTrajectoryFocus = useCallback(() => {
    if (!trajectoryFocused) return false;
    setTrajectoryFocused(false);
    return true;
  }, [trajectoryFocused]);

  const focusInspector = useCallback(() => {
    inspectorRegionRef.current?.focus();
  }, []);

  const openPalette = useCallback(() => setPaletteOpen(true), []);
  const closePalette = useCallback(() => setPaletteOpen(false), []);
  const toggleLiveTail = useCallback(() => setLiveTail((v) => !v), []);

  const showToast = useCallback((message: string) => {
    setToastMessage(message);
    if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setToastMessage(null), TOAST_DURATION_MS);
  }, []);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current);
    };
  }, []);

  useArchiveKeys({
    exitTrajectoryFocus,
    navRef,
    focusInspector,
    paletteOpen,
    openPalette,
    closePalette,
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
          onClick={toggleLiveTail}
          title="Live tail (SSE follow-up — dashboard #213 is on main; archive wiring next)"
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
        <button
          type="button"
          className="kbd-chip"
          onClick={openPalette}
          title="Open command palette"
          style={{
            appearance: 'none',
            border: '0.5px solid var(--mv-border)',
            borderRadius: 6,
            background: 'transparent',
            color: 'var(--mv-muted)',
            fontSize: 10,
            padding: '4px 8px',
            cursor: 'pointer',
            fontFamily: 'var(--font-mono)',
          }}
        >
          ⌘K
        </button>
        <span className="archive-topbar-hint">
          <kbd>j</kbd>/<kbd>k</kbd> move · <kbd>/</kbd> filter · <kbd>⌘K</kbd> palette · <kbd>esc</kbd> leave
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
        ) : activeView === 'overview' ? (
          <div className="archive-view-fill">
            <ErrorBoundary label="Overview">
              <OverviewPane
                onOpenSession={(id) => {
                  setActiveView('sessions');
                  onNavigate(`/s/${encodeURIComponent(id)}`);
                }}
              />
            </ErrorBoundary>
          </div>
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
            <ErrorBoundary label="Exports">
              <ExportsPane sessionId={sessionId} />
            </ErrorBoundary>
          </div>
        )}
      </div>

      <CommandPalette
        open={paletteOpen}
        onClose={closePalette}
        liveTail={liveTail}
        onToggleLiveTail={toggleLiveTail}
        sessionId={sessionId}
        showToast={showToast}
        onNavigateView={setActiveView}
      />

      <div className={`toast${toastMessage ? ' show' : ''}`} role="status" aria-live="polite">
        {toastMessage}
      </div>
    </div>
  );
}
