import React from 'react';
import { Database, Github, RefreshCw } from 'lucide-react';

interface NavbarProps {
  onTriggerScan?: () => void;
  isScanning?: boolean;
  viewMode?: 'landing' | 'explorer';
  onToggleView?: (mode: 'landing' | 'explorer') => void;
}

export const Navbar: React.FC<NavbarProps> = ({ onTriggerScan, isScanning, viewMode = 'explorer', onToggleView }) => {
  return (
    <header className="border-b border-zinc-900 bg-[#fdfdfd] sticky top-0 z-40">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-14 flex items-center justify-between">
        {/* Brand */}
        <div className="flex items-center space-x-3 cursor-pointer" onClick={() => onToggleView && onToggleView('landing')}>
          <div className="flex items-center justify-center w-8 h-8 bg-black text-white font-mono text-sm font-bold border border-black shadow-[2px_2px_0px_0px_rgba(0,0,0,1)]">
            AW
          </div>
          <div>
            <div className="flex items-center space-x-2">
              <span className="font-mono font-bold text-sm tracking-wider uppercase">
                AGENTWORTH
              </span>
              <span className="text-[10px] uppercase font-mono px-1.5 py-0.5 bg-zinc-100 border border-zinc-300 text-zinc-700">
                v0.1.0-alpha
              </span>
            </div>
            <p className="text-[11px] text-zinc-500 font-mono hidden sm:block">
              Your agents left receipts.
            </p>
          </div>
        </div>

        {/* Status Indicators & Actions */}
        <div className="flex items-center space-x-2 sm:space-x-3">
          {onToggleView && (
            <div className="flex items-center border border-black text-xs font-mono bg-white shadow-[1px_1px_0px_0px_rgba(0,0,0,1)]">
              <button
                onClick={() => onToggleView('landing')}
                className={`px-2.5 py-1 ${viewMode === 'landing' ? 'bg-black text-white font-bold' : 'hover:bg-zinc-100'}`}
              >
                Landing
              </button>
              <button
                onClick={() => onToggleView('explorer')}
                className={`px-2.5 py-1 ${viewMode === 'explorer' ? 'bg-black text-white font-bold' : 'hover:bg-zinc-100'}`}
              >
                Explorer
              </button>
            </div>
          )}

          <div className="hidden md:flex items-center space-x-1.5 px-2 py-1 bg-zinc-50 border border-zinc-200 text-[11px] font-mono text-zinc-700">
            <Database className="w-3.5 h-3.5 text-zinc-600" />
            <span>~/.agentworth/agentworth.db</span>
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse ml-1" />
          </div>

          {onTriggerScan && viewMode === 'explorer' && (
            <button
              onClick={onTriggerScan}
              disabled={isScanning}
              className="flex items-center space-x-1.5 px-2.5 py-1 text-xs font-mono bg-white hover:bg-zinc-100 border border-zinc-900 active:translate-x-0.5 active:translate-y-0.5 shadow-[1px_1px_0px_0px_rgba(0,0,0,1)] transition-all disabled:opacity-50"
              title="Rescan ~/.claude, ~/.codex, ~/.gemini"
            >
              <RefreshCw className={`w-3 h-3 ${isScanning ? 'animate-spin' : ''}`} />
              <span>{isScanning ? 'Scanning...' : 'Rescan'}</span>
            </button>
          )}

          <a
            href="https://github.com/unfoundbox/agentworth"
            target="_blank"
            rel="noreferrer"
            className="flex items-center space-x-1 px-2.5 py-1 text-xs font-mono bg-black text-white hover:bg-zinc-800 border border-black shadow-[2px_2px_0px_0px_rgba(0,0,0,0.3)] transition-all"
          >
            <Github className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">GitHub</span>
          </a>
        </div>
      </div>
    </header>
  );
};
