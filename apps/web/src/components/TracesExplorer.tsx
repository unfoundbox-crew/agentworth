import React, { useState } from "react";
import {
  Search,
  Filter,
  ArrowUpDown,
  Clock,
  ChevronRight,
} from "lucide-react";
import { SessionSummary } from "../types";
import {
  formatTokens,
  formatDuration,
  formatDate,
  formatTimeAgo,
  getAdapterBadge,
} from "../utils/formatters";
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
    { id: "all", label: "All Adapters" },
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
    { id: "all", label: "All Outcomes" },
    { id: "ci_or_deployment_verified", label: "CI Verified (R5)" },
    { id: "commit_observed", label: "Committed (R4)" },
    { id: "test_or_build_passed", label: "Tested (R3)" },
    { id: "artifact_changed", label: "Artifact Changed (R2)" },
    { id: "done_claimed", label: "Claim Only (R1)" },
    { id: "unresolved", label: "Unresolved (R0)" },
  ];

  return (
    <section className="py-8 bg-[#fdfdfd] dark:bg-[#0a0a0c]">
      <div>
        
        {/* Header Bar */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6">
          <div>
            <div className="flex items-center space-x-2">
              <div className="w-3 h-3 bg-black dark:bg-white" />
              <h2 className="text-xl sm:text-2xl font-mono font-bold uppercase tracking-tight text-black dark:text-white">
                TRACES EXPLORER
              </h2>
              <span className="text-xs font-mono px-2 py-0.5 bg-neutral-100 dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300">
                {totalTraces} sessions indexed
              </span>
            </div>
            <p className="text-xs font-mono text-neutral-500 mt-1 font-sans">
              Filter by adapter, verify outcomes, inspect step-by-step model decisions.
            </p>
          </div>

          {/* Sort selection */}
          <div className="flex items-center space-x-2">
            <span className="text-xs font-mono text-neutral-500 flex items-center">
              <ArrowUpDown className="w-3 h-3 mr-1" /> Sort:
            </span>
            <select
              value={orderBy}
              onChange={handleSortChange}
              className="bg-white dark:bg-neutral-900 border border-black dark:border-white px-2.5 py-1 text-xs font-mono text-black dark:text-white shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] dark:shadow-[2px_2px_0px_0px_rgba(255,255,255,1)] focus:outline-none"
            >
              <option value="started_at_desc">Newest First</option>
              <option value="started_at_asc">Oldest First</option>
              <option value="tokens_desc">Most Tokens</option>
              <option value="tokens_asc">Least Tokens</option>
              <option value="score_desc">Highest Score</option>
              <option value="duration_desc">Longest Duration</option>
              <option value="events_desc">Most Events</option>
            </select>
          </div>
        </div>

        {/* Filter Controls */}
        <div className="bg-neutral-50 dark:bg-neutral-900 border border-black dark:border-neutral-700 p-4 mb-6 shadow-[3px_3px_0px_0px_rgba(0,0,0,1)] dark:shadow-[3px_3px_0px_0px_rgba(255,255,255,1)] space-y-3">
          
          {/* Top Filter Row: Search Input + Adapter Chips */}
          <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3">
            
            {/* Search Input */}
            <div className="relative flex-1">
              <Search className="w-4 h-4 text-neutral-400 absolute left-3 top-1/2 -translate-y-1/2" />
              <input
                type="text"
                value={search}
                onChange={handleSearchChange}
                placeholder="Search prompt, model, session ID, or task..."
                className="w-full bg-white dark:bg-neutral-800 border border-neutral-400 dark:border-neutral-600 pl-9 pr-3 py-1.5 text-xs font-mono text-black dark:text-white placeholder:text-neutral-400 focus:border-black dark:focus:border-white focus:outline-none"
              />
            </div>

            {/* Adapter Chips */}
            <div className="flex flex-wrap items-center gap-1.5">
              {adapters.map((ad) => (
                <button
                  key={ad.id}
                  onClick={() => handleAdapterChange(ad.id)}
                  className={`px-2.5 py-1 text-xs font-mono border transition-all ${
                    selectedAdapter === ad.id
                      ? "bg-black dark:bg-white text-white dark:text-black border-black dark:border-white font-semibold shadow-[1px_1px_0px_0px_rgba(0,0,0,0.5)]"
                      : "bg-white dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 border-neutral-300 dark:border-neutral-700 hover:border-black dark:hover:border-white"
                  }`}
                >
                  {ad.label}
                </button>
              ))}
            </div>

          </div>

          {/* Bottom Filter Row: Outcome Filter Chips */}
          <div className="flex flex-wrap items-center gap-1.5 pt-2 border-t border-neutral-200 dark:border-neutral-800">
            <span className="text-[11px] font-mono text-neutral-500 mr-1 flex items-center">
              <Filter className="w-3 h-3 mr-1" /> Outcome Rung:
            </span>
            {outcomeFilters.map((out) => (
              <button
                key={out.id}
                onClick={() => handleOutcomeChange(out.id)}
                className={`px-2 py-0.5 text-[11px] font-mono border transition-all ${
                  selectedOutcome === out.id
                    ? "bg-black dark:bg-white text-white dark:text-black border-black dark:border-white font-semibold"
                    : "bg-white dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 border-neutral-300 dark:border-neutral-700 hover:border-neutral-500"
                }`}
              >
                {out.label}
              </button>
            ))}
          </div>

        </div>

        {/* Traces Table */}
        <div className="border-2 border-black dark:border-white bg-white dark:bg-[#121215] shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] overflow-x-auto">
          <table className="w-full text-left border-collapse font-mono text-xs">
            <thead>
              <tr className="bg-neutral-100 dark:bg-neutral-900 border-b-2 border-black dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 select-none">
                <th className="py-2.5 px-4 font-bold">SESSION / ADAPTER</th>
                <th className="py-2.5 px-4 font-bold">PROMPT PREVIEW</th>
                <th className="py-2.5 px-4 font-bold">MODELS & TOOLS</th>
                <th className="py-2.5 px-4 font-bold">TOKENS / DURATION</th>
                <th className="py-2.5 px-4 font-bold">VERDICT</th>
                <th className="py-2.5 px-4 font-bold text-center">SCORE</th>
                <th className="py-2.5 px-4 font-bold text-right">ACTION</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-neutral-200 dark:divide-neutral-800">
              {isLoading ? (
                <tr>
                  <td colSpan={7} className="py-12 text-center text-neutral-500 font-mono">
                    <span className="inline-block animate-spin mr-2">◴</span> Loading trace histories...
                  </td>
                </tr>
              ) : traces.length === 0 ? (
                <tr>
                  <td colSpan={7} className="py-12 text-center text-neutral-500 font-mono">
                    No matching traces found in local index (~/.agentworth). Run &apos;npx agentworth scan&apos; to index dotfiles.
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
                      className={`hover:bg-neutral-50 dark:hover:bg-neutral-900/80 cursor-pointer transition-colors ${
                        isSelected ? "bg-neutral-100 dark:bg-neutral-900 font-medium" : ""
                      }`}
                    >
                      {/* Session ID & Adapter */}
                      <td className="py-3 px-4 whitespace-nowrap">
                        <div className="flex items-center space-x-2">
                          <span className={`px-1.5 py-0.5 text-[10px] uppercase font-bold border ${adapterInfo.borderColor} bg-white dark:bg-black text-black dark:text-white`}>
                            {adapterInfo.tag}
                          </span>
                          <span className="font-semibold text-black dark:text-white">{trace.session_id}</span>
                        </div>
                        <div className="text-[10px] text-neutral-500 mt-0.5">
                          {formatTimeAgo(trace.started_at)} ({formatDate(trace.started_at)})
                        </div>
                      </td>

                      {/* Prompt preview */}
                      <td className="py-3 px-4 max-w-xs sm:max-w-md">
                        <div className="truncate text-neutral-900 dark:text-neutral-100 font-medium" title={trace.prompt_preview}>
                          {trace.prompt_preview || "No prompt preview recorded"}
                        </div>
                        <div className="text-[10px] text-neutral-400 truncate mt-0.5">
                          {trace.source_path}
                        </div>
                      </td>

                      {/* Models & Tools */}
                      <td className="py-3 px-4 whitespace-nowrap">
                        <div className="flex flex-wrap gap-1">
                          {trace.models_used.map((m) => (
                            <span
                              key={m}
                              className="px-1.5 py-0.5 bg-neutral-100 dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 text-[10px] text-neutral-700 dark:text-neutral-300"
                            >
                              {m}
                            </span>
                          ))}
                        </div>
                        <div className="text-[10px] text-neutral-500 mt-0.5">
                          {trace.tool_calls_count} tool calls · {trace.total_events} events
                        </div>
                      </td>

                      {/* Tokens & Duration */}
                      <td className="py-3 px-4 whitespace-nowrap">
                        <div className="font-bold text-neutral-900 dark:text-neutral-100">
                          {trace.total_tokens > 0 ? formatTokens(trace.total_tokens) : "—"}
                        </div>
                        <div className="text-[10px] text-neutral-500 flex items-center mt-0.5">
                          <Clock className="w-3 h-3 mr-1 text-neutral-400" />
                          {formatDuration(trace.duration_seconds)}
                        </div>
                      </td>

                      {/* Verdict Stamp */}
                      <td className="py-3 px-4 whitespace-nowrap">
                        <VerdictStamp
                          status={trace.primary_outcome || "unresolved"}
                          size="sm"
                        />
                      </td>

                      {/* Score Breakdown Pill */}
                      <td className="py-3 px-4 text-center whitespace-nowrap">
                        {trace.composite_score !== undefined ? (
                          <div className="inline-flex items-center space-x-1 px-2 py-0.5 bg-black dark:bg-white text-white dark:text-black font-bold text-[11px] border border-black dark:border-white shadow-[1px_1px_0px_0px_rgba(0,0,0,1)]">
                            <span>{(trace.composite_score * 100).toFixed(0)}</span>
                            <span className="text-[9px] opacity-70">/100</span>
                          </div>
                        ) : (
                          <span className="text-neutral-400">—</span>
                        )}
                      </td>

                      {/* Action Button */}
                      <td className="py-3 px-4 text-right whitespace-nowrap">
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            onSelectSession(trace.session_id);
                          }}
                          className="inline-flex items-center space-x-1 px-2 py-1 text-[11px] font-mono bg-white dark:bg-neutral-800 hover:bg-black hover:text-white dark:hover:bg-white dark:hover:text-black border border-black dark:border-white transition-colors shadow-[1px_1px_0px_0px_rgba(0,0,0,1)] dark:shadow-[1px_1px_0px_0px_rgba(255,255,255,1)] text-black dark:text-white"
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
    </section>
  );
};
