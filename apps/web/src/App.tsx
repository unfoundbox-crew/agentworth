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
import { ErrorBoundary } from "./components/ErrorBoundary";

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
    <div className="min-h-screen flex flex-col bg-[#fdfdfd] dark:bg-[#0a0a0c] text-[#0a0a0c] dark:text-[#ececed]">
      {viewMode === "explorer" && (
        <Navbar
          onTriggerScan={handleTriggerScan}
          isScanning={isScanning}
          viewMode={viewMode}
          onToggleView={(mode) => setViewMode(mode)}
        />
      )}

      {viewMode === "landing" ? (
        <LandingPage onOpenExplorer={() => setViewMode("explorer")} />
      ) : (
        <main className="flex-1 space-y-8 max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6">
          {/* 1. Hero & Physical Receipt Section */}
          <ErrorBoundary label="Agent Receipt">
            <HeroReceipt stats={stats} onScanClick={handleTriggerScan} />
          </ErrorBoundary>

          {/* 2. Top Panel: The Verdict Board */}
          <ErrorBoundary label="Verdict Board">
            <VerdictBoard
              stats={stats}
              selectedRung={(filters.outcome as OutcomeKind) || null}
              onSelectRung={handleSelectRung}
            />
          </ErrorBoundary>

          {/* 3. Archaeology Panel (if present in local index) */}
          {stats.archaeology && (
            <ErrorBoundary label="Archaeology Panel">
              <ArchaeologyPanel data={stats.archaeology} />
            </ErrorBoundary>
          )}

          {/* 4. Traces Explorer Table */}
          <ErrorBoundary label="Traces Explorer">
            <TracesExplorer
              traces={traces}
              totalTraces={totalTraces}
              selectedSessionId={selectedSessionId || undefined}
              onSelectSession={handleSelectSession}
              onFilterChange={handleFilterChange}
              isLoading={isLoadingTraces}
            />
          </ErrorBoundary>

          {/* 5. The Cache Cliff Interactive Widget */}
          <ErrorBoundary label="Cache Cliff Widget">
            <CacheCliffWidget />
          </ErrorBoundary>

          {/* 6. Grounded Coverage Matrix */}
          <ErrorBoundary label="Coverage Matrix">
            <CoverageMatrix />
          </ErrorBoundary>

          {/* 7. Local-first Architecture Footer */}
          <Footer />
        </main>
      )}

      {/* Session Inspector Slideover */}
      {activeTrace && (
        <ErrorBoundary label="Session Inspector">
          <SessionInspector
            trace={activeTrace}
            onClose={handleCloseInspector}
            onOpenExport={() => setIsExportOpen(true)}
          />
        </ErrorBoundary>
      )}

      {/* Safe Redaction & ATIF Export Modal */}
      {isExportOpen && activeTrace && (
        <ErrorBoundary label="Export Modal">
          <ExportModal
            trace={activeTrace}
            onClose={() => setIsExportOpen(false)}
          />
        </ErrorBoundary>
      )}
    </div>
  );
}

export default App;
