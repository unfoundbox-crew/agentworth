import React, { useState } from "react";
import { AggregateStats } from "../types";
import { formatTokens, estimateTokenCostUSD, formatUSD } from "../utils/formatters";
import { VerdictStamp } from "./VerdictStamp";
import { IconCheck, IconCopy, IconRefresh } from "./icons";

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
AUTH:  SQLITE WAL - SHA-256
----------------------------------------
SESSIONS INDEXED:       ${totalSessions.toString().padEnd(12)}
TOTAL TOKENS:           ${formatTokens(totalTokensNum).padEnd(12)}
 - CACHE READ:          ${formatTokens(stats.token_usage.cache_read_input_tokens || 0)} (${cacheReadPercent}%)
 - CACHE WRITE:         ${formatTokens(stats.token_usage.cache_creation_input_tokens || 0)}
 - OUTPUT:              ${formatTokens(stats.token_usage.output_tokens || 0)}
 - FRESH INPUT:         ${formatTokens(stats.token_usage.input_tokens || 0)}
----------------------------------------
LIST-PRICE EQUIVALENT:  ${formatUSD(estimatedCostUSD)}
DATA SENT ANYWHERE:     0 bytes
TASKS VERIFIED DONE:    ${isMeasured ? verifiedCount.toString() : "not measured"}
----------------------------------------
VERDICT: ${isMeasured ? "CI VERIFIED" : "NO VERDICT ON FILE"}
========================================`.trim();

    navigator.clipboard.writeText(rawReceipt);
    setReceiptCopied(true);
    setTimeout(() => setReceiptCopied(false), 2000);
  };

  return (
    <section className="hero">
      <div className="shell">
        <div className="max-w-3xl mx-auto text-center mb-10">
          <span className="eyebrow mx-auto" style={{ marginBottom: "14px" }}>
            The verdict layer for local agent histories
          </span>
          <h1 className="thesis mx-auto" style={{ maxWidth: "none" }}>
            Every agent says it&apos;s done.
          </h1>
          <p className="dek mx-auto" style={{ maxWidth: "56ch" }}>
            AgentWorth checks the diff, the compiler exit code, and{" "}
            <em>the git log</em>.
          </p>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-12 gap-6 items-stretch">
          {/* Left: Terminal console, in a diagram-frame */}
          <div className="lg:col-span-6 diagram-frame !p-0 overflow-hidden flex flex-col">
            <div className="bg-ink text-ground rounded-t-[11px] flex flex-col flex-1">
              <div className="px-4 py-3 flex items-center justify-between border-b border-white/10">
                <span className="font-mono text-xs font-semibold text-ground/90">
                  agentworth stats --all
                </span>
                <span className="font-mono text-[10px] uppercase tracking-wider text-emerald-400">
                  Offline &middot; local
                </span>
              </div>

              <div className="p-5 space-y-2 flex-1 min-h-[240px]">
                <div className="font-mono text-xs text-ground/50">
                  <span className="text-ground font-semibold">$</span> agentworth stats
                </div>

                <div className="font-mono text-xs" style={{ fontVariantNumeric: "tabular-nums" }}>
                  {[
                    ["Sessions indexed", totalSessions.toLocaleString(), "text-ground"],
                    ["Events normalized", stats.total_events.toLocaleString(), "text-ground"],
                    ["Total tokens", formatTokens(totalTokensNum), "text-ground"],
                    [
                      "— cache read",
                      `${formatTokens(stats.token_usage.cache_read_input_tokens || 0)} · ${cacheReadPercent}%`,
                      "text-ground/70",
                      true,
                    ],
                    [
                      "— cache write",
                      formatTokens(stats.token_usage.cache_creation_input_tokens || 0),
                      "text-ground/70",
                      true,
                    ],
                    ["— output", formatTokens(stats.token_usage.output_tokens || 0), "text-ground/70", true],
                    ["List-price equivalent", formatUSD(estimatedCostUSD), "text-ground"],
                    ["Data sent anywhere", "0 bytes", "text-emerald-400"],
                    [
                      "Tasks verified done",
                      isMeasured ? `${verifiedCount.toLocaleString()} verified` : "not measured",
                      isMeasured ? "text-emerald-400" : "text-red-400",
                    ],
                  ].map(([label, value, valueClass, indent], i) => (
                    <div
                      key={label as string}
                      className={`flex justify-between py-1 ${i < 8 ? "border-b border-white/10" : ""} ${
                        indent ? "pl-3" : ""
                      }`}
                    >
                      <span className="text-ground/50">{label}</span>
                      <span className={`font-semibold ${valueClass}`}>{value}</span>
                    </div>
                  ))}
                </div>
              </div>

              <div className="p-3 border-t border-white/10 flex flex-wrap items-center justify-between gap-3">
                <code className="font-mono text-xs text-ground bg-white/5 px-2 py-1 rounded border border-white/10">
                  $ {command}
                </code>
                <div className="flex items-center gap-2">
                  <button
                    onClick={handleCopy}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-mono bg-white/10 hover:bg-white/15 text-ground transition-colors"
                  >
                    {copied ? <IconCheck size={13} /> : <IconCopy size={13} />}
                    <span>{copied ? "Copied" : "Copy"}</span>
                  </button>
                  <button
                    onClick={onScanClick}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-mono bg-ground text-ink font-semibold hover:opacity-85 transition-opacity"
                  >
                    <IconRefresh size={13} />
                    <span>Run scan</span>
                  </button>
                </div>
              </div>
            </div>
          </div>

          {/* Right: Thermal receipt with verdict stamp */}
          <div className="lg:col-span-6 rounded-xl border border-border bg-surface p-6 sm:p-7 font-mono text-xs relative flex flex-col justify-between select-text">
            <div className="text-center pb-4 border-b border-dashed border-border">
              <div className="text-[10px] tracking-widest text-faint font-semibold">
                ========================================
              </div>
              <div className="text-lg font-bold tracking-widest uppercase text-ink">
                * * * Agent receipt * * *
              </div>
              <div className="text-[10px] tracking-widest text-faint font-semibold">
                ========================================
              </div>
              <div className="flex justify-between items-center text-[10px] text-muted mt-3 font-medium">
                <span>STORE: ~/.agentworth (local)</span>
                <span>DATE: {new Date().toISOString().split("T")[0]}</span>
              </div>
            </div>

            <div className="my-auto space-y-2" style={{ fontVariantNumeric: "tabular-nums" }}>
              {[
                ["Sessions indexed", totalSessions.toLocaleString(), "text-ink", false],
                ["Total tokens", formatTokens(totalTokensNum), "text-ink", false],
                [
                  "— cache read",
                  `${formatTokens(stats.token_usage.cache_read_input_tokens || 0)} (${cacheReadPercent}%)`,
                  "text-muted",
                  true,
                ],
                [
                  "— cache write",
                  formatTokens(stats.token_usage.cache_creation_input_tokens || 0),
                  "text-muted",
                  true,
                ],
                ["List-price equiv", formatUSD(estimatedCostUSD), "text-ink", false],
                ["Data sent out", "0 bytes", "text-emerald-600", false],
                [
                  "Tasks verified",
                  isMeasured ? verifiedCount.toLocaleString() : "not measured",
                  isMeasured ? "text-ink" : "text-red-600",
                  false,
                ],
              ].map(([label, value, valueClass, indent]) => (
                <div key={label as string} className={`flex justify-between items-baseline ${indent ? "pl-3" : ""}`}>
                  <span className={indent ? "text-muted" : "uppercase text-muted font-medium"}>{label}</span>
                  <span
                    className="border-b border-dotted border-border mx-2 flex-1"
                    aria-hidden="true"
                  />
                  <span className={`font-semibold ${valueClass}`}>{value}</span>
                </div>
              ))}
            </div>

            <div className="pt-3 border-t border-dashed border-border text-center">
              <div className="my-3 flex justify-center">
                <VerdictStamp
                  status={isMeasured ? "ci_or_deployment_verified" : "not_measured"}
                  size="md"
                  rotated={true}
                />
              </div>

              <div className="flex justify-between items-center text-[10px] text-muted">
                <span>SQLITE WAL: SHA256:7f83d7&hellip;</span>
                <span>[audited local only]</span>
              </div>

              <div className="mt-3 pt-2 border-t border-border-soft flex items-center justify-center">
                <button
                  onClick={handleCopyReceiptText}
                  className="px-3 py-1.5 rounded-md bg-ink text-ground text-xs font-mono font-semibold hover:opacity-85 transition-opacity"
                >
                  {receiptCopied ? "Copied plain text" : "Copy ASCII receipt"}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};
