import React, { useState, useEffect } from "react";
import { fetchCoverageMatrix } from "../services/api";
import { AdapterCapability } from "../types";
import { getAgentLogo } from "./AgentLogos";
import { Search, ExternalLink, Plus, ShieldAlert } from "lucide-react";

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
      item.name.toLowerCase().includes(search.toLowerCase()) || item.id.toLowerCase().includes(search.toLowerCase());

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
        <span className="status-pill is-good">
          <span className="dot" />
          Yes
        </span>
      );
    }
    if (status === "partial" || status === "rung 2") {
      return (
        <span className="status-pill is-warn">
          <span className="dot" />
          {status === "rung 2" ? "Rung 2" : "Partial"}
        </span>
      );
    }
    return (
      <span className="status-pill is-bad">
        <span className="dot" />
        No
      </span>
    );
  };

  return (
    <div className={`panel ${className}`}>
      {/* Panel head */}
      <div className="panel-head">
        <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4">
          <div>
            <div className="panel-kicker">
              <span className="tag-pill">The coverage matrix</span>
              <span className="tag-pill">21 adapters</span>
            </div>
            <h2>Adapter extraction capabilities</h2>
            <p>
              Grounded directly in our automated fixture test suite. We publish exactly what each adapter extracts
              and what remains pending.
            </p>
          </div>

          <a
            href="https://github.com/unfoundbox-crew/agentworth/issues/new?title=Adapter+Extraction+Request&labels=adapter,enhancement"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-ink text-ground font-mono text-xs font-semibold hover:opacity-85 transition-opacity shrink-0"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Request adapter</span>
            <ExternalLink className="w-3 h-3" />
          </a>
        </div>
      </div>

      {/* Filter and search */}
      <div className="flex flex-col sm:flex-row items-stretch sm:items-center justify-between gap-3 mb-5">
        <div className="relative w-full sm:w-72">
          <Search className="w-3.5 h-3.5 text-faint absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            type="text"
            placeholder="Search adapters…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="field"
            style={{ paddingLeft: 32 }}
          />
        </div>

        <div className="flex items-center gap-1.5">
          {(["all", "measured", "pending"] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              onClick={() => setFilterType(mode)}
              className={`chip ${filterType === mode ? "is-active" : ""}`}
            >
              {mode === "all" ? "All (21)" : mode === "measured" ? "Token measured" : "Discovery only"}
            </button>
          ))}
        </div>
      </div>

      {/* Table */}
      <div className="table-frame table-scroll">
        <table className="data-table matrix-table" style={{ minWidth: 860 }}>
          <thead>
            <tr>
              <th>Adapter</th>
              <th className="is-center">Sessions</th>
              <th className="is-center">Tokens</th>
              <th className="is-center">Cache split</th>
              <th className="is-center">Models</th>
              <th className="is-center">File edits</th>
              <th className="is-center">Shell exit</th>
              <th className="is-center">Outcomes</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((item) => (
              <tr key={item.id}>
                <td>
                  <div className="flex items-center gap-2.5">
                    <div className="shrink-0">{getAgentLogo(item.id, 16)}</div>
                    <div>
                      <div className="text-ink font-medium">{item.name}</div>
                      {item.notes && (
                        <div className="text-[11px] font-normal text-muted truncate max-w-xs">{item.notes}</div>
                      )}
                    </div>
                  </div>
                </td>
                <td className="is-center">{renderBadge(item.sessions)}</td>
                <td className="is-center">{renderBadge(item.tokens)}</td>
                <td className="is-center">{renderBadge(item.cache_split)}</td>
                <td className="is-center">{renderBadge(item.models)}</td>
                <td className="is-center">{renderBadge(item.file_edits)}</td>
                <td className="is-center">{renderBadge(item.shell_exit)}</td>
                <td className="is-center">{renderBadge(item.outcomes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Honest footer callout */}
      <div className="mt-4 p-3 rounded-lg border border-border bg-surface flex items-start gap-2 text-xs text-muted">
        <ShieldAlert className="w-4 h-4 text-muted shrink-0 mt-0.5" />
        <div>
          <strong className="text-ink font-semibold">Honest accounting rule.</strong> We never print $0.00 for
          unextracted data. A cell reads <code className="font-mono text-[0.92em] text-text">YES</code> only when an
          automated test fixture verifies it.
        </div>
      </div>
    </div>
  );
};
