import { useCallback, useEffect, useRef, useState } from 'react';
import { useRoute } from '../hooks/useRoute';
import { useShellKeys } from '../hooks/useShellKeys';
import type { ShellNav } from '../hooks/useShellKeys';
import { Rail } from './Rail';
import type { RailViewId } from './Rail';
import { CommandPalette } from './CommandPalette';
import { ThemeToggle } from '@ui/ThemeToggle';
import { SessionList } from './SessionList';
import { InspectorPane } from './InspectorPane';
import { OverviewPane } from './OverviewPane';
import { CoveragePane } from './CoveragePane';
import { ArchaeologyPane } from './ArchaeologyPane';
import { ExportsPane } from './ExportsPane';
import './shell.css';
import './panes.css';

const TOAST_DURATION_MS = 1800;

/**
 * Three-pane keyboard-first application shell (rail / session list /
 * inspector), replacing the old scrolling dashboard. Owns routing, the
 * global key handler, live-tail state, the command palette frame, and
 * which rail view is active. SessionList, InspectorPane and the other
 * rail-view panes render their own contents.
 */
export function ExplorerShell() {
  const { sessionId, navigate } = useRoute();
  const [liveTail, setLiveTail] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const [activeView, setActiveView] = useState<RailViewId>('sessions');

  const navRef = useRef<ShellNav | null>(null);
  const inspectorRegionRef = useRef<HTMLDivElement>(null);
  const toastTimerRef = useRef<number | null>(null);

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

  const openPalette = useCallback(() => setPaletteOpen(true), []);
  const closePalette = useCallback(() => setPaletteOpen(false), []);
  const focusInspector = useCallback(() => {
    inspectorRegionRef.current?.focus();
  }, []);

  useShellKeys({
    navRef,
    paletteOpen,
    openPalette,
    closePalette,
    focusInspector,
  });

  const toggleLiveTail = useCallback(() => setLiveTail((v) => !v), []);

  return (
    <div className="shell-root">
      <header className="topbar">
        <div className="brand">
          <svg
            width="18"
            height="18"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="M3 14 L7 6 L10 12 L13 4 L17 14" />
          </svg>
          AgentWorth
        </div>
        <div className="topbar-spacer" />
        <button
          type="button"
          className="livetail-btn"
          aria-pressed={liveTail}
          onClick={toggleLiveTail}
        >
          <span className="livetail-dot" aria-hidden="true" />
          Live Tail
        </button>
        <ThemeToggle />
        <button type="button" className="kbd-chip" onClick={openPalette} title="Open command palette">
          &#8984;K
        </button>
      </header>

      <div className="shell-body">
        <Rail activeView={activeView} onSelect={setActiveView} />

        {activeView === 'sessions' ? (
          <>
            <div className="list-region">
              <SessionList
                selectedId={sessionId}
                onSelect={(id: string) => navigate(`/s/${encodeURIComponent(id)}`)}
                registerNav={(nav: ShellNav) => {
                  navRef.current = nav;
                }}
                liveTail={liveTail}
              />
            </div>

            <div className="inspector-region" ref={inspectorRegionRef} tabIndex={-1}>
              <InspectorPane sessionId={sessionId} liveTail={liveTail} />
            </div>
          </>
        ) : activeView === 'overview' ? (
          <OverviewPane />
        ) : activeView === 'coverage' ? (
          <CoveragePane />
        ) : activeView === 'archaeology' ? (
          <ArchaeologyPane />
        ) : (
          <ExportsPane sessionId={sessionId} />
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
