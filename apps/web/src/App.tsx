import { useState, useEffect } from "react";
import { Navbar } from "./components/Navbar";
import { LandingPage } from "./components/LandingPage";
import { HeroReceipt } from "./components/HeroReceipt";
import { VerdictBoard } from "./components/VerdictBoard";
import { CacheCliffWidget } from "./components/CacheCliffWidget";
import { CoverageMatrix } from "./components/CoverageMatrix";
import { ArchaeologyPanel } from "./components/ArchaeologyPanel";
import { TracesExplorer } from "./components/TracesExplorer";
import { SessionInspector } from "./components/SessionInspector";
import { ExportModal } from "./components/ExportModal";
import { Footer } from "./components/Footer";

import {
  AggregateStats,
  SessionSummary,
  AgentWorthTrace,
  OutcomeKind,
} from "./types";
import {
  fetchAggregateStats,
  fetchTraces,
  fetchTraceDetail,
  TraceQueryFilters,
  EMPTY_AGGREGATE_STATS,
} from "./services/api";

import { initAnalytics } from "./services/analytics";
import { useTheme } from "./hooks/useTheme";

export function App() {
  useTheme();
  const [viewMode, setViewMode] = useState<"landing" | "explorer">("landing");
  const [stats, setStats] = useState<AggregateStats>(EMPTY_AGGREGATE_STATS);
  const [traces, setTraces] = useState<SessionSummary[]>([]);
  const [totalTraces, setTotalTraces] = useState<number>(0);
  const [filters, setFilters] = useState<TraceQueryFilters>({
    orderBy: "started_at_desc",
  });
  const [isLoadingTraces, setIsLoadingTraces] = useState<boolean>(false);
  const [isScanning, setIsScanning] = useState<boolean>(false);

  // Selected session for Inspection
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [activeTrace, setActiveTrace] = useState<AgentWorthTrace | null>(null);

  // Export Modal state
  const [isExportOpen, setIsExportOpen] = useState<boolean>(false);

  // Load stats & traces on mount
  useEffect(() => {
    initAnalytics();
    // If URL has ?view=explorer, start in explorer mode
    const params = new URLSearchParams(window.location.search);
    if (params.get("view") === "explorer") {
      setViewMode("explorer");
    }
    loadData(filters);
  }, []);

  const loadData = async (currentFilters: TraceQueryFilters) => {
    setIsLoadingTraces(true);
    try {
      const [statsData, tracesData] = await Promise.all([
        fetchAggregateStats(),
        fetchTraces(currentFilters),
      ]);
      setStats(statsData);
      setTraces(tracesData.traces);
      setTotalTraces(tracesData.total);
    } catch (err) {
      console.error("Failed loading data:", err);
    } finally {
      setIsLoadingTraces(false);
    }
  };

  const handleFilterChange = (newFilters: TraceQueryFilters) => {
    const merged = { ...filters, ...newFilters };
    setFilters(merged);
    loadData(merged);
  };

  const handleSelectRung = (rung: OutcomeKind | null) => {
    handleFilterChange({ outcome: rung || undefined });
  };

  const handleSelectSession = async (sessionId: string) => {
    setSelectedSessionId(sessionId);
    try {
      const detail = await fetchTraceDetail(sessionId);
      setActiveTrace(detail);
    } catch (err) {
      console.error("Failed loading session detail:", err);
    }
  };

  const handleCloseInspector = () => {
    setSelectedSessionId(null);
    setActiveTrace(null);
  };

  const handleTriggerScan = () => {
    setIsScanning(true);
    setTimeout(async () => {
      await loadData(filters);
      setIsScanning(false);
    }, 1200);
  };

  return (
    <div className="min-h-screen flex flex-col bg-ground text-text">
      {viewMode === "explorer" && (
        <>
          <div className="bg-grid" aria-hidden="true" />
          <Navbar
            onTriggerScan={handleTriggerScan}
            isScanning={isScanning}
            viewMode={viewMode}
            onToggleView={(mode) => setViewMode(mode)}
          />
        </>
      )}

      {viewMode === "landing" ? (
        <LandingPage onOpenExplorer={() => setViewMode("explorer")} />
      ) : (
        <main className="flex-1 py-8 sm:py-10 space-y-8 sm:space-y-10">
          {/* 1. Hero & Physical Receipt Section (self-contained .hero/.shell) */}
          <HeroReceipt stats={stats} onScanClick={handleTriggerScan} />

          {/* 2. Top Panel: The Verdict Board */}
          <div className="shell">
            <VerdictBoard
              stats={stats}
              selectedRung={(filters.outcome as OutcomeKind) || null}
              onSelectRung={handleSelectRung}
            />
          </div>

          {/* 3. Archaeology Panel (if present in local index) */}
          {stats.archaeology && (
            <div className="shell">
              <ArchaeologyPanel data={stats.archaeology} />
            </div>
          )}

          {/* 4. Traces Explorer Table */}
          <div className="shell">
            <TracesExplorer
              traces={traces}
              totalTraces={totalTraces}
              selectedSessionId={selectedSessionId || undefined}
              onSelectSession={handleSelectSession}
              onFilterChange={handleFilterChange}
              isLoading={isLoadingTraces}
            />
          </div>

          {/* 5. The Cache Cliff Interactive Widget */}
          <div className="shell">
            <CacheCliffWidget />
          </div>

          {/* 6. Grounded Coverage Matrix */}
          <div className="shell">
            <CoverageMatrix />
          </div>

          {/* 7. Local-first Architecture Footer (self-contained .sec/.shell) */}
          <Footer />
        </main>
      )}

      {/* Session Inspector Slideover */}
      {activeTrace && (
        <SessionInspector
          trace={activeTrace}
          onClose={handleCloseInspector}
          onOpenExport={() => setIsExportOpen(true)}
        />
      )}

      {/* Safe Redaction & ATIF Export Modal */}
      {isExportOpen && activeTrace && (
        <ExportModal
          trace={activeTrace}
          onClose={() => setIsExportOpen(false)}
        />
      )}
    </div>
  );
}

export default App;
