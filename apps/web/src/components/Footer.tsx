import React from 'react';
import { Github, ArrowUpRight } from 'lucide-react';

export const Footer: React.FC = () => {
  return (
    <footer className="border-t-2 border-zinc-200 dark:border-zinc-800 bg-[#f8f9fa] dark:bg-[#0a0a0c] text-black dark:text-white font-mono text-xs py-12 transition-colors">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 space-y-10">
        
        {/* Architecture ASCII Flow */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-8 items-center border border-zinc-300 dark:border-zinc-800 bg-white dark:bg-[#121215] p-6 sm:p-8 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-none">
          <div>
            <div className="inline-block border border-zinc-300 dark:border-zinc-700 px-2 py-0.5 bg-zinc-100 dark:bg-zinc-800 mb-2 font-bold text-[10px] uppercase text-neutral-800 dark:text-neutral-200">
              ZERO TELEMETRY GUARANTEE
            </div>
            <h3 className="text-xl font-bold uppercase mb-2 text-neutral-950 dark:text-white">LOCAL MEANS LOCAL</h3>
            <p className="text-zinc-600 dark:text-zinc-400 text-xs leading-relaxed mb-4">
              AgentWorth indexes traces directly from your local filesystem into a local SQLite database at <code className="bg-zinc-100 dark:bg-zinc-800 px-1 py-0.5 border border-zinc-300 dark:border-zinc-700">~/.agentworth/agentworth.db</code>. No servers, no tracking, no external API calls.
            </p>
            <div className="flex items-center space-x-3 text-xs">
              <a
                href="https://github.com/unfoundbox/agentworth"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center space-x-1 font-bold underline hover:text-zinc-600 dark:hover:text-zinc-300"
              >
                <span>View Rust source</span>
                <ArrowUpRight className="w-3.5 h-3.5" />
              </a>
              <span className="text-zinc-400">·</span>
              <a
                href="https://github.com/unfoundbox/agentworth/blob/main/LICENSE"
                target="_blank"
                rel="noreferrer"
                className="font-bold underline hover:text-zinc-600 dark:hover:text-zinc-300"
              >
                Apache 2.0 / MIT
              </a>
            </div>
          </div>

          {/* ASCII Diagram Box */}
          <div className="bg-zinc-950 text-emerald-400 p-4 border border-zinc-900 font-mono text-xs overflow-x-auto select-none">
            <pre className="leading-relaxed">
{`┌───────────────────────────────────────┐
│              YOUR MACHINE             │
│   (~/.claude, ~/.codex, ~/.gemini)    │
└──────────────────┬────────────────────┘
                   │ local adapters
                   ▼
┌───────────────────────────────────────┐
│             AGENTWORTH                │
│    (Normalization + Scoring Engine)   │
└──────────────────┬────────────────────┘
                   │
                   ▼
┌───────────────────────────────────────┐
│            LOCAL SQLITE               │
│     (~/.agentworth/agentworth.db)     │
└──────────────────┬────────────────────┘
                   │
                   ▼
┌───────────────────────────────────────┐
│            YOU (OFFLINE)              │
│       Nothing ever uploads.           │
└───────────────────────────────────────┘`}
            </pre>
          </div>
        </div>

        {/* Bottom Bar */}
        <div className="flex flex-col sm:flex-row items-center justify-between gap-4 pt-6 border-t border-zinc-300 text-zinc-500 text-[11px]">
          <div className="flex items-center space-x-2">
            <span className="font-bold text-black">AGENTWORTH</span>
            <span>— Carbon dating your AI exhaust</span>
          </div>

          <div className="flex items-center space-x-4">
            <span>Built with Rust, React, Vite &amp; Tailwind</span>
            <a
              href="https://github.com/unfoundbox/agentworth"
              target="_blank"
              rel="noreferrer"
              className="text-black font-semibold hover:underline flex items-center space-x-1"
            >
              <Github className="w-3.5 h-3.5" />
              <span>GitHub</span>
            </a>
          </div>
        </div>

      </div>
    </footer>
  );
};
