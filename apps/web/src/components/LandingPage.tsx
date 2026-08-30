import React, { useState, useEffect } from 'react';
import { Github, Copy, Check, CheckCircle2, Plus, ExternalLink, BookOpen } from 'lucide-react';
import { trackEvent } from '../services/analytics';
import { getAgentLogo } from './AgentLogos';
import { ThemeToggle } from './ThemeToggle';

interface LandingPageProps {
  onOpenExplorer?: () => void;
}

export const LandingPage: React.FC<LandingPageProps> = () => {
  const [activeTab, setActiveTab] = useState<'npx' | 'cargo' | 'brew' | 'source'>('npx');
  const [copied, setCopied] = useState(false);
  const [activeStep, setActiveStep] = useState(0);

  const installCommands = {
    npx: 'npx agentworth',
    cargo: 'cargo install agentworth',
    brew: 'brew install unfoundbox/tap/agentworth',
    source: 'git clone https://github.com/unfoundbox/agentworth && cd agentworth && cargo build --release',
  };

  const terminalSteps = [
    { label: '$ agentworth scan', output: 'Scanning machine dotfiles for AI coding agent trajectories...' },
    { label: '[1/11] ~/.claude/projects', output: '✓ 142 sessions found · 4.8M tokens · 89 verified outcomes' },
    { label: '[2/11] ~/.cursor/workspaceStorage', output: '✓ 289 sessions found · 6.2M tokens · 124 diffs analyzed' },
    { label: '[3/11] ~/.gemini/antigravity', output: '✓ 84 sessions found · 1.8M tokens · 25 recovery loops' },
    { label: '[4/11] ~/.codex/sessions', output: '✓ 32 sessions found · 620K tokens · 18 git commits' },
    { label: 'Summary', output: 'Indexed 547 sessions in 0.38s → Local index: ~/.agentworth/index.db (SQLite WAL)' },
  ];

  useEffect(() => {
    const timer = setInterval(() => {
      setActiveStep((prev) => (prev < terminalSteps.length - 1 ? prev + 1 : prev));
    }, 600);
    return () => clearInterval(timer);
  }, []);

  const handleCopy = (cmd: string) => {
    navigator.clipboard.writeText(cmd);
    setCopied(true);
    trackEvent('npx_command_copied', { command: cmd });
    setTimeout(() => setCopied(false), 2000);
  };

  const adapters = [
    { name: 'Claude Code', id: 'claude_code', path: '~/.claude/projects/' },
    { name: 'Cursor Composer', id: 'cursor', path: '~/.cursor/User/workspaceStorage/' },
    { name: 'Google Antigravity', id: 'antigravity', path: '~/.gemini/antigravity/' },
    { name: 'OpenAI Codex', id: 'codex', path: '~/.codex/sessions/' },
    { name: 'Block Goose', id: 'goose', path: '~/.config/goose/sessions/' },
    { name: 'OpenCode', id: 'opencode', path: '~/.opencode/' },
    { name: 'Nous Hermes', id: 'hermes', path: '~/.hermes/sessions/' },
    { name: 'xAI Grok', id: 'grok', path: '~/.grok/sessions/' },
    { name: 'Pi Task Agent', id: 'pi', path: '~/.pi/tasks/' },
    { name: 'OpenClaw', id: 'openclaw', path: '~/.openclaw/' },
    { name: 'Herdr Orchestrator', id: 'herdr', path: '~/.config/herdr/' },
  ];

  return (
    <div className="min-h-screen bg-[#ffffff] dark:bg-[#0a0a0c] text-[#111111] dark:text-[#ececed] font-sans antialiased selection:bg-neutral-900 selection:text-white dark:selection:bg-white dark:selection:text-black transition-colors duration-200">
      
      {/* Top Navbar */}
      <nav className="sticky top-0 z-50 bg-white/90 dark:bg-[#0a0a0c]/90 backdrop-blur-md border-b border-neutral-200 dark:border-neutral-800 transition-colors">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 h-16 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <div className="w-6 h-6 bg-black dark:bg-white rounded flex items-center justify-center text-white dark:text-black font-mono text-xs font-bold shadow-xs">
              aw
            </div>
            <span className="font-sans font-bold tracking-tight text-[16px] text-neutral-950 dark:text-white">
              AgentWorth
            </span>
            <span className="font-mono text-[10px] px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-900 text-neutral-500 dark:text-neutral-400 border border-neutral-200 dark:border-neutral-800">
              v0.1.0
            </span>
          </div>

          <div className="flex items-center gap-1.5 sm:gap-2">
            <a
              href="https://github.com/unfoundbox/agentworth"
              target="_blank"
              rel="noreferrer"
              title="GitHub Repository"
              className="p-1.5 rounded-md text-neutral-600 hover:text-black dark:text-neutral-400 dark:hover:text-white hover:bg-neutral-100 dark:hover:bg-neutral-900 transition flex items-center justify-center"
              aria-label="GitHub Repository"
            >
              <Github className="w-4 h-4" />
            </a>
            <a
              href="https://github.com/unfoundbox/agentworth#readme"
              target="_blank"
              rel="noreferrer"
              title="Documentation"
              className="p-1.5 rounded-md text-neutral-600 hover:text-black dark:text-neutral-400 dark:hover:text-white hover:bg-neutral-100 dark:hover:bg-neutral-900 transition flex items-center justify-center"
              aria-label="Documentation"
            >
              <BookOpen className="w-4 h-4" />
            </a>
            <div className="h-4 w-[1px] bg-neutral-200 dark:bg-neutral-800 mx-1" />
            <ThemeToggle />
          </div>
        </div>
      </nav>

      {/* Hero Section */}
      <main className="max-w-5xl mx-auto px-4 sm:px-6 pt-12 sm:pt-20 pb-16">
        
        {/* Hero Announcement Badge */}
        <div className="flex justify-center mb-6">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 text-xs font-mono text-neutral-700 dark:text-neutral-300">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
            <span>Local-first native tool for AI coding agent trajectories</span>
          </div>
        </div>

        {/* Hero Headlines */}
        <div className="text-center max-w-3xl mx-auto mb-10">
          <h1 className="text-3xl sm:text-5xl sm:leading-[1.15] font-extrabold tracking-tight text-neutral-950 dark:text-white mb-5">
            Your AI coding agents left receipts.
          </h1>
          <p className="text-base sm:text-lg text-neutral-600 dark:text-neutral-400 leading-relaxed font-normal max-w-2xl mx-auto">
            Discover, normalize, and understand the trajectories sitting in your computer's dotfiles.
            Turn gigabytes of messy JSONL into verified outcomes, token accounting, and recovery timelines.
          </p>
        </div>

        {/* Quickstart Tabs Box */}
        <div className="max-w-xl mx-auto mb-10">
          <div className="border border-neutral-300 dark:border-neutral-800 rounded-lg overflow-hidden bg-white dark:bg-[#121215] shadow-sm">
            
            {/* Tabs Header */}
            <div className="flex items-center border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/60 px-2 pt-2">
              {(['npx', 'cargo', 'brew', 'source'] as const).map((tab) => (
                <button
                  key={tab}
                  onClick={() => setActiveTab(tab)}
                  className={`px-4 py-2 font-mono text-xs font-medium rounded-t border-t border-l border-r -mb-[1px] transition ${
                    activeTab === tab
                      ? 'bg-white dark:bg-[#121215] border-neutral-300 dark:border-neutral-700 text-black dark:text-white shadow-sm'
                      : 'border-transparent text-neutral-500 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white'
                  }`}
                >
                  {tab}
                </button>
              ))}
            </div>

            {/* Tab Command & Copy */}
            <div className="p-4 flex items-center justify-between gap-3 bg-white dark:bg-[#121215]">
              <code className="font-mono text-xs sm:text-sm text-neutral-900 dark:text-neutral-100 overflow-x-auto select-all">
                {installCommands[activeTab]}
              </code>
              <button
                onClick={() => handleCopy(installCommands[activeTab])}
                className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded border border-neutral-300 dark:border-neutral-700 hover:border-neutral-400 dark:hover:border-neutral-600 bg-neutral-50 dark:bg-neutral-900 font-mono text-xs font-medium transition text-neutral-700 dark:text-neutral-300 active:scale-95"
                aria-label="Copy installation command"
              >
                {copied ? (
                  <>
                    <Check className="w-3.5 h-3.5 text-emerald-600 dark:text-emerald-400" />
                    <span className="text-emerald-700 dark:text-emerald-400">Copied</span>
                  </>
                ) : (
                  <>
                    <Copy className="w-3.5 h-3.5 text-neutral-500 dark:text-neutral-400" />
                    <span>Copy</span>
                  </>
                )}
              </button>
            </div>
          </div>

          <div className="flex items-center justify-between mt-3 px-1 text-[11px] font-mono text-neutral-500 dark:text-neutral-400">
            <span>✓ 100% Offline & Local</span>
            <span>✓ Zero Telemetry</span>
            <span>✓ Apache-2.0 License</span>
          </div>
        </div>

        {/* Animated Agent Logo Marquee Ticker */}
        <div className="mb-14 overflow-hidden border-y border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-neutral-900/40 py-3 -mx-4 sm:-mx-6 px-4 sm:px-6 relative">
          <div className="relative overflow-hidden w-full">
            {/* Left & Right fade masks */}
            <div className="pointer-events-none absolute left-0 top-0 bottom-0 w-8 sm:w-16 bg-gradient-to-r from-neutral-50 dark:from-[#0a0a0c] to-transparent z-10" />
            <div className="pointer-events-none absolute right-0 top-0 bottom-0 w-8 sm:w-16 bg-gradient-to-l from-neutral-50 dark:from-[#0a0a0c] to-transparent z-10" />

            <div className="animate-marquee flex items-center gap-4 py-1">
              {[...adapters, ...adapters].map((agent, idx) => (
                <div
                  key={`${agent.id}-${idx}`}
                  className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 shadow-xs hover:border-neutral-400 dark:hover:border-neutral-600 transition shrink-0"
                >
                  <div className="shrink-0">{getAgentLogo(agent.id, 16)}</div>
                  <span className="font-mono text-xs font-medium text-neutral-800 dark:text-neutral-200">{agent.name}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Live Terminal Simulation */}
        <section className="border border-neutral-300 dark:border-neutral-800 rounded-lg overflow-hidden bg-neutral-950 text-neutral-100 shadow-xl mb-16 font-mono text-xs">
          
          {/* Terminal Window Header */}
          <div className="bg-neutral-900 px-4 py-2.5 flex items-center justify-between border-b border-neutral-800">
            <div className="flex items-center gap-2">
              <span className="w-2.5 h-2.5 rounded-full bg-neutral-700" />
              <span className="w-2.5 h-2.5 rounded-full bg-neutral-700" />
              <span className="w-2.5 h-2.5 rounded-full bg-neutral-700" />
              <span className="ml-2 text-[11px] text-neutral-400">agentworth ~ terminal</span>
            </div>
            <button
              onClick={() => setActiveStep(0)}
              className="text-[11px] text-neutral-400 hover:text-neutral-200 transition"
            >
              [Replay]
            </button>
          </div>

          {/* Terminal Content */}
          <div className="p-5 space-y-3 min-h-[220px]">
            {terminalSteps.slice(0, activeStep + 1).map((step, idx) => (
              <div key={idx} className="space-y-1">
                <div className="text-neutral-400 flex items-center gap-2">
                  <span className="text-emerald-400">➜</span>
                  <span className="font-semibold text-white">{step.label}</span>
                </div>
                <div className="text-neutral-300 pl-4">{step.output}</div>
              </div>
            ))}
          </div>
        </section>

        {/* Technical Architecture & Value (OpenCode Fig 1, 2, 3 Style) */}
        <section className="mb-20">
          <div className="mb-8">
            <h2 className="text-xl font-bold tracking-tight text-neutral-950 dark:text-white font-sans">
              Engineered for real agent archaeology
            </h2>
            <p className="text-sm text-neutral-600 dark:text-neutral-400 mt-1">
              Deterministic parsing, explainable scoring, and verifiable evidence ladders.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            
            {/* Fig 1: Token Math */}
            <div className="border border-neutral-200 dark:border-neutral-800 rounded-lg p-5 bg-white dark:bg-[#121215] shadow-sm flex flex-col justify-between">
              <div>
                <div className="font-mono text-xs text-neutral-400 dark:text-neutral-500 mb-2">Fig 1. Token Accounting</div>
                <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100 mb-2">Input, Output & Cache Math</h3>
                <p className="text-xs text-neutral-600 dark:text-neutral-400 leading-relaxed mb-4">
                  Normalizes prompt, completion, and cache creation/read tokens across 11 different vendor log formats into precise dollar expenditures.
                </p>
              </div>
              <div className="bg-neutral-50 dark:bg-neutral-900 rounded border border-neutral-200 dark:border-neutral-800 p-3 font-mono text-[11px] text-neutral-700 dark:text-neutral-300 space-y-1">
                <div className="flex justify-between">
                  <span>Input Tokens</span>
                  <span className="font-bold text-neutral-950 dark:text-white">4.2M</span>
                </div>
                <div className="flex justify-between">
                  <span>Output Tokens</span>
                  <span className="font-bold text-neutral-950 dark:text-white">890K</span>
                </div>
                <div className="flex justify-between text-neutral-500 dark:text-neutral-400">
                  <span>Cache Hits (84%)</span>
                  <span>9.6M</span>
                </div>
              </div>
            </div>

            {/* Fig 2: Outcome Hierarchy */}
            <div className="border border-neutral-200 dark:border-neutral-800 rounded-lg p-5 bg-white dark:bg-[#121215] shadow-sm flex flex-col justify-between">
              <div>
                <div className="font-mono text-xs text-neutral-400 dark:text-neutral-500 mb-2">Fig 2. Evidence Ladder</div>
                <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100 mb-2">Deterministic Verification</h3>
                <p className="text-xs text-neutral-600 dark:text-neutral-400 leading-relaxed mb-4">
                  Never trusts self-claimed "done" messages. Extracts empirical proofs: file modifications, compiler exit codes, and git commits.
                </p>
              </div>
              <div className="bg-neutral-50 dark:bg-neutral-900 rounded border border-neutral-200 dark:border-neutral-800 p-2.5 font-mono text-[11px] space-y-1">
                <div className="text-emerald-700 dark:text-emerald-400 flex items-center gap-1.5 font-semibold">
                  <CheckCircle2 className="w-3.5 h-3.5" />
                  <span>CI / Build Passed</span>
                </div>
                <div className="text-neutral-700 dark:text-neutral-300 pl-5">▲ Commit Observed</div>
                <div className="text-neutral-500 dark:text-neutral-400 pl-5">▲ Artifact Changed</div>
                <div className="text-neutral-400 dark:text-neutral-600 pl-5 line-through">▲ Claimed "Done"</div>
              </div>
            </div>

            {/* Fig 3: Local Storage */}
            <div className="border border-neutral-200 dark:border-neutral-800 rounded-lg p-5 bg-white dark:bg-[#121215] shadow-sm flex flex-col justify-between">
              <div>
                <div className="font-mono text-xs text-neutral-400 dark:text-neutral-500 mb-2">Fig 3. Zero Telemetry</div>
                <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100 mb-2">100% Local SQLite WAL</h3>
                <p className="text-xs text-neutral-600 dark:text-neutral-400 leading-relaxed mb-4">
                  Raw transcripts remain untouched on disk. Only derived metrics and fingerprints are stored in your local SQLite index.
                </p>
              </div>
              <div className="bg-neutral-50 dark:bg-neutral-900 rounded border border-neutral-200 dark:border-neutral-800 p-3 font-mono text-[11px] text-neutral-700 dark:text-neutral-300 space-y-1.5">
                <div className="flex items-center justify-between text-neutral-900 dark:text-neutral-100 font-semibold">
                  <span>~/.agentworth/index.db</span>
                  <span className="text-emerald-600 dark:text-emerald-400 font-mono">WAL</span>
                </div>
                <div className="text-neutral-500 dark:text-neutral-400 text-[10px]">
                  Zero network requests · Lazy transcript reads
                </div>
              </div>
            </div>

          </div>
        </section>

        {/* Supported Adapters Section */}
        <section className="mb-20">
          <div className="border border-neutral-200 dark:border-neutral-800 rounded-lg p-6 bg-white dark:bg-[#121215] shadow-sm">
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6 pb-4 border-b border-neutral-200 dark:border-neutral-800">
              <div>
                <h2 className="text-lg font-bold tracking-tight text-neutral-950 dark:text-white font-sans">
                  Supported Agent Adapters
                </h2>
                <p className="text-xs text-neutral-600 dark:text-neutral-400 mt-0.5">
                  Native streaming parsers discovering history paths on macOS and Linux.
                </p>
              </div>
              <span className="font-mono text-xs px-2 py-1 rounded bg-neutral-100 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 self-start sm:self-auto">
                11 Adapters Active
              </span>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
              {adapters.map((adapter) => (
                <div
                  key={adapter.id}
                  className="border border-neutral-200 dark:border-neutral-800 rounded p-3.5 bg-neutral-50/50 dark:bg-neutral-900/50 hover:bg-neutral-50 dark:hover:bg-neutral-900 transition flex items-start gap-3"
                >
                  <div className="p-1.5 rounded bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 shrink-0 shadow-xs">
                    {getAgentLogo(adapter.id, 20)}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="font-mono text-xs mb-1 font-semibold text-neutral-900 dark:text-neutral-100 truncate">
                      {adapter.name}
                    </div>
                    <div className="font-mono text-[11px] text-neutral-500 dark:text-neutral-400 truncate">
                      {adapter.path}
                    </div>
                  </div>
                </div>
              ))}

              {/* 12th Box: Request New Adapter */}
              <a
                href="https://github.com/unfoundbox/agentworth/issues/new?title=Adapter+Request:+[Agent+Name]&labels=enhancement,adapter"
                target="_blank"
                rel="noreferrer"
                className="border border-dashed border-neutral-300 dark:border-neutral-700 hover:border-neutral-500 dark:hover:border-neutral-500 rounded p-3.5 bg-neutral-50/30 dark:bg-neutral-900/30 hover:bg-neutral-50 dark:hover:bg-neutral-900 transition flex items-start gap-3 group text-left"
              >
                <div className="p-1.5 rounded bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 group-hover:border-neutral-500 dark:group-hover:border-neutral-500 shrink-0 shadow-xs flex items-center justify-center text-neutral-600 dark:text-neutral-400 group-hover:text-black dark:group-hover:text-white transition">
                  <Plus className="w-5 h-5" />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between font-mono text-xs mb-1">
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-black dark:group-hover:text-white">Want to add yours?</span>
                    <span className="text-[10px] text-neutral-700 dark:text-neutral-300 bg-neutral-100 dark:bg-neutral-800 group-hover:bg-black group-hover:text-white dark:group-hover:bg-white dark:group-hover:text-black border border-neutral-300 dark:border-neutral-700 px-1.5 py-0.2 rounded font-medium transition flex items-center gap-0.5">
                      Request here <ExternalLink className="w-2.5 h-2.5 inline" />
                    </span>
                  </div>
                  <div className="font-mono text-[11px] text-neutral-500 dark:text-neutral-400 group-hover:text-neutral-700 dark:group-hover:text-neutral-300 truncate">
                    Open an issue on GitHub
                  </div>
                </div>
              </a>
            </div>
          </div>
        </section>

        {/* Bottom CTA Box */}
        <section className="border border-neutral-900 dark:border-neutral-800 bg-neutral-950 text-white rounded-lg p-8 sm:p-12 text-center shadow-lg">
          <h2 className="text-2xl sm:text-3xl font-extrabold tracking-tight mb-3">
            Start discovering your agent history.
          </h2>
          <p className="text-sm text-neutral-400 max-w-md mx-auto mb-6">
            Run the precompiled native binary with zero installation. Works offline on your machine.
          </p>

          <div className="inline-flex items-center gap-3 bg-neutral-900 border border-neutral-800 rounded px-4 py-2 font-mono text-sm text-neutral-100 shadow-inner">
            <span>$ npx agentworth</span>
            <button
              onClick={() => handleCopy('npx agentworth')}
              className="text-xs text-neutral-400 hover:text-white transition"
              aria-label="Copy npx agentworth"
            >
              {copied ? '✓' : 'Copy'}
            </button>
          </div>

          <div className="mt-8 flex items-center justify-center gap-6 font-mono text-xs text-neutral-400">
            <a
              href="https://github.com/unfoundbox/agentworth"
              target="_blank"
              rel="noreferrer"
              className="hover:text-white transition flex items-center gap-1.5"
            >
              <Github className="w-3.5 h-3.5" />
              <span>Star on GitHub</span>
            </a>
            <span>·</span>
            <a
              href="https://agentworth.dev/llms.txt"
              target="_blank"
              rel="noreferrer"
              className="hover:text-white transition"
            >
              llms.txt
            </a>
          </div>
        </section>

      </main>

      {/* Minimal Footer */}
      <footer className="border-t border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-[#0a0a0c] py-8 text-center font-mono text-xs text-neutral-500 dark:text-neutral-400 transition-colors">
        <div className="max-w-5xl mx-auto px-4 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div>
            <span>AgentWorth</span> · <span>Apache-2.0 License</span>
          </div>
          <div>
            <span>Local-first AI trajectory archaeology</span>
          </div>
        </div>
      </footer>

    </div>
  );
};
