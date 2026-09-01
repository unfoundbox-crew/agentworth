import React from 'react';
import { ThemeToggle } from './ThemeToggle';
import { IconDatabase, IconGithub, IconRefresh } from './icons';
import { APP_VERSION } from '../version';

interface NavbarProps {
  onTriggerScan?: () => void;
  isScanning?: boolean;
  viewMode?: 'landing' | 'explorer';
  onToggleView?: (mode: 'landing' | 'explorer') => void;
}

export const Navbar: React.FC<NavbarProps> = ({ onTriggerScan, isScanning, viewMode = 'explorer', onToggleView }) => {
  return (
    <header className="topbar">
      <div
        className="flex items-center gap-3 cursor-pointer"
        onClick={() => onToggleView && onToggleView('landing')}
      >
        <span className="wordmark">
          <span className="dot" />
          AgentWorth
          <span className="sub">/ {viewMode === 'landing' ? 'Landing' : 'Explorer'}</span>
        </span>
        <span className="hidden sm:inline font-mono text-[10px] px-1.5 py-0.5 rounded border border-border text-muted">
          v{APP_VERSION}
        </span>
      </div>

      <div className="flex items-center gap-2 sm:gap-3">
        {onToggleView && (
          <div className="theme-toggle" role="group" aria-label="View">
            <button
              type="button"
              aria-pressed={viewMode === 'landing'}
              onClick={() => onToggleView('landing')}
            >
              Landing
            </button>
            <button
              type="button"
              aria-pressed={viewMode === 'explorer'}
              onClick={() => onToggleView('explorer')}
            >
              Explorer
            </button>
          </div>
        )}

        <div className="hidden md:flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-border bg-surface text-[11px] font-mono text-muted">
          <IconDatabase size={14} className="text-faint" />
          <span>~/.agentworth/agentworth.db</span>
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 ml-1" aria-hidden="true" />
        </div>

        {onTriggerScan && viewMode === 'explorer' && (
          <button
            onClick={onTriggerScan}
            disabled={isScanning}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-mono border border-border bg-ground text-ink hover:border-accent-border hover:text-accent transition-colors disabled:opacity-50"
            title="Rescan ~/.claude, ~/.codex, ~/.gemini"
          >
            <IconRefresh size={13} spinning={isScanning} />
            <span>{isScanning ? 'Scanning…' : 'Rescan'}</span>
          </button>
        )}

        <a
          href="https://github.com/unfoundbox-crew/agentworth"
          target="_blank"
          rel="noreferrer"
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-mono bg-ink text-ground hover:opacity-85 transition-opacity"
        >
          <IconGithub size={14} />
          <span className="hidden sm:inline">GitHub</span>
        </a>

        <div className="h-4 w-px bg-border" />
        <ThemeToggle />
      </div>
    </header>
  );
};
