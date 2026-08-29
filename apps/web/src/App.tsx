import { useState, useEffect } from 'react';
import { Navbar } from './components/Navbar';
import { LandingPage } from './components/LandingPage';
import { HeroReceipt } from './components/HeroReceipt';
import { ArchaeologyPanel } from './components/ArchaeologyPanel';
import { TracesExplorer } from './components/TracesExplorer';
import { SessionInspector } from './components/SessionInspector';
import { ExportModal } from './components/ExportModal';
import { Footer } from './components/Footer';

import {
  AggregateStats,
  SessionSummary,
  AgentWorthTrace,
} from './types';
import {
  fetchAggregateStats,
  fetchTraces,
  fetchTraceDetail,
  TraceQueryFilters,
} from './services/api';
import { mockAggregateStats, mockSummaries } from './services/mockData';

export function App() {
  const [viewMode, setViewMode] = useState<'landing' | 'explorer'>('landing');
  const [stats, setStats] = useState<AggregateStats>(mockAggregateStats);
  const [traces, setTraces] = useState<SessionSummary[]>(mockSummaries);
  const [totalTraces, setTotalTraces] = useState<number>(mockSummaries.length);
  const [filters, setFilters] = useState<TraceQueryFilters>({
    orderBy: 'started_at_desc',
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
    // If URL has ?view=explorer, start in explorer mode
    const params = new URLSearchParams(window.location.search);
    if (params.get('view') === 'explorer') {
      setViewMode('explorer');
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
      console.error('Failed loading data:', err);
    } finally {
      setIsLoadingTraces(false);
    }
  };

  const handleFilterChange = (newFilters: TraceQueryFilters) => {
    const merged = { ...filters, ...newFilters };
    setFilters(merged);
    loadData(merged);
  };

  const handleSelectSession = async (sessionId: string) => {
    setSelectedSessionId(sessionId);
    try {
      const detail = await fetchTraceDetail(sessionId);
      setActiveTrace(detail);
    } catch (err) {
      console.error('Failed loading session detail:', err);
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
    <div className="min-h-screen flex flex-col bg-[#fdfdfd] text-[#0a0a0c]">
      <Navbar
        onTriggerScan={handleTriggerScan}
        isScanning={isScanning}
        viewMode={viewMode}
        onToggleView={(mode) => setViewMode(mode)}
      />

      {viewMode === 'landing' ? (
        <LandingPage onOpenExplorer={() => setViewMode('explorer')} />
      ) : (
        <main className="flex-1">
          {/* 1. Hero & Physical Receipt Section */}
          <HeroReceipt stats={stats} onScanClick={handleTriggerScan} />

          {/* 2. Archaeology Panel */}
          {stats.archaeology && <ArchaeologyPanel data={stats.archaeology} />}

          {/* 3. Traces Explorer Table */}
          <TracesExplorer
            traces={traces}
            totalTraces={totalTraces}
            selectedSessionId={selectedSessionId || undefined}
            onSelectSession={handleSelectSession}
            onFilterChange={handleFilterChange}
            isLoading={isLoadingTraces}
          />

          {/* 4. Local-first Architecture Footer */}
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
