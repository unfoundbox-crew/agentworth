import React, { useState } from "react";
import { formatUSD, formatTokens } from "../utils/formatters";
import { AlertCircle, Zap } from "lucide-react";

interface CacheCliffWidgetProps {
  className?: string;
}

export const CacheCliffWidget: React.FC<CacheCliffWidgetProps> = ({ className = "" }) => {
  const [switchTurn, setSwitchTurn] = useState<number>(28);

  const totalTurns = 40;
  const growthPerTurn = 8000; // 8k tokens context growth per turn
  const outputPerTurn = 1200; // 1.2k output per turn

  // Model Pricing (Anthropic Sonnet class standard list prices)
  const cacheWriteRatePerMTok = 3.0 * 1.25; // $3.75 / MTok (1.25x)
  const cacheReadRatePerMTok = 3.0 * 0.1; // $0.30 / MTok (0.1x)
  const outputRatePerMTok = 15.0; // $15.00 / MTok

  // Compute baseline (no switch: turn 1 writes initial prompt, turns 2-40 read previous context + write delta)
  const calculateTurnCost = (turn: number, isSwitch: boolean) => {
    const contextSize = turn * growthPerTurn;
    const outputCost = (outputPerTurn / 1000000) * outputRatePerMTok;

    if (turn === 1) {
      // Turn 1 writes initial context
      const inputCost = (contextSize / 1000000) * cacheWriteRatePerMTok;
      return { cost: inputCost + outputCost, isSpike: false, contextSize };
    }

    if (isSwitch) {
      // Entire accumulated context invalidated & rewritten at 1.25x
      const inputCost = (contextSize / 1000000) * cacheWriteRatePerMTok;
      return { cost: inputCost + outputCost, isSpike: true, contextSize };
    }

    // Normal warm turn: previous context read at 0.1x, new turn delta written at 1.25x
    const prevContext = (turn - 1) * growthPerTurn;
    const delta = growthPerTurn;
    const readCost = (prevContext / 1000000) * cacheReadRatePerMTok;
    const writeDeltaCost = (delta / 1000000) * cacheWriteRatePerMTok;
    return { cost: readCost + writeDeltaCost + outputCost, isSpike: false, contextSize };
  };

  let noSwitchTotal = 0;
  for (let t = 1; t <= totalTurns; t++) {
    noSwitchTotal += calculateTurnCost(t, false).cost;
  }

  let withSwitchTotal = 0;
  const turnBreakdown: { turn: number; cost: number; isSpike: boolean; contextSize: number }[] = [];
  for (let t = 1; t <= totalTurns; t++) {
    const res = calculateTurnCost(t, t === switchTurn);
    withSwitchTotal += res.cost;
    turnBreakdown.push({ turn: t, ...res });
  }

  const switchTurnNormalCost = calculateTurnCost(switchTurn, false).cost;
  const switchTurnActualCost = calculateTurnCost(switchTurn, true).cost;
  const turnMultiplier = (switchTurnActualCost / switchTurnNormalCost).toFixed(1);
  const contextRewritten = switchTurn * growthPerTurn;
  const maxCost = Math.max(...turnBreakdown.map((b) => b.cost));

  return (
    <div className={`panel ${className}`}>
      {/* Panel head */}
      <div className="panel-head">
        <div className="panel-kicker">
          <span className="tag-pill">The cache cliff</span>
          <span className="status-pill is-bad">
            <span className="dot" />
            12.5&times; price swing
          </span>
        </div>
        <h2>The Anthropic cache invalidation penalty</h2>
        <p>
          Switching models or reasoning effort mid-session destroys prefix KV-cache. The turn you switch on pays for
          the entire conversation again.
        </p>
      </div>

      {/* Interactive turn slider */}
      <div className="rounded-xl border border-border bg-surface p-4 sm:p-5 mb-6">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 mb-3">
          <label htmlFor="turn-slider" className="text-xs font-mono font-semibold text-ink flex items-center gap-2">
            <Zap className="w-3.5 h-3.5 text-accent" />
            <span>Switch model or effort at turn</span>
            <span className="px-2 py-0.5 rounded-md bg-ink text-ground font-mono font-bold text-xs">
              {switchTurn} / {totalTurns}
            </span>
          </label>
          <div className="text-xs text-muted font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
            Accumulated context: <span className="font-semibold text-ink">{formatTokens(contextRewritten)}</span>
          </div>
        </div>

        <input
          id="turn-slider"
          type="range"
          min={1}
          max={totalTurns}
          value={switchTurn}
          onChange={(e) => setSwitchTurn(parseInt(e.target.value, 10))}
          className="w-full h-1.5 rounded-full appearance-none cursor-pointer bg-border-soft"
          style={{ accentColor: "var(--mv-accent)" }}
        />

        <div className="flex justify-between text-[10px] text-faint mt-2 font-mono">
          <span>Turn 1 (session start)</span>
          <span className="text-danger font-semibold">&#9650; switch at turn {switchTurn}</span>
          <span>Turn 40 (session end)</span>
        </div>
      </div>

      {/* Comparison stats */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-6">
        <div className="rounded-xl border border-border p-3.5">
          <div className="text-[10px] font-mono font-medium text-faint uppercase mb-1">Session, no switch</div>
          <div className="text-2xl font-bold text-ink font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
            {formatUSD(noSwitchTotal)}
          </div>
          <div className="text-[10px] text-success mt-1 font-medium">Warm prefix cache (97.2% hit)</div>
        </div>

        <div className="rounded-xl border border-danger-border bg-danger-soft p-3.5">
          <div className="text-[10px] font-mono font-medium text-danger uppercase mb-1">With 1 switch</div>
          <div className="text-2xl font-bold text-danger font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
            {formatUSD(withSwitchTotal)}
          </div>
          <div className="text-[10px] text-danger mt-1 font-medium">
            + {formatUSD(withSwitchTotal - noSwitchTotal)} penalty
          </div>
        </div>

        <div className="rounded-xl border border-border p-3.5">
          <div className="text-[10px] font-mono font-medium text-faint uppercase mb-1">That single turn costs</div>
          <div className="text-2xl font-bold text-ink font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
            {turnMultiplier}&times;
          </div>
          <div className="text-[10px] text-muted mt-1 font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
            {formatUSD(switchTurnActualCost)} vs {formatUSD(switchTurnNormalCost)}
          </div>
        </div>

        <div className="rounded-xl border border-border p-3.5">
          <div className="text-[10px] font-mono font-medium text-faint uppercase mb-1">Context re-written</div>
          <div className="text-2xl font-bold text-ink font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
            {formatTokens(contextRewritten)}
          </div>
          <div className="text-[10px] text-muted mt-1">At $3.75/MTok cache-write</div>
        </div>
      </div>

      {/* Turn-by-turn bar chart */}
      <div className="rounded-xl border border-border bg-surface p-4 mb-6">
        <div className="flex justify-between text-xs font-mono mb-3">
          <span className="font-semibold text-ink">Per-turn cost profile (40 turns)</span>
          <span className="text-danger">red spike = cache re-write cliff</span>
        </div>

        <div className="h-28 flex items-end gap-1 sm:gap-1.5 pt-2 pb-1 border-b border-border">
          {turnBreakdown.map((item) => {
            const heightPercent = Math.max(8, (item.cost / maxCost) * 100);

            return (
              <div key={item.turn} className="flex-1 flex flex-col items-center justify-end h-full group relative">
                <div className="opacity-0 group-hover:opacity-100 pointer-events-none absolute bottom-full mb-2 z-20 bg-ink text-ground text-[10px] font-mono p-1.5 rounded whitespace-nowrap shadow-lg">
                  <div>
                    Turn {item.turn}: {formatUSD(item.cost)}
                  </div>
                  <div>Context: {formatTokens(item.contextSize)}</div>
                  {item.isSpike && <div className="text-danger font-semibold">Invalidation spike</div>}
                </div>

                <div
                  style={{ height: `${heightPercent}%` }}
                  className={`w-full rounded-t-sm transition-all ${
                    item.isSpike ? "bg-danger" : item.turn < switchTurn ? "bg-ink" : "bg-faint"
                  }`}
                />
              </div>
            );
          })}
        </div>

        <div className="flex justify-between text-[10px] text-faint mt-2 font-mono">
          <span>Turn 1</span>
          <span>Turn 10</span>
          <span>Turn 20</span>
          <span>Turn 30</span>
          <span>Turn 40</span>
        </div>
      </div>

      {/* Explanatory formula */}
      <div className="rounded-xl border border-border p-4">
        <div className="flex items-center gap-2 font-semibold text-sm mb-2 text-ink">
          <AlertCircle className="w-4 h-4 text-muted" />
          <span>Why this happens under the hood</span>
        </div>
        <p className="text-sm text-text leading-relaxed mb-3">
          Anthropic prompt caching is an exact byte-for-byte prefix match. When you change model (e.g. from Claude
          Sonnet to Claude Opus) or change reasoning effort, the system prompt prefix is altered. All{" "}
          <strong className="text-ink font-semibold">{formatTokens(contextRewritten)}</strong> previously cached
          tokens can no longer be read at <strong className="text-ink font-semibold">$0.30/MTok</strong> (0.1&times;)
          and must be recreated at <strong className="text-ink font-semibold">$3.75/MTok</strong> (1.25&times;).
        </p>
        <div className="quote-block">
          cache_invalidation_delta = context_tokens &times; ($3.75/MTok &minus; $0.30/MTok) ={" "}
          {formatUSD((contextRewritten / 1000000) * 3.45)}
        </div>
      </div>
    </div>
  );
};
