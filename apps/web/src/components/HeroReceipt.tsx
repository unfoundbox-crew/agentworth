import React, { useState } from "react";
import { Copy, Check, RefreshCw } from "lucide-react";
import { AggregateStats } from "../types";
import { formatTokens, estimateTokenCostUSD, formatUSD } from "../utils/formatters";
import { VerdictStamp } from "./VerdictStamp";

interface HeroReceiptProps {
  stats: AggregateStats;
  onScanClick: () => void;
}

export const HeroReceipt: React.FC<HeroReceiptProps> = ({ stats, onScanClick }) => {
  const [copied, setCopied] = useState(false);
  const [receiptCopied, setReceiptCopied] = useState(false);

  const command = "npx agentworth";

  const handleCopy = () => {
    navigator.clipboard.writeText(command);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const totalTokensNum =
    (stats.token_usage.input_tokens || 0) +
    (stats.token_usage.output_tokens || 0) +
    (stats.token_usage.cache_read_input_tokens || 0) +
    (stats.token_usage.cache_creation_input_tokens || 0);

  const estimatedCostUSD = estimateTokenCostUSD(totalTokensNum, stats.models_usage_count);

  const cacheReadPercent =
    totalTokensNum > 0
      ? (((stats.token_usage.cache_read_input_tokens || 0) / totalTokensNum) * 100).toFixed(1)
      : "0.0";

  const verifiedCount = stats.verified_outcomes_count || 0;
  const totalSessions = stats.total_sessions || 0;
  const isMeasured = totalSessions > 0 && verifiedCount > 0;

  const handleCopyReceiptText = () => {
    const rawReceipt = `
========================================
       * * * AGENT RECEIPT * * *
========================================
STORE: LOCAL DISK (~/.agentworth)
DATE:  ${new Date().toISOString().split("T")[0]}
AUTH:  SQLITE WAL · SHA-256
----------------------------------------
SESSIONS INDEXED:       ${totalSessions.toString().padEnd(12)}
TOTAL TOKENS:           ${formatTokens(totalTokensNum).padEnd(12)}
 — CACHE READ:          ${formatTokens(stats.token_usage.cache_read_input_tokens || 0)} (${cacheReadPercent}%)
 — CACHE WRITE:         ${formatTokens(stats.token_usage.cache_creation_input_tokens || 0)}
 — OUTPUT:              ${formatTokens(stats.token_usage.output_tokens || 0)}
 — FRESH INPUT:         ${formatTokens(stats.token_usage.input_tokens || 0)}
----------------------------------------
LIST-PRICE EQUIVALENT:  ${formatUSD(estimatedCostUSD)}
DATA SENT ANYWHERE:     0 bytes
TASKS VERIFIED DONE:    ${isMeasured ? verifiedCount.toString() : "not measured"}
----------------------------------------
||| | ||||| || |||||| | |||| ||| |||||||
VERDICT: ${isMeasured ? "CI VERIFIED" : "NO VERDICT ON FILE"}
========================================`.trim();

    navigator.clipboard.writeText(rawReceipt);
    setReceiptCopied(true);
    setTimeout(() => setReceiptCopied(false), 2000);
  };

  return (
    <section className="py-8 sm:py-12 border-b-2 border-black dark:border-white bg-[#fbfbfb] dark:bg-[#0a0a0c]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        
        {/* Header Tagline */}
        <div className="text-center max-w-3xl mx-auto mb-8">
          <div className="inline-flex items-center space-x-2 border-2 border-black dark:border-white px-3 py-1 bg-white dark:bg-neutral-900 mb-3 text-xs font-mono font-bold tracking-wider uppercase shadow-[3px_3px_0px_0px_rgba(0,0,0,1)] dark:shadow-[3px_3px_0px_0px_rgba(255,255,255,1)]">
            <span className="w-2 h-2 bg-black dark:bg-white animate-pulse" />
            <span>THE VERDICT LAYER FOR LOCAL AGENT HISTORIES</span>
          </div>
          <h1 className="text-3xl sm:text-4xl lg:text-5xl font-mono font-extrabold tracking-tight text-black dark:text-white mb-2">
            Every agent says it&apos;s done.
          </h1>
          <p className="text-sm sm:text-base font-mono text-neutral-600 dark:text-neutral-400 max-w-2xl mx-auto leading-relaxed">
            AgentWorth checks the diff, the compiler exit code, and the git log.
          </p>
        </div>

        {/* 2-Column Physical Grid */}
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-stretch">
          
          {/* Left: Terminal Console */}
          <div className="lg:col-span-6 bg-black text-white border-2 border-black dark:border-white font-mono text-xs shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)] flex flex-col justify-between">
            {/* Titlebar */}
            <div className="bg-zinc-900 border-b border-zinc-800 px-4 py-3 flex items-center justify-between">
              <div className="flex items-center space-x-2">
                <div className="w-3 h-3 border border-zinc-700 bg-zinc-800" />
                <div className="w-3 h-3 border border-zinc-700 bg-zinc-800" />
                <div className="w-3 h-3 border border-zinc-700 bg-zinc-800" />
                <span className="text-xs text-zinc-300 ml-2 font-mono font-semibold">agentworth stats --all</span>
              </div>
              <span className="text-[10px] text-emerald-400 font-mono uppercase tracking-wider">OFFLINE · LOCAL</span>
            </div>

            {/* Real Stats Console */}
            <div className="p-5 space-y-2.5 overflow-x-auto min-h-[260px] font-mono text-xs">
              <div className="text-zinc-400">
                <span className="text-white font-bold">$</span> agentworth stats
              </div>

              <div className="text-zinc-300 pt-1 space-y-1.5">
                <div className="flex justify-between border-b border-zinc-900 pb-1">
                  <span className="text-zinc-400">Sessions indexed:</span>
                  <span className="text-white font-bold">{totalSessions.toLocaleString()}</span>
                </div>
                <div className="flex justify-between border-b border-zinc-900 pb-1">
                  <span className="text-zinc-400">Events normalized:</span>
                  <span className="text-white font-bold">{stats.total_events.toLocaleString()}</span>
                </div>
                <div className="flex justify-between border-b border-zinc-900 pb-1">
                  <span className="text-zinc-400">Total tokens:</span>
                  <span className="text-white font-bold">{formatTokens(totalTokensNum)}</span>
                </div>
                <div className="flex justify-between pl-4 text-zinc-400">
                  <span>— cache read:</span>
                  <span className="text-zinc-200">{formatTokens(stats.token_usage.cache_read_input_tokens || 0)} · {cacheReadPercent}%</span>
                </div>
                <div className="flex justify-between pl-4 text-zinc-400">
                  <span>— cache write:</span>
                  <span className="text-zinc-200">{formatTokens(stats.token_usage.cache_creation_input_tokens || 0)}</span>
                </div>
                <div className="flex justify-between pl-4 text-zinc-400">
                  <span>— output:</span>
                  <span className="text-zinc-200">{formatTokens(stats.token_usage.output_tokens || 0)}</span>
                </div>
                <div className="flex justify-between border-b border-zinc-900 pb-1 pt-1">
                  <span className="text-zinc-400">List-price equivalent:</span>
                  <span className="text-white font-bold">{formatUSD(estimatedCostUSD)}</span>
                </div>
                <div className="flex justify-between border-b border-zinc-900 pb-1">
                  <span className="text-zinc-400">Data sent anywhere:</span>
                  <span className="text-emerald-400 font-bold">0 bytes</span>
                </div>
                <div className="flex justify-between pt-1">
                  <span className="text-zinc-400">Tasks verified done:</span>
                  <span className={isMeasured ? "text-emerald-400 font-bold" : "text-red-400 font-bold"}>
                    {isMeasured ? `${verifiedCount.toLocaleString()} verified` : "not measured"}
                  </span>
                </div>
              </div>
            </div>

            {/* Quick Action Bar */}
            <div className="bg-zinc-900 border-t border-zinc-800 p-3 flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center space-x-2 font-mono text-xs text-zinc-300">
                <span className="text-zinc-500 font-bold">$</span>
                <code className="text-white font-bold bg-black px-2 py-0.5 border border-zinc-700">{command}</code>
              </div>
              <div className="flex items-center space-x-2">
                <button
                  onClick={handleCopy}
                  className="flex items-center space-x-1.5 px-3 py-1.5 text-xs font-mono bg-zinc-800 hover:bg-zinc-700 text-white border border-zinc-600 transition-colors"
                >
                  {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                  <span>{copied ? "Copied" : "Copy"}</span>
                </button>
                <button
                  onClick={onScanClick}
                  className="flex items-center space-x-1.5 px-3 py-1.5 text-xs font-mono bg-white hover:bg-zinc-200 text-black font-bold border border-white transition-colors"
                >
                  <RefreshCw className="w-3.5 h-3.5" />
                  <span>Run Scan</span>
                </button>
              </div>
            </div>
          </div>

          {/* Right: Thermal Receipt with Verdict Stamp */}
          <div className="lg:col-span-6 bg-white dark:bg-[#151518] border-2 border-black dark:border-white p-6 sm:p-7 font-mono text-xs shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)] relative flex flex-col justify-between select-text">
            
            {/* Top Receipt Header */}
            <div className="text-center pb-4 border-b-2 border-dashed border-neutral-300 dark:border-neutral-700">
              <div className="text-xs tracking-widest text-neutral-400 font-bold uppercase mb-1">
                ========================================
              </div>
              <div className="text-lg sm:text-xl font-extrabold tracking-widest uppercase text-black dark:text-white">
                * * * AGENT RECEIPT * * *
              </div>
              <div className="text-xs tracking-widest text-neutral-400 font-bold uppercase mt-1">
                ========================================
              </div>
              <div className="flex justify-between items-center text-[10px] text-neutral-500 mt-3 font-semibold">
                <span>STORE: ~/.agentworth (LOCAL)</span>
                <span>DATE: {new Date().toISOString().split("T")[0]}</span>
              </div>
            </div>

            {/* Receipt Table Items with Dotted Leaders */}
            <div className="py-4 space-y-2.5 my-auto text-xs">
              <div className="flex justify-between items-baseline">
                <span className="text-neutral-600 dark:text-neutral-400 uppercase font-medium">SESSIONS INDEXED</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span className="font-bold text-black dark:text-white">{totalSessions.toLocaleString()}</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-neutral-600 dark:text-neutral-400 uppercase font-medium">TOTAL TOKENS</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span className="font-bold text-black dark:text-white">{formatTokens(totalTokensNum)}</span>
              </div>

              <div className="flex justify-between items-baseline text-[11px] text-neutral-500">
                <span className="pl-3">— CACHE READ</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span>{formatTokens(stats.token_usage.cache_read_input_tokens || 0)} ({cacheReadPercent}%)</span>
              </div>

              <div className="flex justify-between items-baseline text-[11px] text-neutral-500">
                <span className="pl-3">— CACHE WRITE</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span>{formatTokens(stats.token_usage.cache_creation_input_tokens || 0)}</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-neutral-600 dark:text-neutral-400 uppercase font-medium">LIST-PRICE EQUIV</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span className="font-bold text-black dark:text-white">{formatUSD(estimatedCostUSD)}</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-neutral-600 dark:text-neutral-400 uppercase font-medium">DATA SENT OUT</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span className="font-bold text-emerald-600 dark:text-emerald-400">0 BYTES</span>
              </div>

              <div className="flex justify-between items-baseline">
                <span className="text-neutral-600 dark:text-neutral-400 uppercase font-medium">TASKS VERIFIED</span>
                <span className="text-neutral-300 dark:text-neutral-700 mx-2 flex-1 border-b border-dotted border-neutral-300 dark:border-neutral-700" />
                <span className={isMeasured ? "font-bold text-black dark:text-white" : "font-bold text-red-600 dark:text-red-400"}>
                  {isMeasured ? `${verifiedCount.toLocaleString()}` : "not measured"}
                </span>
              </div>
            </div>

            {/* Barcode & The Verdict Stamp */}
            <div className="pt-3 border-t-2 border-dashed border-neutral-300 dark:border-neutral-700 text-center">
              <div className="font-mono text-xs tracking-widest text-black dark:text-white select-none font-bold py-1 overflow-hidden">
                ||| | ||||| || |||||| | |||| ||| ||||||| ||| |||| | ||||
              </div>
              
              <div className="my-3 flex justify-center">
                <VerdictStamp
                  status={isMeasured ? "ci_or_deployment_verified" : "not_measured"}
                  size="md"
                  rotated={true}
                />
              </div>

              <div className="flex justify-between items-center text-[10px] text-neutral-500 mt-1 font-mono">
                <span>SQLITE WAL: SHA256:7f83d7...</span>
                <span>[AUDITED LOCAL ONLY]</span>
              </div>

              <div className="mt-3 pt-2 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-center">
                <button
                  onClick={handleCopyReceiptText}
                  className="px-3 py-1.5 bg-black hover:bg-neutral-800 dark:bg-white dark:hover:bg-neutral-200 text-white dark:text-black text-xs font-mono font-bold border border-black dark:border-white transition-colors shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] dark:shadow-[2px_2px_0px_0px_rgba(255,255,255,1)]"
                >
                  {receiptCopied ? "✓ Copied Plain Text" : "Copy ASCII Receipt"}
                </button>
              </div>
            </div>

          </div>

        </div>

      </div>
    </section>
  );
};
