import React from "react";
import { ArchaeologyData } from "../types";

interface ArchaeologyPanelProps {
  data: ArchaeologyData;
}

export const ArchaeologyPanel: React.FC<ArchaeologyPanelProps> = ({ data }) => {
  const { most_expensive_task, longest_recovery_loop, model_hopping, weird_discoveries } = data;

  return (
    <div className="panel">
      <div className="panel-head">
        <div className="panel-kicker">
          <span className="tag-pill">Your agent archaeology</span>
        </div>
        <h2>Strange fossils excavated from ~/.config</h2>
        <p>The three sessions worth telling a story about, and the finds nobody asked for.</p>
      </div>

      {/* 3 highlight cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        {/* Card 1: Most Expensive Unsolved Task */}
        <div className="rounded-xl border border-border p-4 flex flex-col justify-between">
          <div>
            <div className="flex justify-between items-start gap-2 mb-3">
              <span className="text-[11px] font-mono font-semibold text-muted uppercase">
                {most_expensive_task.title}
              </span>
              <span className="status-pill is-bad shrink-0">
                <span className="dot" />
                Unresolved
              </span>
            </div>

            <div className="quote-block mb-4">&ldquo;{most_expensive_task.prompt}&rdquo;</div>

            <div className="space-y-0">
              <div className="kv-row">
                <span className="k">Tokens burnt</span>
                <span className="v">{most_expensive_task.tokens}</span>
              </div>
              <div className="kv-row">
                <span className="k">Models consulted</span>
                <span className="v">{most_expensive_task.models_count} models</span>
              </div>
              <div className="kv-row">
                <span className="k">Time spent</span>
                <span className="v">{most_expensive_task.duration}</span>
              </div>
            </div>
          </div>

          <div className="mt-4 pt-3 border-t border-border-soft text-xs text-muted leading-relaxed">
            <span className="font-semibold text-ink">Post-mortem. </span>
            {most_expensive_task.notes}
          </div>
        </div>

        {/* Card 2: Longest Recovery Loop */}
        <div className="rounded-xl border border-border p-4 flex flex-col justify-between">
          <div>
            <div className="flex justify-between items-start gap-2 mb-3">
              <span className="text-[11px] font-mono font-semibold text-muted uppercase">
                {longest_recovery_loop.title}
              </span>
              <span className="status-pill is-warn shrink-0">
                <span className="dot" />
                Autonomous recovery
              </span>
            </div>

            <div className="quote-block mb-4">
              <code>{longest_recovery_loop.initial_error}</code>
            </div>

            <div className="space-y-0">
              <div className="kv-row">
                <span className="k">Failed attempts</span>
                <span className="v">{longest_recovery_loop.attempts_count} iterations</span>
              </div>
              <div className="kv-row">
                <span className="k">Tool executions</span>
                <span className="v">{longest_recovery_loop.tool_calls} calls</span>
              </div>
              <div className="kv-row">
                <span className="k">Tokens consumed</span>
                <span className="v">{longest_recovery_loop.tokens_burned}</span>
              </div>
            </div>
          </div>

          <div className="mt-4 pt-3 border-t border-border-soft text-xs text-muted leading-relaxed">
            <span className="font-semibold text-ink">Resolution. </span>
            {longest_recovery_loop.final_resolution}
          </div>
        </div>

        {/* Card 3: Model Hopping Ping-Pong */}
        <div className="rounded-xl border border-border p-4 flex flex-col justify-between">
          <div>
            <div className="flex justify-between items-start gap-2 mb-3">
              <span className="text-[11px] font-mono font-semibold text-muted uppercase">{model_hopping.title}</span>
              <span className="status-pill is-neutral shrink-0">
                <span className="dot" />
                Fallback relay
              </span>
            </div>

            <div className="space-y-2 my-3">
              {model_hopping.sequence.map((step, idx) => (
                <div key={idx} className="flex items-center gap-2.5 text-sm">
                  <span className="w-5 h-5 rounded-full bg-surface-3 text-ink flex items-center justify-center font-mono font-semibold text-[10px] shrink-0">
                    {idx + 1}
                  </span>
                  <span className="text-text">{step}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="mt-4 pt-3 border-t border-border-soft text-xs text-muted leading-relaxed">
            <div className="flex justify-between font-semibold text-ink mb-1">
              <span>Orchestrator cost</span>
              <span className="font-mono">{model_hopping.total_cost}</span>
            </div>
            <div>{model_hopping.reason}</div>
          </div>
        </div>
      </div>

      {/* Weird discoveries — term-grid, design.md's own "term → explanation" pattern */}
      <div className="pt-6 border-t border-border">
        <div className="flex items-center justify-between mb-1">
          <span className="eyebrow" style={{ marginBottom: 0 }}>
            Hilarious archaeological finds
          </span>
          <span className="text-xs font-mono text-faint hidden sm:inline">Extracted via AST &amp; outcome analyzer</span>
        </div>

        <div className="term-grid cols-3">
          {weird_discoveries.map((disc) => (
            <div key={disc.id} className="term-card">
              <div className="flex justify-between items-start gap-2 mb-2">
                <h3 style={{ marginBottom: 0, fontSize: "0.9375rem" }}>{disc.title}</h3>
                <span className="tag-pill shrink-0">{disc.stat}</span>
              </div>
              <p>{disc.description}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
