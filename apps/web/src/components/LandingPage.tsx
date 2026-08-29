import React, { useState, useEffect } from 'react';
import { Github, ArrowUpRight, Copy, Check, Terminal, ExternalLink } from 'lucide-react';
import { trackEvent } from '../services/analytics';

interface LandingPageProps {
  onOpenExplorer?: () => void;
}

export const LandingPage: React.FC<LandingPageProps> = ({ onOpenExplorer }) => {
  const [copied, setCopied] = useState(false);
  const [terminalLineIndex, setTerminalLineIndex] = useState(0);

  const terminalLines = [
    { text: 'Scanning ~/.claude...', delay: 400 },
    { text: 'Scanning ~/.cursor...', delay: 800 },
    { text: 'Scanning ~/.gemini...', delay: 1200 },
    { text: 'Scanning ~/.codex...', delay: 1600 },
    { text: '', delay: 1900 },
    { text: 'Found:', delay: 2100, bold: true },
    { text: '8.4B tokens', delay: 2400 },
    { text: '4,281 sessions', delay: 2600 },
    { text: '17 agents', delay: 2800 },
    { text: '1,337 verified outcomes', delay: 3000 },
    { text: '', delay: 3200 },
    { text: 'Useful work:', delay: 3400, bold: true },
    { text: 'unfortunately, some.', delay: 3700, highlight: true },
  ];

  useEffect(() => {
    const interval = setInterval(() => {
      setTerminalLineIndex((prev) => (prev < terminalLines.length ? prev + 1 : prev));
    }, 300);
    return () => clearInterval(interval);
  }, []);

  const handleCopyNpx = () => {
    navigator.clipboard.writeText('npx agentworth');
    setCopied(true);
    trackEvent('npx_command_copied', { source: 'landing_hero' });
    setTimeout(() => setCopied(false), 2000);
  };

  const restartScanDemo = () => {
    setTerminalLineIndex(0);
    trackEvent('terminal_demo_replayed');
  };

  return (
    <div className="min-h-screen bg-[#fdfdfd] text-black font-mono selection:bg-black selection:text-white">
      
      {/* 1. Header Box */}
      <header className="border-b-2 border-black bg-white">
        <div className="max-w-4xl mx-auto px-4 py-8 sm:py-12">
          
          <div className="border-2 border-black bg-white p-6 sm:p-8 shadow-[6px_6px_0px_0px_rgba(0,0,0,1)]">
            <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4">
              <div>
                <h1 className="text-2xl sm:text-4xl font-extrabold tracking-tight uppercase">
                  AGENTWORTH
                </h1>
                <p className="text-sm sm:text-base text-zinc-600 mt-1 font-medium">
                  Your agents left receipts.
                </p>
              </div>

              {/* Top Links */}
              <div className="flex items-center space-x-2">
                <a
                  href="https://github.com/unfoundbox/agentworth"
                  target="_blank"
                  rel="noreferrer"
                  aria-label="View AgentWorth on GitHub"
                  onClick={() => trackEvent('github_clicked', { location: 'header' })}
                  className="px-3 py-1.5 text-xs font-bold border-2 border-black bg-white hover:bg-black hover:text-white transition-colors shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] flex items-center space-x-1"
                >
                  <Github className="w-3.5 h-3.5" />
                  <span>GitHub</span>
                </a>
                <a
                  href="https://github.com/unfoundbox/agentworth#readme"
                  target="_blank"
                  rel="noreferrer"
                  aria-label="Read Documentation on GitHub"
                  onClick={() => trackEvent('docs_clicked', { location: 'header' })}
                  className="px-3 py-1.5 text-xs font-bold border-2 border-black bg-white hover:bg-black hover:text-white transition-colors shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] flex items-center space-x-1"
                >
                  <span>Docs</span>
                  <ArrowUpRight className="w-3 h-3" />
                </a>
              </div>
            </div>

            {/* Quickstart NPX Box */}
            <div className="mt-6 pt-6 border-t-2 border-dashed border-zinc-300 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 bg-zinc-50 p-3.5 border border-zinc-900">
              <div className="flex items-center space-x-2">
                <span className="text-zinc-400 select-none">$</span>
                <code className="text-sm sm:text-base font-bold text-black select-all">
                  npx agentworth
                </code>
              </div>
              <div className="flex items-center space-x-2 w-full sm:w-auto">
                <button
                  onClick={handleCopyNpx}
                  aria-label="Copy npx agentworth command"
                  className="flex-1 sm:flex-none px-3 py-1 text-xs font-bold border border-black bg-white hover:bg-black hover:text-white transition-colors flex items-center justify-center space-x-1"
                >
                  {copied ? (
                    <>
                      <Check className="w-3 h-3 text-black" />
                      <span>Copied!</span>
                    </>
                  ) : (
                    <>
                      <Copy className="w-3 h-3" />
                      <span>Copy</span>
                    </>
                  )}
                </button>
                {onOpenExplorer && (
                  <button
                    onClick={() => {
                      trackEvent('explorer_launched');
                      onOpenExplorer();
                    }}
                    aria-label="Launch live trace explorer"
                    className="flex-1 sm:flex-none px-3 py-1 text-xs font-bold border border-black bg-black text-white hover:bg-zinc-800 transition-colors"
                  >
                    Launch Explorer →
                  </button>
                )}
              </div>
            </div>
          </div>

        </div>
      </header>

      {/* 2. Live Terminal Demo */}
      <section className="py-8 sm:py-12">
        <div className="max-w-4xl mx-auto px-4">
          <div className="text-center mb-3">
            <span className="text-[11px] uppercase tracking-widest text-zinc-500 font-bold">
              ↓ live terminal demo
            </span>
          </div>

          <div className="border-2 border-black bg-black text-zinc-100 p-6 sm:p-8 shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] text-xs sm:text-sm leading-relaxed">
            <div className="flex items-center justify-between border-b border-zinc-800 pb-3 mb-4 text-zinc-500 text-xs">
              <div className="flex items-center space-x-2">
                <Terminal className="w-3.5 h-3.5 text-zinc-400" />
                <span>agentworth scan --offline</span>
              </div>
              <button
                onClick={restartScanDemo}
                className="text-[11px] underline hover:text-white"
              >
                Replay Demo
              </button>
            </div>

            <div className="space-y-1 min-h-[220px]">
              {terminalLines.slice(0, terminalLineIndex).map((line, idx) => (
                <div
                  key={idx}
                  className={`${line.bold ? 'font-bold text-white pt-2' : ''} ${
                    line.highlight ? 'text-emerald-400 font-bold text-sm sm:text-base' : 'text-zinc-300'
                  }`}
                >
                  {line.text}
                </div>
              ))}
              {terminalLineIndex < terminalLines.length && (
                <span className="inline-block w-2 h-4 bg-white animate-pulse"></span>
              )}
            </div>
          </div>
        </div>
      </section>

      {/* Separator */}
      <div className="max-w-4xl mx-auto px-4">
        <div className="border-t-2 border-black"></div>
      </div>

      {/* 3. WHAT IS AGENTWORTH? */}
      <section className="py-12 sm:py-16">
        <div className="max-w-4xl mx-auto px-4">
          <h2 className="text-xl sm:text-2xl font-extrabold uppercase tracking-tight mb-4">
            WHAT IS AGENTWORTH?
          </h2>
          <p className="text-sm sm:text-base text-zinc-700 leading-relaxed mb-6">
            Your coding agents have quietly accumulated <strong>gigabytes of JSONL</strong> in hidden dotfiles across your computer.
          </p>
          <p className="text-xs uppercase tracking-wider text-zinc-500 font-bold mb-4">
            AgentWorth turns that mess into:
          </p>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 border-2 border-black bg-white p-6 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] text-xs sm:text-sm">
            <div className="flex items-center space-x-2">
              <span className="font-bold text-black">✓</span>
              <span>sessions</span>
            </div>
            <div className="flex items-center space-x-2">
              <span className="font-bold text-black">✓</span>
              <span>token usage</span>
            </div>
            <div className="flex items-center space-x-2">
              <span className="font-bold text-black">✓</span>
              <span>failures + recoveries</span>
            </div>
            <div className="flex items-center space-x-2">
              <span className="font-bold text-black">✓</span>
              <span>model switches</span>
            </div>
            <div className="flex items-center space-x-2">
              <span className="font-bold text-black">✓</span>
              <span>tests/builds/diffs</span>
            </div>
            <div className="flex items-center space-x-2">
              <span className="font-bold text-black">✓</span>
              <span>verified outcomes</span>
            </div>
            <div className="flex items-center space-x-2 sm:col-span-2">
              <span className="font-bold text-black">✓</span>
              <span>weird archaeological discoveries</span>
            </div>
          </div>
        </div>
      </section>

      {/* Separator */}
      <div className="max-w-4xl mx-auto px-4">
        <div className="border-t-2 border-black"></div>
      </div>

      {/* 4. YOUR AGENT ARCHAEOLOGY */}
      <section className="py-12 sm:py-16">
        <div className="max-w-4xl mx-auto px-4">
          <div className="inline-block border border-black px-2 py-0.5 bg-zinc-100 font-bold text-[10px] uppercase mb-2">
            EXCAVATED EVIDENCE
          </div>
          <h2 className="text-xl sm:text-2xl font-extrabold uppercase tracking-tight mb-6">
            YOUR AGENT ARCHAEOLOGY
          </h2>

          <div className="border-2 border-black bg-white p-6 sm:p-8 shadow-[6px_6px_0px_0px_rgba(0,0,0,1)]">
            <div className="text-xs uppercase font-bold text-zinc-500 mb-1">
              MOST EXPENSIVE UNSOLVED TASK
            </div>
            <div className="text-lg sm:text-2xl font-extrabold text-black mb-6">
              "center this div"
            </div>

            <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 pt-4 border-t-2 border-dashed border-zinc-300 text-xs sm:text-sm">
              <div>
                <div className="text-zinc-500 uppercase text-[11px] font-bold">Tokens</div>
                <div className="font-extrabold text-base sm:text-lg mt-0.5">18.3M</div>
              </div>
              <div>
                <div className="text-zinc-500 uppercase text-[11px] font-bold">Models</div>
                <div className="font-extrabold text-base sm:text-lg mt-0.5">7</div>
              </div>
              <div>
                <div className="text-zinc-500 uppercase text-[11px] font-bold">Time</div>
                <div className="font-extrabold text-base sm:text-lg mt-0.5">6h 42m</div>
              </div>
              <div>
                <div className="text-zinc-500 uppercase text-[11px] font-bold">Outcome</div>
                <div className="font-extrabold text-base sm:text-lg mt-0.5 uppercase text-black">
                  [unresolved]
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Separator */}
      <div className="max-w-4xl mx-auto px-4">
        <div className="border-t-2 border-black"></div>
      </div>

      {/* 5. LOCAL MEANS LOCAL */}
      <section className="py-12 sm:py-16">
        <div className="max-w-4xl mx-auto px-4">
          <h2 className="text-xl sm:text-2xl font-extrabold uppercase tracking-tight mb-4">
            LOCAL MEANS LOCAL
          </h2>
          <p className="text-sm text-zinc-700 mb-6">
            AgentWorth runs 100% offline. No telemetry, no background network pings, no cloud sync.
          </p>

          <div className="border-2 border-black bg-zinc-950 text-emerald-400 p-6 shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] text-xs sm:text-sm overflow-x-auto select-none">
            <pre className="leading-relaxed">
{`your machine
    ↓
AgentWorth
    ↓
SQLite
    ↓
you

Nothing uploads.`}
            </pre>
          </div>

          <div className="mt-4 text-xs">
            <a
              href="https://github.com/unfoundbox/agentworth"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center space-x-1 font-bold underline hover:text-zinc-600"
            >
              <span>View source →</span>
              <ExternalLink className="w-3 h-3" />
            </a>
          </div>
        </div>
      </section>

      {/* Separator */}
      <div className="max-w-4xl mx-auto px-4">
        <div className="border-t-2 border-black"></div>
      </div>

      {/* 6. SUPPORTED */}
      <section className="py-12 sm:py-16">
        <div className="max-w-4xl mx-auto px-4">
          <h2 className="text-xl sm:text-2xl font-extrabold uppercase tracking-tight mb-4">
            SUPPORTED
          </h2>
          <div className="border-2 border-black bg-white p-6 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)]">
            <p className="text-sm sm:text-base font-bold text-black">
              Claude Code · Codex · Gemini CLI · OpenCode · Cursor · Goose · Pi
            </p>
            <p className="text-xs text-zinc-500 mt-2">
              More via open Rust streaming adapters.
            </p>
          </div>
        </div>
      </section>

      {/* Separator */}
      <div className="max-w-4xl mx-auto px-4">
        <div className="border-t-2 border-black"></div>
      </div>

      {/* 7. OPEN SOURCE */}
      <footer className="py-12 sm:py-20 bg-zinc-50 border-t-2 border-black">
        <div className="max-w-4xl mx-auto px-4 text-center sm:text-left">
          <h2 className="text-xl sm:text-2xl font-extrabold uppercase tracking-tight mb-4">
            OPEN SOURCE
          </h2>
          <p className="text-xs sm:text-sm text-zinc-600 leading-relaxed mb-6">
            Rust core. Local-first. Adapter-driven. No account required.
          </p>

          <div className="flex flex-col sm:flex-row items-center gap-4">
            <button
              onClick={handleCopyNpx}
              className="w-full sm:w-auto px-6 py-3 text-sm font-bold border-2 border-black bg-white hover:bg-black hover:text-white transition-colors shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex items-center justify-center space-x-2"
            >
              <span>$ npx agentworth</span>
              {copied ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
            </button>

            <a
              href="https://github.com/unfoundbox/agentworth"
              target="_blank"
              rel="noreferrer"
              className="w-full sm:w-auto px-6 py-3 text-sm font-bold border-2 border-black bg-black text-white hover:bg-zinc-800 transition-colors shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex items-center justify-center space-x-2"
            >
              <Github className="w-4 h-4" />
              <span>Star on GitHub</span>
            </a>
          </div>
        </div>
      </footer>

    </div>
  );
};
