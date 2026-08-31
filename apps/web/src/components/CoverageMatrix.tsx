import React, { useState, useEffect } from "react";
import { fetchCoverageMatrix } from "../services/api";
import { AdapterCapability } from "../types";
import { getAgentLogo } from "./AgentLogos";
import { Search, ExternalLink, Plus, Check, X, ShieldAlert } from "lucide-react";

interface CoverageMatrixProps {
  className?: string;
}

export const CoverageMatrix: React.FC<CoverageMatrixProps> = ({ className = "" }) => {
  const [capabilities, setCapabilities] = useState<AdapterCapability[]>([]);
  const [search, setSearch] = useState("");
  const [filterType, setFilterType] = useState<"all" | "measured" | "pending">("all");

  useEffect(() => {
    fetchCoverageMatrix().then((res) => {
      setCapabilities(res.adapters);
    });
  }, []);

  const filtered = capabilities.filter((item) => {
    const matchesSearch =
      item.name.toLowerCase().includes(search.toLowerCase()) ||
      item.id.toLowerCase().includes(search.toLowerCase());

    if (!matchesSearch) return false;

    if (filterType === "measured") {
      return item.tokens === "yes" || item.tokens === "partial";
    }
    if (filterType === "pending") {
      return item.tokens === "no";
    }
    return true;
  });

  const renderBadge = (status: "yes" | "no" | "partial" | string) => {
    if (status === "yes") {
      return (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-mono font-bold bg-black dark:bg-white text-white dark:text-black border border-black dark:border-white">
          <Check className="w-3 h-3" />
          <span>YES</span>
        </span>
      );
    }
    if (status === "partial" || status === "rung 2") {
      return (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-mono font-bold bg-amber-100 dark:bg-amber-950/60 text-amber-900 dark:text-amber-300 border border-amber-500">
          <span>{status.toUpperCase()}</span>
        </span>
      );
    }
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-mono font-bold bg-red-50 dark:bg-red-950/40 text-red-700 dark:text-red-400 border border-red-500/70 border-dashed">
        <X className="w-2.5 h-2.5" />
        <span>NO</span>
      </span>
    );
  };

  return (
    <div
      className={`border-2 border-black dark:border-white bg-white dark:bg-[#121215] text-black dark:text-white p-6 sm:p-7 font-mono shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)] ${className}`}
    >
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-5 border-b-2 border-dashed border-neutral-300 dark:border-neutral-700">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <span className="text-xs font-bold uppercase tracking-widest text-neutral-500 dark:text-neutral-400">
              § THE COVERAGE MATRIX
            </span>
            <span className="text-[10px] px-2 py-0.5 border border-black dark:border-white font-bold bg-neutral-100 dark:bg-neutral-900 text-black dark:text-white">
              21 ADAPTERS
            </span>
          </div>
          <h2 className="text-xl sm:text-2xl font-extrabold tracking-tight">
            Adapter Extraction Capabilities
          </h2>
          <p className="text-xs text-neutral-600 dark:text-neutral-400 mt-1 font-sans">
            Grounded directly in our automated fixture test suite. We publish exactly what each adapter extracts and what remains pending.
          </p>
        </div>

        <a
          href="https://github.com/unfoundbox-crew/agentworth/issues/new?title=Adapter+Extraction+Request&labels=adapter,enhancement"
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1.5 px-3 py-1.5 bg-black hover:bg-neutral-800 dark:bg-white dark:hover:bg-neutral-200 text-white dark:text-black font-mono text-xs font-bold transition shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] dark:shadow-[2px_2px_0px_0px_rgba(255,255,255,1)] shrink-0"
        >
          <Plus className="w-3.5 h-3.5" />
          <span>Request Adapter PR</span>
          <ExternalLink className="w-3 h-3 ml-0.5" />
        </a>
      </div>

      {/* Filter and Search Bar */}
      <div className="flex flex-col sm:flex-row items-center justify-between gap-3 my-5">
        <div className="relative w-full sm:w-72">
          <Search className="w-4 h-4 text-neutral-400 absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            type="text"
            placeholder="Search adapters..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full pl-9 pr-3 py-1.5 text-xs bg-neutral-50 dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 text-black dark:text-white focus:outline-none focus:border-black dark:focus:border-white font-mono"
          />
        </div>

        <div className="flex items-center gap-1.5 w-full sm:w-auto">
          {(["all", "measured", "pending"] as const).map((mode) => (
            <button
              key={mode}
              onClick={() => setFilterType(mode)}
              className={`px-3 py-1 text-xs uppercase font-bold border transition ${
                filterType === mode
                  ? "bg-black dark:bg-white text-white dark:text-black border-black dark:border-white"
                  : "bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 border-neutral-300 dark:border-neutral-800 hover:border-black dark:hover:border-white"
              }`}
            >
              {mode === "all" ? "All (21)" : mode === "measured" ? "Token Measured" : "Discovery Only"}
            </button>
          ))}
        </div>
      </div>

      {/* Table */}
      <div className="overflow-x-auto border border-neutral-300 dark:border-neutral-800">
        <table className="w-full text-left text-xs border-collapse">
          <thead>
            <tr className="bg-neutral-100 dark:bg-neutral-900 border-b border-neutral-300 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 font-bold uppercase text-[10px] tracking-wider">
              <th className="p-3 border-r border-neutral-300 dark:border-neutral-800">Adapter</th>
              <th className="p-3 text-center border-r border-neutral-300 dark:border-neutral-800">Sessions</th>
              <th className="p-3 text-center border-r border-neutral-300 dark:border-neutral-800">Tokens</th>
              <th className="p-3 text-center border-r border-neutral-300 dark:border-neutral-800">Cache Split</th>
              <th className="p-3 text-center border-r border-neutral-300 dark:border-neutral-800">Models</th>
              <th className="p-3 text-center border-r border-neutral-300 dark:border-neutral-800">File Edits</th>
              <th className="p-3 text-center border-r border-neutral-300 dark:border-neutral-800">Shell Exit</th>
              <th className="p-3 text-center">Outcomes</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-neutral-200 dark:divide-neutral-800">
            {filtered.map((item) => (
              <tr
                key={item.id}
                className="hover:bg-neutral-50 dark:hover:bg-neutral-900/50 transition-colors"
              >
                <td className="p-3 border-r border-neutral-200 dark:border-neutral-800 font-bold flex items-center gap-2.5">
                  <div className="shrink-0">{getAgentLogo(item.id, 16)}</div>
                  <div>
                    <div className="text-black dark:text-white">{item.name}</div>
                    {item.notes && (
                      <div className="text-[10px] font-normal text-neutral-500 dark:text-neutral-400 font-sans truncate max-w-xs">
                        {item.notes}
                      </div>
                    )}
                  </div>
                </td>
                <td className="p-3 text-center border-r border-neutral-200 dark:border-neutral-800">
                  {renderBadge(item.sessions)}
                </td>
                <td className="p-3 text-center border-r border-neutral-200 dark:border-neutral-800">
                  {renderBadge(item.tokens)}
                </td>
                <td className="p-3 text-center border-r border-neutral-200 dark:border-neutral-800">
                  {renderBadge(item.cache_split)}
                </td>
                <td className="p-3 text-center border-r border-neutral-200 dark:border-neutral-800">
                  {renderBadge(item.models)}
                </td>
                <td className="p-3 text-center border-r border-neutral-200 dark:border-neutral-800">
                  {renderBadge(item.file_edits)}
                </td>
                <td className="p-3 text-center border-r border-neutral-200 dark:border-neutral-800">
                  {renderBadge(item.shell_exit)}
                </td>
                <td className="p-3 text-center">
                  {renderBadge(item.outcomes)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Honest Footer Callout */}
      <div className="mt-4 p-3 bg-neutral-50 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 flex items-start gap-2 text-[11px] text-neutral-600 dark:text-neutral-400">
        <ShieldAlert className="w-4 h-4 text-neutral-700 dark:text-neutral-300 shrink-0 mt-0.5" />
        <div>
          <strong className="text-black dark:text-white">Honest Accounting Rule:</strong> We never print $0.00 for unextracted data. A cell reads <code>YES</code> only when an automated test fixture verifies it.
        </div>
      </div>
    </div>
  );
};
