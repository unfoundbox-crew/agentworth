import React, { useState } from "react";
import { Github, Copy, Check, ShieldCheck, ArrowRight } from "lucide-react";
import { trackEvent } from "../services/analytics";
import { ThemeToggle } from "./ThemeToggle";
import { VerdictBoard } from "./VerdictBoard";
import { CacheCliffWidget } from "./CacheCliffWidget";
import { CoverageMatrix } from "./CoverageMatrix";
import { VerdictStamp } from "./VerdictStamp";
import { ArchieMascot } from "./ArchieMascot";

interface LandingPageProps {
  onOpenExplorer?: () => void;
}

export const LandingPage: React.FC<LandingPageProps> = ({ onOpenExplorer }) => {
  const [copied, setCopied] = useState(false);

  const installCommand = "npx agentworth";

  const handleCopy = (cmd: string) => {
    navigator.clipboard.writeText(cmd);
    setCopied(true);
    trackEvent("npx_command_copied", { command: cmd });
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="min-h-screen bg-[#ffffff] dark:bg-[#0a0a0c] text-[#111111] dark:text-[#ececed] font-sans antialiased selection:bg-neutral-900 selection:text-white dark:selection:bg-white dark:selection:text-black transition-colors duration-200">
      
      {/* Top Navbar */}
      <nav className="sticky top-0 z-50 bg-white/90 dark:bg-[#0a0a0c]/90 backdrop-blur-md border-b border-neutral-200 dark:border-neutral-800 transition-colors">
        <div className="max-w-6xl mx-auto px-4 sm:px-6 h-16 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <div className="w-6 h-6 bg-black dark:bg-white rounded flex items-center justify-center text-white dark:text-black font-mono text-xs font-bold shadow-xs">
              aw
            </div>
            <span className="font-sans font-bold tracking-tight text-[16px] text-neutral-950 dark:text-white">
              AgentWorth
            </span>
            <span className="font-mono text-[10px] px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-900 text-neutral-500 dark:text-neutral-400 border border-neutral-200 dark:border-neutral-800">
              v0.1.2
            </span>
          </div>

          <div className="flex items-center gap-2">
            {onOpenExplorer && (
              <button
                onClick={onOpenExplorer}
                className="px-3 py-1.5 rounded bg-black hover:bg-neutral-800 dark:bg-white dark:hover:bg-neutral-200 text-white dark:text-black font-mono text-xs font-bold transition shadow-xs flex items-center gap-1"
              >
                <span>Launch Explorer</span>
                <ArrowRight className="w-3.5 h-3.5" />
              </button>
            )}

            <a
              href="https://github.com/unfoundbox-crew/agentworth"
              target="_blank"
              rel="noreferrer"
              title="GitHub Repository"
              className="p-1.5 rounded-md text-neutral-600 hover:text-black dark:text-neutral-400 dark:hover:text-white hover:bg-neutral-100 dark:hover:bg-neutral-900 transition flex items-center justify-center"
              aria-label="GitHub Repository"
            >
              <Github className="w-4 h-4" />
            </a>
            <div className="h-4 w-[1px] bg-neutral-200 dark:border-neutral-800 mx-1" />
            <ThemeToggle />
          </div>
        </div>
      </nav>

      {/* Main Container */}
      <main className="max-w-6xl mx-auto px-4 sm:px-6 pt-12 sm:pt-16 pb-20 space-y-20 sm:space-y-28">
        
        {/* § 1 · HERO */}
        <section className="text-center max-w-4xl mx-auto pt-4">
          
          {/* Trust badge */}
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 text-xs font-mono text-neutral-800 dark:text-neutral-300 mb-6">
            <span className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
            <span>The Verdict Layer for AI Coding Agents</span>
          </div>

          {/* Headline */}
          <h1 className="text-3xl sm:text-5xl lg:text-6xl font-black tracking-tight text-neutral-950 dark:text-white leading-[1.12] mb-6">
            Every agent says it&apos;s done.
            <br />
            <span className="underline decoration-black dark:decoration-white decoration-4 underline-offset-8">
              AgentWorth checks the git log.
            </span>
          </h1>

          {/* Subhead */}
          <p className="text-base sm:text-lg text-neutral-600 dark:text-neutral-400 leading-relaxed font-normal max-w-2xl mx-auto mb-8 font-sans">
            AgentWorth reads the session logs already on your disk — Claude Code, Codex, Cursor, Antigravity and 20+ CLIs — and grades every session against evidence it can check: files changed, tests passed, commits landed, CI green. Native Rust, local SQLite, zero telemetry.
          </p>

          {/* Install box */}
          <div className="max-w-md mx-auto mb-6">
            <div className="border-2 border-black dark:border-white bg-white dark:bg-[#121215] shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] p-3 flex items-center justify-between gap-3">
              <code className="font-mono text-sm sm:text-base font-bold text-black dark:text-white select-all">
                $ {installCommand}
              </code>
              <button
                onClick={() => handleCopy(installCommand)}
                className="flex items-center gap-1.5 px-3 py-1.5 bg-black dark:bg-white text-white dark:text-black font-mono text-xs font-bold transition hover:bg-neutral-800 dark:hover:bg-neutral-200"
              >
                {copied ? <Check className="w-3.5 h-3.5 text-emerald-400 dark:text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
                <span>{copied ? "Copied" : "Copy"}</span>
              </button>
            </div>

            <div className="flex items-center justify-between mt-3 px-1 text-[11px] font-mono text-neutral-500 dark:text-neutral-400">
              <span>✓ 100% Offline</span>
              <span>✓ Local SQLite WAL</span>
              <span>✓ Zero Telemetry</span>
              <span>✓ Apache-2.0</span>
            </div>
          </div>

          {/* Hero Visual: A Real Scored Session Card */}
          <div className="mt-12 text-left border-2 border-black dark:border-white bg-white dark:bg-[#121215] p-5 sm:p-6 font-mono shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)]">
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-4 border-b border-neutral-300 dark:border-neutral-800">
              <div className="flex items-center gap-3">
                <span className="w-3 h-3 bg-black dark:bg-white inline-block" />
                <span className="font-bold text-xs sm:text-sm text-black dark:text-white">
                  SESSION AUDIT #98893-CLAUDE
                </span>
                <span className="text-[10px] px-1.5 py-0.5 bg-neutral-100 dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-800 text-neutral-600 dark:text-neutral-400">
                  claude-opus-5
                </span>
              </div>
              <VerdictStamp status="ci_or_deployment_verified" size="sm" />
            </div>

            <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 py-4 border-b border-neutral-200 dark:border-neutral-800 text-xs">
              <div>
                <div className="text-[10px] text-neutral-500 uppercase">Outcome Rung</div>
                <div className="font-extrabold text-black dark:text-white mt-0.5">Rung 5 · CI Green</div>
              </div>
              <div>
                <div className="text-[10px] text-neutral-500 uppercase">Composite Score</div>
                <div className="font-extrabold text-black dark:text-white mt-0.5">94.2 / 100</div>
              </div>
              <div>
                <div className="text-[10px] text-neutral-500 uppercase">Token Volume</div>
                <div className="font-extrabold text-black dark:text-white mt-0.5">14.2 M (97.4% Cache)</div>
              </div>
              <div>
                <div className="text-[10px] text-neutral-500 uppercase">List Equivalent</div>
                <div className="font-extrabold text-black dark:text-white mt-0.5">$6.12 USD</div>
              </div>
            </div>

            <div className="pt-4 text-xs space-y-2">
              <div className="text-neutral-500 text-[10px] font-bold uppercase">Empirical Evidence Quoted from Disk:</div>
              <div className="p-2.5 bg-neutral-50 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 text-neutral-800 dark:text-neutral-200 text-[11px]">
                <code>✓ Git commit 4c901e8 observed → CI test-suite exit code 0 on branch main</code>
              </div>
            </div>
          </div>
        </section>

        {/* § 2 · THE LADDER / VERDICT BOARD */}
        <section id="ladder">
          <VerdictBoard />
        </section>

        {/* § 3 · THE CACHE CLIFF */}
        <section id="cache-cliff">
          <CacheCliffWidget />
        </section>

        {/* § 4 · WHAT YOU GET TODAY */}
        <section id="features" className="space-y-6">
          <div>
            <div className="text-xs font-mono font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">
              § WHAT YOU GET TODAY
            </div>
            <h2 className="text-2xl sm:text-3xl font-extrabold tracking-tight">
              Three Real CLI Commands. No Fabricated Data.
            </h2>
            <p className="text-xs sm:text-sm text-neutral-600 dark:text-neutral-400 mt-1 font-sans">
              Shipped in the native Rust binary today.
            </p>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
            {/* Terminal Block 1: Stats */}
            <div className="border-2 border-black dark:border-white bg-black text-white p-5 font-mono text-xs shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between">
              <div>
                <div className="text-neutral-400 text-[11px] mb-2">$ agentworth stats</div>
                <div className="text-zinc-300 space-y-1 text-[11px]">
                  <div className="flex justify-between text-white font-bold"><span>Sessions:</span><span>9,713</span></div>
                  <div className="flex justify-between"><span>Total tokens:</span><span>77.9 B</span></div>
                  <div className="flex justify-between text-emerald-400"><span>Cache read:</span><span>97.2%</span></div>
                  <div className="flex justify-between"><span>List equivalent:</span><span>$33,234</span></div>
                  <div className="flex justify-between text-zinc-400"><span>Uploaded:</span><span>0 bytes</span></div>
                </div>
              </div>
              <div className="mt-4 pt-3 border-t border-zinc-800 text-[11px] text-zinc-400 font-sans">
                Full token math across input, output, cache-read, and cache-creation.
              </div>
            </div>

            {/* Terminal Block 2: Pacing */}
            <div className="border-2 border-black dark:border-white bg-black text-white p-5 font-mono text-xs shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between">
              <div>
                <div className="text-neutral-400 text-[11px] mb-2">$ agentworth usage --pacing</div>
                <div className="text-zinc-300 space-y-1 text-[11px]">
                  <div className="flex justify-between text-white font-bold"><span>Pacing window:</span><span>5 hours</span></div>
                  <div className="flex justify-between"><span>Burn rate:</span><span>1.4M tok/hr</span></div>
                  <div className="flex justify-between text-emerald-400"><span>Cache hit %:</span><span>98.1%</span></div>
                  <div className="flex justify-between"><span>5h spend:</span><span>$4.18</span></div>
                  <div className="flex justify-between text-zinc-400"><span>Active tasks:</span><span>3</span></div>
                </div>
              </div>
              <div className="mt-4 pt-3 border-t border-zinc-800 text-[11px] text-zinc-400 font-sans">
                Real-time burn rate pacing aligned with Anthropic 5-hour rate limits.
              </div>
            </div>

            {/* Terminal Block 3: Blame */}
            <div className="border-2 border-black dark:border-white bg-black text-white p-5 font-mono text-xs shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between">
              <div>
                <div className="text-neutral-400 text-[11px] mb-2">$ agentworth blame src/api.ts</div>
                <div className="text-zinc-300 space-y-1 text-[11px]">
                  <div className="flex justify-between text-white font-bold"><span>L10-45:</span><span>Claude Opus</span></div>
                  <div className="flex justify-between"><span>Prompt:</span><span>&apos;Zero-mock refactor&apos;</span></div>
                  <div className="flex justify-between text-emerald-400"><span>Session:</span><span>#89312 (Rung 5)</span></div>
                  <div className="flex justify-between"><span>Modified:</span><span>2026-08-31 10:20</span></div>
                  <div className="flex justify-between text-zinc-400"><span>Diff:</span><span>+45 -12 lines</span></div>
                </div>
              </div>
              <div className="mt-4 pt-3 border-t border-zinc-800 text-[11px] text-zinc-400 font-sans">
                Trace any line of source code back to the exact agent session and prompt that wrote it.
              </div>
            </div>
          </div>
        </section>

        {/* § 5 · COVERAGE MATRIX */}
        <section id="coverage">
          <CoverageMatrix />
        </section>

        {/* § 6 · LOCAL MEANS LOCAL (4 Invariants from AGENTS.md) */}
        <section id="invariants" className="border-2 border-black dark:border-white bg-white dark:bg-[#121215] p-6 sm:p-8 font-mono shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)]">
          <div className="flex items-center gap-2 mb-2">
            <span className="text-[10px] font-bold px-2 py-0.5 bg-black dark:bg-white text-white dark:text-black">
              CANONICAL INVARIANTS
            </span>
            <span className="text-xs font-bold text-neutral-500 uppercase">
              AGENTS.md Contract
            </span>
          </div>
          <h2 className="text-2xl font-extrabold tracking-tight mb-6">
            Local Means Local. Always.
          </h2>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 text-xs">
            <div className="p-4 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 space-y-1.5">
              <div className="font-extrabold text-black dark:text-white flex items-center gap-1.5">
                <ShieldCheck className="w-4 h-4 text-emerald-500" />
                <span>1. Never upload user data</span>
              </div>
              <p className="text-neutral-600 dark:text-neutral-400 font-sans leading-relaxed">
                Scanning works completely offline. Raw histories remain on your disk untouched. Zero network telemetry.
              </p>
            </div>

            <div className="p-4 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 space-y-1.5">
              <div className="font-extrabold text-black dark:text-white flex items-center gap-1.5">
                <ShieldCheck className="w-4 h-4 text-emerald-500" />
                <span>2. No raw-log duplication</span>
              </div>
              <p className="text-neutral-600 dark:text-neutral-400 font-sans leading-relaxed">
                AgentWorth never copies multi-gigabyte transcripts into SQLite. It stores only derived indexes, scores, and fingerprints.
              </p>
            </div>

            <div className="p-4 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 space-y-1.5">
              <div className="font-extrabold text-black dark:text-white flex items-center gap-1.5">
                <ShieldCheck className="w-4 h-4 text-emerald-500" />
                <span>3. Streaming JSONL parsing</span>
              </div>
              <p className="text-neutral-600 dark:text-neutral-400 font-sans leading-relaxed">
                Bounded memory parsers handle 100+ GB logs without crashing. Rescans skip unchanged files in milliseconds via SHA-256.
              </p>
            </div>

            <div className="p-4 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 space-y-1.5">
              <div className="font-extrabold text-black dark:text-white flex items-center gap-1.5">
                <ShieldCheck className="w-4 h-4 text-emerald-500" />
                <span>4. Deterministic verification</span>
              </div>
              <p className="text-neutral-600 dark:text-neutral-400 font-sans leading-relaxed">
                Never trusts self-claimed completion. Scores are backed by compiler exit codes, positive diffs, and git commits.
              </p>
            </div>
          </div>
        </section>

        {/* § 7 · WHERE THIS GOES / ROADMAP */}
        <section id="roadmap" className="border-2 border-dashed border-neutral-400 dark:border-neutral-700 bg-neutral-50/50 dark:bg-neutral-900/30 p-6 sm:p-8 font-mono">
          <div className="flex items-center justify-between gap-2 mb-2">
            <div className="text-xs font-bold uppercase tracking-wider text-neutral-500">
              § 7 · PHASE 2 ROADMAP
            </div>
            <VerdictStamp status="not_built" size="sm" />
          </div>
          <h2 className="text-xl sm:text-2xl font-extrabold tracking-tight mb-2">
            Policy Engine: Route, Explain, Run
          </h2>
          <p className="text-xs text-neutral-600 dark:text-neutral-400 font-sans leading-relaxed mb-6">
            Transparently marked <strong>NOT BUILT</strong>. Routing off your own repo&apos;s verified build/test/CI history, computed locally on your machine.
          </p>

          <div className="bg-black text-white p-4 text-xs font-mono border border-neutral-800">
            <div className="text-neutral-400 mb-1">$ aw route --task &apos;fix race condition in queue&apos;</div>
            <div className="text-emerald-400 font-bold">RECOMMENDED: Claude Sonnet 4.6 (Score: 92.4% success on async Rust tasks in this repo)</div>
            <div className="text-zinc-400 mt-1">Estimated Cost: $0.18 · Expected Turns: 4 · Context Cache: Warm</div>
          </div>
        </section>

      </main>

      {/* Footer */}
      <footer className="border-t-2 border-black dark:border-white bg-[#f8f9fa] dark:bg-[#0a0a0c] py-12 font-mono text-xs">
        <div className="max-w-6xl mx-auto px-4 sm:px-6 flex flex-col sm:flex-row items-center justify-between gap-6">
          <ArchieMascot />
          <div className="text-right space-y-1 text-neutral-500">
            <div className="text-black dark:text-white font-bold">AgentWorth v0.1.2</div>
            <div>Apache-2.0 License · Native Rust Core</div>
            <div className="pt-2">
              <a
                href="https://github.com/unfoundbox-crew/agentworth"
                target="_blank"
                rel="noreferrer"
                className="underline hover:text-black dark:hover:text-white font-bold"
              >
                github.com/unfoundbox-crew/agentworth
              </a>
            </div>
          </div>
        </div>
      </footer>

    </div>
  );
};
