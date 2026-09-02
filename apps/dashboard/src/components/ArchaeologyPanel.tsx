import React from "react";
import { ArchaeologyData } from "../types";
import { formatTokens, formatDuration, formatUSD, estimateTokenCostUSD } from "../utils/formatters";

interface ArchaeologyPanelProps {
  data: ArchaeologyData;
}

/** Placeholder for a highlight card whose underlying signal (an unsolved task, a recovery
 * loop, a model switch) hasn't shown up in the index yet — real absence, not a loading state. */
function NotDetectedCard({ label }: { label: string }) {
  return (
    <div className="rounded-xl border border-border-soft border-dashed p-4 flex items-center justify-center text-xs text-muted">
      No {label} found in the indexed sessions yet.
    </div>
  );
}

export const ArchaeologyPanel: React.FC<ArchaeologyPanelProps> = ({ data }) => {
  const { most_expensive_unsolved, longest_recovery_loop, most_frequent_model_switches, token_carbon_dating } = data;

  const timelineByTokens = [...token_carbon_dating.timeline].sort((a, b) => b.tokens - a.tokens).slice(0, 6);
  const adapterTokenEntries = Object.entries(token_carbon_dating.adapter_tokens).sort((a, b) => b[1] - a[1]).slice(0, 6);

  return (
    <div className="panel">
      <div className="panel-head">
        <div className="panel-kicker">
          <span className="tag-pill">Your agent archaeology</span>
        </div>
        <h2>Strange fossils excavated from ~/.config</h2>
        <p>The sessions worth telling a story about, dug out of your own trace history.</p>
      </div>

      {/* 3 highlight cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        {/* Card 1: Most Expensive Unsolved Task */}
        {most_expensive_unsolved ? (
          <div className="rounded-xl border border-border p-4 flex flex-col justify-between">
            <div>
              <div className="flex justify-between items-start gap-2 mb-3">
                <span className="text-[11px] font-mono font-semibold text-muted uppercase">
                  Most expensive unsolved task
                </span>
                <span className="status-pill is-bad shrink-0">
                  <span className="dot" />
                  Unresolved
                </span>
              </div>

              <div className="quote-block mb-4">&ldquo;{most_expensive_unsolved.prompt}&rdquo;</div>

              <div className="space-y-0">
                <div className="kv-row">
                  <span className="k">Tokens burnt</span>
                  <span className="v">{formatTokens(most_expensive_unsolved.total_tokens)}</span>
                </div>
                <div className="kv-row">
                  <span className="k">Models consulted</span>
                  <span className="v">{most_expensive_unsolved.models_used.length} models</span>
                </div>
                <div className="kv-row">
                  <span className="k">Time spent</span>
                  <span className="v">{formatDuration(most_expensive_unsolved.duration_seconds ?? undefined)}</span>
                </div>
              </div>
            </div>

            <div className="mt-4 pt-3 border-t border-border-soft text-xs text-muted leading-relaxed">
              <span className="font-semibold text-ink">Post-mortem. </span>
              {most_expensive_unsolved.outcome_summary}
              {most_expensive_unsolved.error_count > 0
                ? ` (${most_expensive_unsolved.error_count} error${most_expensive_unsolved.error_count === 1 ? "" : "s"} observed)`
                : ""}
            </div>
          </div>
        ) : (
          <NotDetectedCard label="unsolved expensive task" />
        )}

        {/* Card 2: Longest Recovery Loop */}
        {longest_recovery_loop ? (
          <div className="rounded-xl border border-border p-4 flex flex-col justify-between">
            <div>
              <div className="flex justify-between items-start gap-2 mb-3">
                <span className="text-[11px] font-mono font-semibold text-muted uppercase">
                  Longest recovery loop
                </span>
                <span className="status-pill is-warn shrink-0">
                  <span className="dot" />
                  Autonomous recovery
                </span>
              </div>

              <div className="quote-block mb-4">
                <code>{longest_recovery_loop.failure_summary}</code>
              </div>

              <div className="space-y-0">
                <div className="kv-row">
                  <span className="k">Steps to recover</span>
                  <span className="v">{longest_recovery_loop.steps_to_recover} steps</span>
                </div>
                <div className="kv-row">
                  <span className="k">Corrective actions</span>
                  <span className="v">{longest_recovery_loop.corrective_actions_count}</span>
                </div>
                <div className="kv-row">
                  <span className="k">Time to recover</span>
                  <span className="v">{formatDuration(longest_recovery_loop.duration_seconds ?? undefined)}</span>
                </div>
              </div>
            </div>

            <div className="mt-4 pt-3 border-t border-border-soft text-xs text-muted leading-relaxed">
              <span className="font-semibold text-ink">Resolution. </span>
              {longest_recovery_loop.recovery_summary}
            </div>
          </div>
        ) : (
          <NotDetectedCard label="recovery loop" />
        )}

        {/* Card 3: Model Hopping Ping-Pong */}
        {most_frequent_model_switches ? (
          <div className="rounded-xl border border-border p-4 flex flex-col justify-between">
            <div>
              <div className="flex justify-between items-start gap-2 mb-3">
                <span className="text-[11px] font-mono font-semibold text-muted uppercase">
                  Heaviest model hopping
                </span>
                <span className="status-pill is-neutral shrink-0">
                  <span className="dot" />
                  Fallback relay
                </span>
              </div>

              <div className="space-y-2 my-3">
                {most_frequent_model_switches.unique_models.map((model, idx) => (
                  <div key={idx} className="flex items-center gap-2.5 text-sm">
                    <span className="w-5 h-5 rounded-full bg-surface-3 text-ink flex items-center justify-center font-mono font-semibold text-[10px] shrink-0">
                      {idx + 1}
                    </span>
                    <span className="text-text">{model}</span>
                  </div>
                ))}
              </div>
            </div>

            <div className="mt-4 pt-3 border-t border-border-soft text-xs text-muted leading-relaxed">
              <div className="flex justify-between font-semibold text-ink mb-1">
                <span>Estimated cost</span>
                <span className="font-mono">
                  {formatUSD(
                    estimateTokenCostUSD(
                      most_frequent_model_switches.total_tokens,
                      most_frequent_model_switches.unique_models
                    )
                  )}
                </span>
              </div>
              <div>
                {most_frequent_model_switches.switch_count} switch
                {most_frequent_model_switches.switch_count === 1 ? "" : "es"} across{" "}
                {most_frequent_model_switches.models_sequence.length} model invocations
              </div>
            </div>
          </div>
        ) : (
          <NotDetectedCard label="model-hopping session" />
        )}
      </div>

      {/* Token carbon dating — real monthly timeline + adapter breakdown, in place of invented finds */}
      <div className="pt-6 border-t border-border">
        <div className="flex items-center justify-between mb-1">
          <span className="eyebrow" style={{ marginBottom: 0 }}>
            Token carbon dating
          </span>
          <span className="text-xs font-mono text-faint hidden sm:inline">
            {token_carbon_dating.total_days_active} days active &middot; avg{" "}
            {formatTokens(token_carbon_dating.average_tokens_per_session)}/session
          </span>
        </div>

        <div className="term-grid cols-3">
          {timelineByTokens.map((era) => (
            <div key={era.period} className="term-card">
              <div className="flex justify-between items-start gap-2 mb-2">
                <h3 style={{ marginBottom: 0, fontSize: "0.9375rem" }}>{era.period}</h3>
                <span className="tag-pill shrink-0">{formatTokens(era.tokens)}</span>
              </div>
              <p>
                {era.sessions_count} session{era.sessions_count === 1 ? "" : "s"}
              </p>
            </div>
          ))}
          {adapterTokenEntries.map(([adapter, tokens]) => (
            <div key={adapter} className="term-card">
              <div className="flex justify-between items-start gap-2 mb-2">
                <h3 style={{ marginBottom: 0, fontSize: "0.9375rem" }}>{adapter}</h3>
                <span className="tag-pill shrink-0">{formatTokens(tokens)}</span>
              </div>
              <p>lifetime tokens for this adapter</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
