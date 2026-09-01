import React, { useState } from "react";
import { ArrowUpDown, Clock, ChevronRight, Search, Filter } from "lucide-react";
import { SessionSummary } from "../types";
import { formatTokens, formatDuration, formatDate, formatTimeAgo, getAdapterBadge } from "../utils/formatters";
import { VerdictStamp } from "./VerdictStamp";

interface TracesExplorerProps {
  traces: SessionSummary[];
  totalTraces: number;
  selectedSessionId?: string;
  onSelectSession: (sessionId: string) => void;
  onFilterChange: (filters: {
    adapter?: string;
    search?: string;
    outcome?: string;
    orderBy?: string;
  }) => void;
  isLoading?: boolean;
}

export const TracesExplorer: React.FC<TracesExplorerProps> = ({
  traces,
  totalTraces,
  selectedSessionId,
  onSelectSession,
  onFilterChange,
  isLoading,
}) => {
  const [search, setSearch] = useState("");
  const [selectedAdapter, setSelectedAdapter] = useState("all");
  const [selectedOutcome, setSelectedOutcome] = useState("all");
  const [orderBy, setOrderBy] = useState("started_at_desc");

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    setSearch(val);
    onFilterChange({ adapter: selectedAdapter, search: val, outcome: selectedOutcome, orderBy });
  };

  const handleAdapterChange = (adapter: string) => {
    setSelectedAdapter(adapter);
    onFilterChange({ adapter, search, outcome: selectedOutcome, orderBy });
  };

  const handleOutcomeChange = (outcome: string) => {
    setSelectedOutcome(outcome);
    onFilterChange({ adapter: selectedAdapter, search, outcome, orderBy });
  };

  const handleSortChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const val = e.target.value;
    setOrderBy(val);
    onFilterChange({ adapter: selectedAdapter, search, outcome: selectedOutcome, orderBy: val });
  };

  const adapters = [
    { id: "all", label: "All adapters" },
    { id: "claude_code", label: "Claude Code" },
    { id: "cursor", label: "Cursor" },
    { id: "codex", label: "Codex" },
    { id: "antigravity", label: "Antigravity (AGY)" },
    { id: "gemini", label: "Gemini CLI" },
    { id: "hermes", label: "Nous Hermes" },
    { id: "goose", label: "Block Goose" },
    { id: "pi", label: "Pi" },
    { id: "grok", label: "Grok" },
    { id: "opencode", label: "OpenCode" },
  ];

  const outcomeFilters = [
    { id: "all", label: "All outcomes" },
    { id: "ci_or_deployment_verified", label: "CI verified (R5)" },
    { id: "commit_observed", label: "Committed (R4)" },
    { id: "test_or_build_passed", label: "Tested (R3)" },
    { id: "artifact_changed", label: "Artifact changed (R2)" },
    { id: "done_claimed", label: "Claim only (R1)" },
    { id: "unresolved", label: "Unresolved (R0)" },
  ];

  return (
    <div className="panel">
      {/* Panel head */}
      <div className="panel-head">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div>
            <div className="panel-kicker">
              <span className="tag-pill">Traces explorer</span>
              <span className="tag-pill">{totalTraces.toLocaleString()} sessions indexed</span>
            </div>
            <h2>Filter, verify, inspect</h2>
            <p>Filter by adapter, verify outcomes, inspect step-by-step model decisions.</p>
          </div>

          <div className="flex items-center gap-2">
            <span className="text-xs font-mono text-faint flex items-center gap-1 shrink-0">
              <ArrowUpDown className="w-3 h-3" /> Sort
            </span>
            <select value={orderBy} onChange={handleSortChange} className="field" style={{ width: "auto" }}>
              <option value="started_at_desc">Newest first</option>
              <option value="started_at_asc">Oldest first</option>
              <option value="tokens_desc">Most tokens</option>
              <option value="tokens_asc">Least tokens</option>
              <option value="score_desc">Highest score</option>
              <option value="duration_desc">Longest duration</option>
              <option value="events_desc">Most events</option>
            </select>
          </div>
        </div>
      </div>

      {/* Filter controls */}
      <div className="rounded-xl border border-border bg-surface p-4 mb-5 space-y-3">
        <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3">
          <div className="relative flex-1">
            <Search className="w-3.5 h-3.5 text-faint absolute left-3 top-1/2 -translate-y-1/2" />
            <input
              type="text"
              value={search}
              onChange={handleSearchChange}
              placeholder="Search prompt, model, session ID, or task…"
              className="field"
              style={{ paddingLeft: 32, background: "var(--mv-ground)" }}
            />
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            {adapters.map((ad) => (
              <button
                key={ad.id}
                type="button"
                onClick={() => handleAdapterChange(ad.id)}
                className={`chip ${selectedAdapter === ad.id ? "is-active" : ""}`}
              >
                {ad.label}
              </button>
            ))}
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1.5 pt-3 border-t border-border-soft">
          <span className="text-[11px] font-mono text-faint mr-1 flex items-center gap-1">
            <Filter className="w-3 h-3" /> Outcome rung
          </span>
          {outcomeFilters.map((out) => (
            <button
              key={out.id}
              type="button"
              onClick={() => handleOutcomeChange(out.id)}
              className={`chip ${selectedOutcome === out.id ? "is-active" : ""}`}
              style={{ fontSize: "0.6875rem", padding: "3px 10px" }}
            >
              {out.label}
            </button>
          ))}
        </div>
      </div>

      {/* Traces table — scrolls in its own container, never the page */}
      <div className="table-frame table-scroll">
        <table className="data-table" style={{ minWidth: 920 }}>
          <thead>
            <tr>
              <th>Session / adapter</th>
              <th>Prompt preview</th>
              <th>Models &amp; tools</th>
              <th>Tokens / duration</th>
              <th>Verdict</th>
              <th className="is-center">Score</th>
              <th className="is-right">Action</th>
            </tr>
          </thead>
          <tbody>
            {isLoading ? (
              <tr>
                <td colSpan={7} className="text-center py-12 text-muted font-mono text-xs">
                  <span className="inline-block animate-spin mr-2">&#9700;</span> Loading trace histories…
                </td>
              </tr>
            ) : traces.length === 0 ? (
              <tr>
                <td colSpan={7} className="text-center py-12 text-muted font-mono text-xs">
                  No matching traces found in local index (~/.agentworth). Run &apos;npx agentworth scan&apos; to
                  index dotfiles.
                </td>
              </tr>
            ) : (
              traces.map((trace) => {
                const adapterInfo = getAdapterBadge(trace.adapter);
                const isSelected = selectedSessionId === trace.session_id;

                return (
                  <tr
                    key={trace.session_id}
                    onClick={() => onSelectSession(trace.session_id)}
                    className={isSelected ? "is-selected" : ""}
                  >
                    {/* Session ID & Adapter */}
                    <td style={{ whiteSpace: "nowrap" }}>
                      <div className="flex items-center gap-2">
                        <span className="tag-pill" style={{ textTransform: "uppercase" }}>
                          {adapterInfo.tag}
                        </span>
                        <span className="font-semibold text-ink">{trace.session_id}</span>
                      </div>
                      <div className="text-[10px] text-faint mt-1 font-mono">
                        {formatTimeAgo(trace.started_at)} ({formatDate(trace.started_at)})
                      </div>
                    </td>

                    {/* Prompt preview */}
                    <td style={{ maxWidth: 320 }}>
                      <div className="truncate text-text font-medium" title={trace.prompt_preview}>
                        {trace.prompt_preview || "No prompt preview recorded"}
                      </div>
                      <div className="text-[10px] text-faint truncate mt-1 font-mono">{trace.source_path}</div>
                    </td>

                    {/* Models & Tools */}
                    <td style={{ whiteSpace: "nowrap" }}>
                      <div className="flex flex-wrap gap-1">
                        {trace.models_used.map((m) => (
                          <span key={m} className="tag-pill">
                            {m}
                          </span>
                        ))}
                      </div>
                      <div className="text-[10px] text-faint mt-1 font-mono">
                        {trace.tool_calls_count} tool calls &middot; {trace.total_events} events
                      </div>
                    </td>

                    {/* Tokens & Duration */}
                    <td className="num" style={{ whiteSpace: "nowrap" }}>
                      <div className="font-semibold text-ink">
                        {trace.total_tokens > 0 ? formatTokens(trace.total_tokens) : "—"}
                      </div>
                      <div className="text-[10px] text-faint flex items-center gap-1 mt-1">
                        <Clock className="w-3 h-3" />
                        {formatDuration(trace.duration_seconds)}
                      </div>
                    </td>

                    {/* Verdict Stamp */}
                    <td style={{ whiteSpace: "nowrap" }}>
                      <VerdictStamp status={trace.primary_outcome || "unresolved"} size="sm" />
                    </td>

                    {/* Score */}
                    <td className="is-center num" style={{ whiteSpace: "nowrap" }}>
                      {trace.composite_score !== undefined ? (
                        <span className="inline-flex items-baseline gap-0.5 px-2 py-0.5 rounded-md bg-surface-3 text-ink font-semibold text-xs">
                          {(trace.composite_score * 100).toFixed(0)}
                          <span className="text-[9px] text-faint font-normal">/100</span>
                        </span>
                      ) : (
                        <span className="text-faint">—</span>
                      )}
                    </td>

                    {/* Action */}
                    <td className="is-right" style={{ whiteSpace: "nowrap" }}>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          onSelectSession(trace.session_id);
                        }}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[11px] font-mono border border-border bg-ground text-ink hover:border-accent-border hover:text-accent transition-colors"
                      >
                        <span>Inspect</span>
                        <ChevronRight className="w-3 h-3" />
                      </button>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
};
