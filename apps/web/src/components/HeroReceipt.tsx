import React, { useState, useEffect } from 'react';
import { Copy, Check } from 'lucide-react';
import { AggregateStats } from '../types';
import { formatTokens, estimateTokenCostUSD, formatUSD, getAdapterBadge } from '../utils/formatters';

interface HeroReceiptProps {
  stats: AggregateStats;
  onScanClick: () => void;
}

export const HeroReceipt: React.FC<HeroReceiptProps> = ({ stats, onScanClick }) => {
  const [copied, setCopied] = useState(false);
  const [terminalStep, setTerminalStep] = useState(0);

  const command = 'npx agentworth';

  const handleCopy = () => {
    navigator.clipboard.writeText(command);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  useEffect(() => {
    const timer = setInterval(() => {
      setTerminalStep((prev) => (prev < 5 ? prev + 1 : 5));
    }, 600);
    return () => clearInterval(timer);
  }, []);

  const totalTokensNum =
    stats.token_usage.input_tokens +
    stats.token_usage.output_tokens +
    stats.token_usage.cache_read_input_tokens +
    stats.token_usage.cache_creation_input_tokens;

  const estimatedCostUSD = estimateTokenCostUSD(totalTokensNum, stats.models_usage_count);

  const verifiedPercent =
    stats.total_sessions > 0
      ? ((stats.verified_outcomes_count / stats.total_sessions) * 100).toFixed(1)
      : '0.0';

  const topAdapterEntry = Object.entries(stats.sessions_by_adapter || {}).sort(
    (a, b) => b[1] - a[1]
  )[0];
  const topAdapterInfo = topAdapterEntry ? getAdapterBadge(topAdapterEntry[0]) : null;
  const topAdapterPercent =
    stats.total_sessions > 0 && topAdapterEntry
      ? ((topAdapterEntry[1] / stats.total_sessions) * 100).toFixed(1)
      : '0.0';

  return (
    <section className="py-8 sm:py-12 border-b border-zinc-300 bg-[#fdfdfd]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        
        {/* Top Header Copy */}
        <div className="text-center max-w-3xl mx-auto mb-8">
          <div className="inline-block border border-zinc-900 px-3 py-1 bg-zinc-50 mb-3 text-xs font-mono tracking-wide shadow-[2px_2px_0px_0px_rgba(0,0,0,1)]">
            CARBON DATING YOUR AI EXHAUST
          </div>
          <h1 className="text-3xl sm:text-4xl lg:text-5xl font-mono font-extrabold tracking-tight text-black mb-3">
            Your agents left receipts.
          </h1>
          <p className="text-sm sm:text-base font-mono text-zinc-600 max-w-xl mx-auto">
            Find out what Claude Code, Codex, Gemini CLI, and OpenCode actually executed in <code className="bg-zinc-100 px-1 py-0.5 border border-zinc-300 text-zinc-800">~/.config</code>.
          </p>
        </div>

        {/* The Receipt Grid */}
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-6 items-stretch">
          
          {/* Left: Terminal Output Box */}
          <div className="lg:col-span-6 bg-zinc-950 text-zinc-200 border border-zinc-900 font-mono text-xs shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between">
            {/* Terminal Window Bar */}
            <div className="bg-zinc-900 border-b border-zinc-800 px-3 py-2 flex items-center justify-between">
              <div className="flex items-center space-x-2">
                <div className="w-2.5 h-2.5 rounded-full bg-red-500/80"></div>
                <div className="w-2.5 h-2.5 rounded-full bg-yellow-500/80"></div>
                <div className="w-2.5 h-2.5 rounded-full bg-green-500/80"></div>
                <span className="text-[11px] text-zinc-400 ml-2 font-mono">agentworth scan --all</span>
              </div>
              <span className="text-[10px] text-zinc-500">local sqlite v3</span>
            </div>

            {/* Terminal Content */}
            <div className="p-4 sm:p-5 space-y-2.5 overflow-x-auto min-h-[220px]">
              <div className="text-zinc-400">
                <span className="text-emerald-400">$</span> npx agentworth scan
              </div>

              {terminalStep >= 1 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.claude/sessions...</span>
                  <span className="text-emerald-400">✓ 2,840 sessions</span>
                </div>
              )}
              {terminalStep >= 2 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.codex/traces...</span>
                  <span className="text-emerald-400">✓ 812 sessions</span>
                </div>
              )}
              {terminalStep >= 3 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.gemini/...</span>
                  <span className="text-emerald-400">✓ 490 sessions</span>
                </div>
              )}
              {terminalStep >= 4 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.opencode/...</span>
                  <span className="text-emerald-400">✓ 139 sessions</span>
                </div>
              )}

              {terminalStep >= 5 && (
                <div className="pt-3 border-t border-zinc-800 space-y-1.5 font-mono text-[11px]">
                  <div className="text-zinc-400">
                    Total Indexed: <span className="text-white font-bold">{stats.total_sessions.toLocaleString()} sessions</span> ({formatTokens(totalTokensNum)} tokens ~ <span className="text-emerald-400 font-semibold">{formatUSD(estimatedCostUSD)}</span>)
                  </div>
                  <div className="text-zinc-400">
                    Verified Outcomes: <span className="text-emerald-400 font-bold">{stats.verified_outcomes_count.toLocaleString()}</span> ({verifiedPercent}%)
                  </div>
                  <div className="text-amber-300 italic pt-1">
                    &gt; Useful work: unfortunately, some.
                  </div>
                </div>
              )}

              {terminalStep < 5 && (
                <div className="flex items-center space-x-1 text-emerald-400">
                  <span>_</span>
                  <span className="animate-cursor-blink">▋</span>
                </div>
              )}
            </div>

            {/* Quick Action / Command Bar */}
            <div className="bg-zinc-900/90 border-t border-zinc-800 p-3 flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center space-x-2 font-mono text-xs text-zinc-300">
                <span className="text-zinc-500">$</span>
                <code className="text-emerald-400 font-semibold">{command}</code>
              </div>
              <div className="flex items-center space-x-2">
                <button
                  onClick={handleCopy}
                  className="flex items-center space-x-1 px-2.5 py-1 text-[11px] font-mono bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700 transition-colors"
                >
                  {copied ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
                  <span>{copied ? 'Copied' : 'Copy'}</span>
                </button>
                <button
                  onClick={onScanClick}
                  className="px-2.5 py-1 text-[11px] font-mono bg-emerald-600 hover:bg-emerald-500 text-black font-semibold border border-emerald-500 transition-colors"
                >
                  Run Scan
                </button>
              </div>
            </div>
          </div>

          {/* Right: The Physical Paper Receipt Card */}
          <div className="lg:col-span-6 bg-[#fdfdfd] border-2 border-zinc-900 p-5 sm:p-6 font-mono text-xs shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] relative flex flex-col justify-between">
            {/* Top receipt punch decoration */}
            <div className="border-b-2 border-dashed border-zinc-400 pb-3 mb-4 text-center">
              <div className="text-base font-extrabold tracking-widest uppercase text-black">
                *** AGENT RECEIPT ***
              </div>
              <div className="text-[10px] text-zinc-500 mt-0.5">
                LOCAL MACHINE AUDIT // NO DATA TRANSMITTED
              </div>
            </div>

            {/* Metrics Breakdown */}
            <div className="space-y-2.5 my-auto">
              <div className="flex justify-between items-center py-1 border-b border-zinc-200">
                <span className="text-zinc-600">TOTAL EXHAUST TOKENS</span>
                <span className="text-sm font-bold text-black">{formatTokens(totalTokensNum)}</span>
              </div>

              <div className="flex justify-between items-center py-1 border-b border-zinc-200">
                <span className="text-zinc-600">ESTIMATED TOKEN COST</span>
                <span className="text-sm font-bold text-emerald-700">{formatUSD(estimatedCostUSD)}</span>
              </div>

              <div className="flex justify-between items-center py-1 border-b border-zinc-200">
                <span className="text-zinc-600">INDEXED SESSIONS</span>
                <span className="text-sm font-bold text-black">{stats.total_sessions.toLocaleString()}</span>
              </div>

              <div className="flex justify-between items-center py-1 border-b border-zinc-200">
                <span className="text-zinc-600">ACTIVE AGENTS DETECTED</span>
                <span className="text-sm font-bold text-black">
                  {Object.keys(stats.sessions_by_adapter || {}).length} adapters ({Object.keys(stats.models_usage_count || {}).length} models)
                </span>
              </div>

              <div className="flex justify-between items-center py-1 border-b border-zinc-200">
                <span className="text-zinc-600">VERIFIED OUTCOMES</span>
                <span className="text-sm font-bold text-emerald-700">
                  {stats.verified_outcomes_count.toLocaleString()}{' '}
                  <span className="text-[10px] font-normal text-zinc-500">({verifiedPercent}%)</span>
                </span>
              </div>

              <div className="flex justify-between items-center py-1 border-b border-zinc-200">
                <span className="text-zinc-600">TOP ADAPTER</span>
                <span className="text-xs font-semibold text-black">
                  {topAdapterInfo?.name || 'Claude Code'} ({topAdapterPercent}%)
                </span>
              </div>
            </div>

            {/* Receipt Bottom / Barcode */}
            <div className="mt-4 pt-3 border-t-2 border-dashed border-zinc-400">
              <div className="flex justify-between items-center text-[10px] text-zinc-500 mb-2">
                <span>AUTH: SQLite SHA-256</span>
                <span>STATUS: VERIFIED</span>
              </div>
              <div className="font-mono text-[9px] tracking-widest text-center text-zinc-400 select-none overflow-hidden">
                |||| | |||||| || | |||| ||| ||||||| |||| || |||||| | |||||| ||||| ||||
              </div>
              <div className="text-center text-[10px] text-zinc-500 mt-1">
                #agentworth-audit-2026
              </div>
            </div>

          </div>

        </div>

      </div>
    </section>
  );
};
