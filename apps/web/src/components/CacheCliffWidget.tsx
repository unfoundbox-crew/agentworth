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

  return (
    <div
      className={`border-2 border-black dark:border-white bg-white dark:bg-[#121215] text-black dark:text-white p-6 sm:p-7 font-mono shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)] ${className}`}
    >
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-5 border-b-2 border-dashed border-neutral-300 dark:border-neutral-700">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <span className="text-xs font-bold uppercase tracking-widest text-neutral-500 dark:text-neutral-400">
              § THE CACHE CLIFF
            </span>
            <span className="text-[10px] px-2 py-0.5 border border-red-600 dark:border-red-500 font-bold bg-red-50 dark:bg-red-950/40 text-red-700 dark:text-red-400">
              12.5× PRICE SWING
            </span>
          </div>
          <h2 className="text-xl sm:text-2xl font-extrabold tracking-tight">
            The Anthropic Cache Invalidation Penalty
          </h2>
          <p className="text-xs text-neutral-600 dark:text-neutral-400 mt-1 font-sans">
            Switching models or reasoning effort mid-session destroys prefix KV-cache. The turn you switch on pays for the entire conversation again.
          </p>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <div className="text-right">
            <div className="text-[10px] text-neutral-500 uppercase font-bold">Write vs Read Delta</div>
            <div className="text-sm font-black text-red-600 dark:text-red-400">1.25× vs 0.1×</div>
          </div>
        </div>
      </div>

      {/* Interactive Turn Slider Control */}
      <div className="my-6 p-4 sm:p-5 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/50">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 mb-3">
          <label htmlFor="turn-slider" className="text-xs font-bold uppercase tracking-wider text-black dark:text-white flex items-center gap-2">
            <Zap className="w-3.5 h-3.5 text-amber-500 fill-amber-500" />
            <span>Switch Model or Effort at Turn:</span>
            <span className="px-2 py-0.5 bg-black dark:bg-white text-white dark:text-black font-black text-xs">
              Turn {switchTurn} / {totalTurns}
            </span>
          </label>
          <div className="text-xs text-neutral-500 dark:text-neutral-400">
            Accumulated Context: <span className="font-bold text-black dark:text-white">{formatTokens(contextRewritten)}</span>
          </div>
        </div>

        <input
          id="turn-slider"
          type="range"
          min={1}
          max={totalTurns}
          value={switchTurn}
          onChange={(e) => setSwitchTurn(parseInt(e.target.value, 10))}
          className="w-full h-2 bg-neutral-300 dark:bg-neutral-700 rounded-none appearance-none cursor-pointer accent-black dark:accent-white"
        />

        <div className="flex justify-between text-[10px] text-neutral-500 mt-2 font-mono">
          <span>Turn 1 (Session start)</span>
          <span className="text-red-600 dark:text-red-400 font-bold">▲ Model Switch at Turn {switchTurn}</span>
          <span>Turn 40 (Session end)</span>
        </div>
      </div>

      {/* Real Arithmetic Comparison Cards */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3.5 mb-6">
        {/* Card 1: Baseline */}
        <div className="p-3.5 border-2 border-neutral-300 dark:border-neutral-800 bg-white dark:bg-[#151518]">
          <div className="text-[10px] font-bold text-neutral-500 uppercase mb-1">
            Session, No Switch
          </div>
          <div className="text-2xl font-black text-black dark:text-white">
            {formatUSD(noSwitchTotal)}
          </div>
          <div className="text-[10px] text-emerald-600 dark:text-emerald-400 mt-1 font-semibold">
            Warm prefix cache (97.2% hit)
          </div>
        </div>

        {/* Card 2: With Switch */}
        <div className="p-3.5 border-2 border-red-600 dark:border-red-500 bg-red-50/20 dark:bg-red-950/20">
          <div className="text-[10px] font-bold text-red-600 dark:text-red-400 uppercase mb-1">
            With 1 Switch
          </div>
          <div className="text-2xl font-black text-red-700 dark:text-red-400">
            {formatUSD(withSwitchTotal)}
          </div>
          <div className="text-[10px] text-red-600 dark:text-red-400 mt-1 font-semibold">
            + {formatUSD(withSwitchTotal - noSwitchTotal)} penalty
          </div>
        </div>

        {/* Card 3: Switch Turn Multiplier */}
        <div className="p-3.5 border-2 border-neutral-300 dark:border-neutral-800 bg-white dark:bg-[#151518]">
          <div className="text-[10px] font-bold text-neutral-500 uppercase mb-1">
            That Single Turn Costs
          </div>
          <div className="text-2xl font-black text-black dark:text-white">
            {turnMultiplier}×
          </div>
          <div className="text-[10px] text-neutral-500 mt-1">
            {formatUSD(switchTurnActualCost)} vs {formatUSD(switchTurnNormalCost)}
          </div>
        </div>

        {/* Card 4: Context Re-written */}
        <div className="p-3.5 border-2 border-neutral-300 dark:border-neutral-800 bg-white dark:bg-[#151518]">
          <div className="text-[10px] font-bold text-neutral-500 uppercase mb-1">
            Context Re-written
          </div>
          <div className="text-2xl font-black text-black dark:text-white">
            {formatTokens(contextRewritten)}
          </div>
          <div className="text-[10px] text-neutral-500 mt-1">
            At $3.75/MTok cache-write
          </div>
        </div>
      </div>

      {/* Visual Turn Bar Diagram */}
      <div className="p-4 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/50 mb-6">
        <div className="flex justify-between text-xs font-bold mb-3">
          <span>PER-TURN COST PROFILE (40 TURNS)</span>
          <span className="text-red-600 dark:text-red-400 font-mono">
            RED SPIKE = CACHE RE-WRITE CLIFF
          </span>
        </div>

        {/* Bar chart grid */}
        <div className="h-28 flex items-end gap-1 sm:gap-1.5 pt-2 pb-1 border-b border-neutral-300 dark:border-neutral-700">
          {turnBreakdown.map((item) => {
            const maxCost = Math.max(...turnBreakdown.map((b) => b.cost));
            const heightPercent = Math.max(8, (item.cost / maxCost) * 100);

            return (
              <div
                key={item.turn}
                className="flex-1 flex flex-col items-center justify-end h-full group relative"
              >
                {/* Tooltip */}
                <div className="opacity-0 group-hover:opacity-100 pointer-events-none absolute bottom-full mb-2 z-20 bg-black text-white text-[9px] p-1.5 whitespace-nowrap shadow-md border border-neutral-700">
                  <div>Turn {item.turn}: {formatUSD(item.cost)}</div>
                  <div>Context: {formatTokens(item.contextSize)}</div>
                  {item.isSpike && <div className="text-red-400 font-bold">★ Invalidation Spike</div>}
                </div>

                <div
                  style={{ height: `${heightPercent}%` }}
                  className={`w-full transition-all ${
                    item.isSpike
                      ? "bg-red-600 border border-red-700 shadow-[0_0_8px_rgba(220,38,38,0.6)] animate-pulse"
                      : item.turn < switchTurn
                      ? "bg-black dark:bg-white"
                      : "bg-neutral-600 dark:bg-neutral-400"
                  }`}
                />
              </div>
            );
          })}
        </div>

        <div className="flex justify-between text-[9px] text-neutral-500 mt-2 font-mono">
          <span>Turn 1</span>
          <span>Turn 10</span>
          <span>Turn 20</span>
          <span>Turn 30</span>
          <span>Turn 40</span>
        </div>
      </div>

      {/* Explanatory Formula Callout */}
      <div className="p-4 border-2 border-black dark:border-neutral-700 bg-white dark:bg-[#17171c] text-xs">
        <div className="flex items-center gap-2 font-bold mb-2 text-black dark:text-white">
          <AlertCircle className="w-4 h-4 text-neutral-700 dark:text-neutral-300" />
          <span>Why this happens under the hood</span>
        </div>
        <p className="text-neutral-700 dark:text-neutral-300 font-sans text-xs leading-relaxed mb-3">
          Anthropic prompt caching is an exact byte-for-byte prefix match. When you change model (e.g. from Claude Sonnet to Claude Opus) or change reasoning effort, the system prompt prefix is altered. All <strong>{formatTokens(contextRewritten)}</strong> previously cached tokens can no longer be read at <strong>$0.30/MTok</strong> (0.1×) and must be recreated at <strong>$3.75/MTok</strong> (1.25×).
        </p>
        <div className="font-mono text-[11px] bg-neutral-100 dark:bg-neutral-900 p-2.5 border border-neutral-300 dark:border-neutral-800 text-neutral-800 dark:text-neutral-200">
          <code>cache_invalidation_delta = context_tokens × ($3.75/MTok - $0.30/MTok) = {formatUSD((contextRewritten / 1000000) * 3.45)}</code>
        </div>
      </div>
    </div>
  );
};
