import React, { useState, useEffect } from 'react';
import { Copy, Check, RefreshCw } from 'lucide-react';
import { AggregateStats } from '../types';
import { formatTokens, estimateTokenCostUSD, formatUSD, getAdapterBadge } from '../utils/formatters';

interface HeroReceiptProps {
  stats: AggregateStats;
  onScanClick: () => void;
}

export const HeroReceipt: React.FC<HeroReceiptProps> = ({ stats, onScanClick }) => {
  const [copied, setCopied] = useState(false);
  const [receiptCopied, setReceiptCopied] = useState(false);
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
    }, 500);
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

  const handleCopyReceiptText = () => {
    const rawReceipt = `
========================================
       * * * AGENTWORTH RECEIPT * * *
========================================
STORE: LOCAL MACHINE DISK (~/.config)
DATE:  ${new Date().toISOString().split('T')[0]}
AUTH:  SQLITE SHA-256 (LOCAL ONLY)
----------------------------------------
TOTAL TOKENS BURNT:      ${formatTokens(totalTokensNum).padEnd(12)}
ESTIMATED EXPENDITURE:   ${formatUSD(estimatedCostUSD).padEnd(12)}
INDEXED SESSIONS:        ${stats.total_sessions.toString().padEnd(12)}
ACTIVE ADAPTERS:         ${Object.keys(stats.sessions_by_adapter || {}).length.toString().padEnd(12)}
VERIFIED OUTCOMES:       ${stats.verified_outcomes_count.toString().padEnd(6)} (${verifiedPercent}%)
TOP ADAPTER:             ${(topAdapterInfo?.name || 'Claude Code')} (${topAdapterPercent}%)
----------------------------------------
TOTAL AMOUNT DUE:        ${formatUSD(estimatedCostUSD)}
TAX:                     $0.00
----------------------------------------
||| | ||||| || |||||| | |||| ||| |||||||
#agentworth-audit-2026
========================================`.trim();

    navigator.clipboard.writeText(rawReceipt);
    setReceiptCopied(true);
    setTimeout(() => setReceiptCopied(false), 2000);
  };

  return (
    <section className="py-10 sm:py-14 border-b-2 border-black bg-[#fbfbfb]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        
        {/* Top Header */}
        <div className="text-center max-w-3xl mx-auto mb-10">
          <div className="inline-flex items-center space-x-2 border-2 border-black px-3 py-1 bg-white mb-4 text-xs font-mono font-bold tracking-wider uppercase shadow-[3px_3px_0px_0px_rgba(0,0,0,1)]">
            <span className="w-2 h-2 bg-black animate-pulse"></span>
            <span>CARBON DATING YOUR AI EXHAUST</span>
          </div>
          <h1 className="text-4xl sm:text-5xl lg:text-6xl font-mono font-extrabold tracking-tight text-black mb-4">
            Your agents left receipts.
          </h1>
          <p className="text-sm sm:text-base font-mono text-zinc-600 max-w-2xl mx-auto leading-relaxed">
            Discover, normalize, and audit what Claude Code, Cursor, Codex, and Antigravity actually executed on your machine.
          </p>
        </div>

        {/* The 2-Column Physical Grid */}
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-stretch">
          
          {/* Left: Minimalist Obsidian Console */}
          <div className="lg:col-span-6 bg-black text-white border-2 border-black font-mono text-xs shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between">
            {/* Terminal Titlebar */}
            <div className="bg-zinc-900 border-b border-zinc-800 px-4 py-3 flex items-center justify-between">
              <div className="flex items-center space-x-2">
                <div className="w-3 h-3 border border-zinc-700 bg-zinc-800"></div>
                <div className="w-3 h-3 border border-zinc-700 bg-zinc-800"></div>
                <div className="w-3 h-3 border border-zinc-700 bg-zinc-800"></div>
                <span className="text-xs text-zinc-300 ml-2 font-mono font-semibold">agentworth scan --all</span>
              </div>
              <span className="text-[10px] text-zinc-400 font-mono uppercase tracking-wider">OFFLINE INDEX</span>
            </div>

            {/* Terminal Logs */}
            <div className="p-5 space-y-3 overflow-x-auto min-h-[260px] font-mono text-xs">
              <div className="text-zinc-400">
                <span className="text-white font-bold">$</span> npx agentworth scan
              </div>

              {terminalStep >= 1 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.claude/sessions...</span>
                  <span className="text-white font-bold">[2,840 TRACES]</span>
                </div>
              )}
              {terminalStep >= 2 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.cursor/history...</span>
                  <span className="text-white font-bold">[812 TRACES]</span>
                </div>
              )}
              {terminalStep >= 3 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.gemini/antigravity...</span>
                  <span className="text-white font-bold">[490 TRACES]</span>
                </div>
              )}
              {terminalStep >= 4 && (
                <div className="text-zinc-300 flex items-center justify-between">
                  <span>Scanning ~/.codex/traces...</span>
                  <span className="text-white font-bold">[139 TRACES]</span>
                </div>
              )}

              {terminalStep >= 5 && (
                <div className="pt-3 border-t border-zinc-800 space-y-2 font-mono text-xs">
                  <div className="text-zinc-300 flex justify-between">
                    <span className="text-zinc-400">TOTAL INDEXED:</span>
                    <span className="text-white font-bold">{stats.total_sessions.toLocaleString()} sessions ({formatTokens(totalTokensNum)} tokens)</span>
                  </div>
                  <div className="text-zinc-300 flex justify-between">
                    <span className="text-zinc-400">ESTIMATED COST:</span>
                    <span className="text-white font-bold">{formatUSD(estimatedCostUSD)} USD</span>
                  </div>
                  <div className="text-zinc-300 flex justify-between">
                    <span className="text-zinc-400">VERIFIED OUTCOMES:</span>
                    <span className="text-white font-bold">{stats.verified_outcomes_count.toLocaleString()} ({verifiedPercent}%)</span>
                  </div>
                  <div className="text-zinc-400 italic pt-2 text-[11px]">
                    &gt; SQLite database up-to-date: ~/.agentworth/agentworth.db
                  </div>
                </div>
              )}

              {terminalStep < 5 && (
                <div className="flex items-center space-x-1 text-white">
                  <span>_</span>
                  <span className="animate-cursor-blink">▋</span>
                </div>
              )}
            </div>

            {/* Quick Action Bar */}
            <div className="bg-zinc-900 border-t border-zinc-800 p-3.5 flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center space-x-2 font-mono text-xs text-zinc-300">
                <span className="text-zinc-500 font-bold">$</span>
                <code className="text-white font-bold bg-black px-2 py-0.5 border border-zinc-700">{command}</code>
              </div>
              <div className="flex items-center space-x-2">
                <button
                  onClick={handleCopy}
                  className="flex items-center space-x-1.5 px-3 py-1.5 text-xs font-mono bg-zinc-800 hover:bg-zinc-700 text-white border border-zinc-600 transition-colors active:translate-x-0.5 active:translate-y-0.5"
                >
                  {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                  <span>{copied ? 'Copied' : 'Copy'}</span>
                </button>
                <button
                  onClick={onScanClick}
                  className="flex items-center space-x-1.5 px-3 py-1.5 text-xs font-mono bg-white hover:bg-zinc-200 text-black font-bold border border-white transition-colors active:translate-x-0.5 active:translate-y-0.5"
                >
                  <RefreshCw className="w-3.5 h-3.5" />
                  <span>Run Scan</span>
                </button>
              </div>
            </div>
          </div>

          {/* Right: The Physical Thermal Paper Receipt */}
          <div className="lg:col-span-6 bg-white border-2 border-black p-6 sm:p-7 font-mono text-xs shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] relative flex flex-col justify-between select-text">
            
            {/* Top Receipt Serration */}
            <div className="text-center pb-4 border-b-2 border-dashed border-zinc-400">
              <div className="text-xs tracking-widest text-zinc-500 font-bold uppercase mb-1">
                ========================================
              </div>
              <div className="text-lg sm:text-xl font-extrabold tracking-widest uppercase text-black">
                * * * AGENT RECEIPT * * *
              </div>
              <div className="text-xs tracking-widest text-zinc-500 font-bold uppercase mt-1">
                ========================================
              </div>
              <div className="flex justify-between items-center text-[10px] text-zinc-600 mt-3 font-semibold">
                <span>STORE: ~/.config (~/local)</span>
                <span>DATE: {new Date().toISOString().split('T')[0]}</span>
              </div>
            </div>

            {/* Receipt Table Items with Dotted Leaders */}
            <div className="py-5 space-y-3 my-auto text-xs">
              <div className="flex justify-between items-baseline">
                <span className="text-zinc-600 uppercase font-medium">TOTAL EXHAUST TOKENS</span>
                <span className="text-zinc-400 mx-2 flex-1 border-b border-dotted border-zinc-300"></span>
                <span className="font-bold text-black text-sm">{formatTokens(totalTokensNum)}</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-zinc-600 uppercase font-medium">ESTIMATED EXPENDITURE</span>
                <span className="text-zinc-400 mx-2 flex-1 border-b border-dotted border-zinc-300"></span>
                <span className="font-bold text-black text-sm">{formatUSD(estimatedCostUSD)} USD</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-zinc-600 uppercase font-medium">INDEXED SESSIONS</span>
                <span className="text-zinc-400 mx-2 flex-1 border-b border-dotted border-zinc-300"></span>
                <span className="font-bold text-black">{stats.total_sessions.toLocaleString()}</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-zinc-600 uppercase font-medium">DETECTED AGENTS</span>
                <span className="text-zinc-400 mx-2 flex-1 border-b border-dotted border-zinc-300"></span>
                <span className="font-bold text-black">
                  {Object.keys(stats.sessions_by_adapter || {}).length} Adapters ({Object.keys(stats.models_usage_count || {}).length} Models)
                </span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-zinc-600 uppercase font-medium">VERIFIED OUTCOMES</span>
                <span className="text-zinc-400 mx-2 flex-1 border-b border-dotted border-zinc-300"></span>
                <span className="font-bold text-black">
                  {stats.verified_outcomes_count.toLocaleString()} ({verifiedPercent}%)
                </span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-zinc-600 uppercase font-medium">PRIMARY ADAPTER</span>
                <span className="text-zinc-400 mx-2 flex-1 border-b border-dotted border-zinc-300"></span>
                <span className="font-bold text-black">
                  {topAdapterInfo?.name || 'Claude Code'} ({topAdapterPercent}%)
                </span>
              </div>
            </div>

            {/* Financial Summary Box */}
            <div className="border-t-2 border-dashed border-zinc-400 pt-4 mb-4">
              <div className="flex justify-between text-xs font-bold text-black py-1">
                <span>SUBTOTAL (RAW EXHAUST):</span>
                <span>{formatUSD(estimatedCostUSD)}</span>
              </div>
              <div className="flex justify-between text-xs text-zinc-500 py-0.5">
                <span>DATA SENT TO CLOUD:</span>
                <span>$0.00 (0 BYTES)</span>
              </div>
              <div className="flex justify-between text-sm font-extrabold text-black pt-2 border-t border-black mt-1">
                <span>FINAL ESTIMATED COST:</span>
                <span>{formatUSD(estimatedCostUSD)}</span>
              </div>
            </div>

            {/* Bottom Barcode Stamp & Actions */}
            <div className="pt-3 border-t-2 border-dashed border-zinc-400 text-center">
              <div className="font-mono text-xs tracking-widest text-black select-none font-bold py-1 overflow-hidden">
                ||| | ||||| || |||||| | |||| ||| ||||||| ||| |||| | ||||
              </div>
              <div className="flex justify-between items-center text-[10px] text-zinc-500 mt-2 font-mono">
                <span>AUTH: SHA256:7f83d7...</span>
                <span>[VERIFIED LOCAL ONLY]</span>
              </div>

              <div className="mt-4 pt-3 border-t border-zinc-200 flex items-center justify-center space-x-3">
                <button
                  onClick={handleCopyReceiptText}
                  className="px-3 py-1.5 bg-black hover:bg-zinc-800 text-white text-xs font-mono font-bold border border-black transition-colors shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] active:translate-x-0.5 active:translate-y-0.5"
                >
                  {receiptCopied ? '✓ Copied Plain Text' : 'Copy ASCII Receipt'}
                </button>
              </div>
            </div>

          </div>

        </div>

      </div>
    </section>
  );
};
