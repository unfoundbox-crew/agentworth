import React, { useState } from 'react';
import {
  AgentWorthTrace,
} from '../types';
import {
  formatTokens,
  formatDuration,
  formatDate,
  getOutcomeBadgeInfo,
  getAdapterBadge,
} from '../utils/formatters';
import {
  X,
  Terminal,
  FileCode,
  Brain,
  Shield,
  Download,
  RotateCcw,
  ChevronDown,
  ChevronRight,
  Code,
  Copy,
  Check,
  CheckCheck,
} from 'lucide-react';

interface SessionInspectorProps {
  trace: AgentWorthTrace;
  onClose: () => void;
  onOpenExport: () => void;
}

export const SessionInspector: React.FC<SessionInspectorProps> = ({
  trace,
  onClose,
  onOpenExport,
}) => {
  const [expandedThinking, setExpandedThinking] = useState<Record<string, boolean>>({});
  const [expandedTools, setExpandedTools] = useState<Record<string, boolean>>({});
  const [copiedId, setCopiedId] = useState(false);
  const [isClosing, setIsClosing] = useState(false);

  const toggleThinking = (id: string) => {
    setExpandedThinking((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const toggleTool = (id: string) => {
    setExpandedTools((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const handleCopyId = () => {
    navigator.clipboard.writeText(trace.session_id);
    setCopiedId(true);
    setTimeout(() => setCopiedId(false), 2000);
  };

  // Exits are fast and stay in place (design.md "Motion rules" — --motion-exit,
  // no travel): fade the overlay out, then unmount via the real onClose.
  const handleClose = () => {
    setIsClosing(true);
    setTimeout(onClose, 120);
  };

  const adapterBadge = getAdapterBadge(trace.adapter);
  const primaryOutcome = trace.outcomes?.[0]?.kind || 'done_claimed';
  const outcomeInfo = getOutcomeBadgeInfo(primaryOutcome);

  const totalTokens =
    (trace.stats?.token_usage?.input_tokens ?? 0) +
    (trace.stats?.token_usage?.output_tokens ?? 0) +
    (trace.stats?.token_usage?.cache_read_input_tokens ?? (trace.stats?.token_usage as any)?.cache_read_tokens ?? 0) +
    (trace.stats?.token_usage?.cache_creation_input_tokens ?? (trace.stats?.token_usage as any)?.cache_creation_tokens ?? 0);

  return (
    <div className={`overlay-backdrop fixed inset-0 z-50 bg-black/60 backdrop-blur-xs flex justify-end ${isClosing ? 'is-closing' : ''}`}>
      {/* Slideover Panel */}
      <div className={`slideover-panel w-full max-w-5xl bg-ground h-full shadow-2xl flex flex-col border-l border-border overflow-hidden ${isClosing ? 'is-closing' : ''}`}>

        {/* Top Header Bar */}
        <div className="bg-ink text-ground p-4 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-3 overflow-hidden">
            <span className="px-2 py-0.5 rounded text-[10px] font-mono uppercase font-semibold bg-ground/10 text-ground border border-ground/20 shrink-0">
              {adapterBadge.name}
            </span>
            <div className="flex items-center gap-2 truncate">
              <span className="font-mono font-semibold text-sm truncate">{trace.session_id}</span>
              <button
                onClick={handleCopyId}
                className="text-ground/60 hover:text-ground transition-colors shrink-0"
                title="Copy Session ID"
              >
                {copiedId ? <Check className="w-3.5 h-3.5 text-success" /> : <Copy className="w-3.5 h-3.5" />}
              </button>
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <button
              onClick={onOpenExport}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent text-accent-contrast font-mono text-xs font-semibold hover:opacity-85 transition-opacity"
            >
              <Download className="w-3.5 h-3.5" />
              <span className="hidden sm:inline">Safe redact &amp; ATIF export</span>
              <span className="sm:hidden">Export</span>
            </button>
            <button
              onClick={handleClose}
              className="p-1.5 rounded-lg text-ground/60 hover:text-ground hover:bg-ground/10 transition-colors"
              aria-label="Close inspector"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Sub-Header Metadata Ribbon */}
        <div className="bg-surface border-b border-border p-3 grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-6 gap-3 text-xs shrink-0">
          <div>
            <span className="text-faint block text-[9px] uppercase font-mono tracking-wide">Started at</span>
            <span className="font-semibold text-ink font-mono" style={{ fontVariantNumeric: 'tabular-nums' }}>{formatDate(trace.started_at)}</span>
          </div>
          <div>
            <span className="text-faint block text-[9px] uppercase font-mono tracking-wide">Duration</span>
            <span className="font-semibold text-ink font-mono" style={{ fontVariantNumeric: 'tabular-nums' }}>{formatDuration(trace.stats?.duration_seconds)}</span>
          </div>
          <div>
            <span className="text-faint block text-[9px] uppercase font-mono tracking-wide">Total tokens</span>
            <span className="font-semibold text-ink font-mono" style={{ fontVariantNumeric: 'tabular-nums' }}>{formatTokens(totalTokens)}</span>
          </div>
          <div>
            <span className="text-faint block text-[9px] uppercase font-mono tracking-wide">Models used</span>
            <span className="font-semibold text-ink font-mono truncate block">{trace.stats?.models_used?.join(', ') || '—'}</span>
          </div>
          <div>
            <span className="text-faint block text-[9px] uppercase font-mono tracking-wide">Outcome evidence</span>
            <span className={`inline-block px-1.5 py-0.5 rounded text-[9px] font-mono font-bold border ${outcomeInfo.className}`}>
              {outcomeInfo.label}
            </span>
          </div>
          <div>
            <span className="text-faint block text-[9px] uppercase font-mono tracking-wide">Composite score</span>
            <span className="font-semibold text-ink font-mono" style={{ fontVariantNumeric: 'tabular-nums' }}>
              {trace.score ? `${(trace.score.composite_score * 100).toFixed(0)} / 100` : 'N/A'}
            </span>
          </div>
        </div>

        {/* Main Content: Split Timeline & Score Sidebar */}
        <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 overflow-hidden">

          {/* Left: Event Stream Timeline (8 cols) */}
          <div className="lg:col-span-8 p-4 sm:p-6 overflow-y-auto space-y-4 border-r border-border bg-ground">

            {/* Recovery Loop Alert (if present) */}
            {trace.recoveries && trace.recoveries.length > 0 && (
              <div className="rounded-xl border border-warn-border bg-warn-soft p-3 space-y-2">
                <div className="flex items-center gap-2 text-warn font-mono font-semibold text-xs uppercase">
                  <RotateCcw className="w-4 h-4" />
                  <span>Recovery loop detected ({trace.recoveries.length} cycles)</span>
                </div>
                {trace.recoveries.map((rec, idx) => (
                  <div key={idx} className="text-[11px] text-text bg-ground/60 rounded-lg p-2.5 border border-warn-border/60 font-mono">
                    <div>
                      <span className="font-semibold text-danger">Failure [Step #{rec.failure_sequence}]: </span>
                      {rec.failure_summary}
                    </div>
                    <div className="mt-1">
                      <span className="font-semibold text-success">Resolution [Step #{rec.recovery_sequence}]: </span>
                      {rec.recovery_summary} ({rec.steps_to_recover} steps, {rec.corrective_actions_count} corrective actions)
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* Events Stream */}
            <div className="space-y-3">
              {trace.events.map((event) => (
                <div key={event.id} className="rounded-xl border border-border bg-ground overflow-hidden">

                  {/* Event Sequence Header */}
                  <div className="bg-surface border-b border-border px-3 py-2 flex items-center justify-between text-[11px]">
                    <div className="flex items-center gap-2">
                      <span className="w-5 h-5 rounded-md bg-surface-3 text-ink font-mono font-semibold flex items-center justify-center text-[10px]">
                        #{event.sequence}
                      </span>
                      <span className="font-mono font-semibold uppercase text-muted tracking-wide">
                        {event.payload.type.replace('_', ' ')}
                      </span>
                    </div>
                    <span className="text-[10px] text-faint font-mono">{formatDate(event.timestamp)}</span>
                  </div>

                  {/* Event Payload Rendering */}
                  <div className="p-3">
                    {/* 1. User Message */}
                    {event.payload.type === 'user_message' && (
                      <div className="rounded-lg border border-border bg-surface p-3 text-text whitespace-pre-wrap leading-relaxed text-sm">
                        <span className="font-mono font-semibold text-faint mr-2">&gt;</span>
                        {event.payload.data.content}
                      </div>
                    )}

                    {/* 2. Assistant Message */}
                    {event.payload.type === 'assistant_message' && (
                      <div className="space-y-2">
                        {event.payload.data.thinking && (
                          <div className="rounded-lg border border-border bg-surface overflow-hidden">
                            <button
                              onClick={() => toggleThinking(event.id)}
                              className="w-full px-2.5 py-1.5 flex items-center justify-between text-left text-muted hover:text-ink font-mono font-semibold text-[11px]"
                            >
                              <div className="flex items-center gap-1.5">
                                <Brain className="w-3.5 h-3.5" />
                                <span>Model thinking &amp; reasoning</span>
                              </div>
                              {expandedThinking[event.id] ? (
                                <ChevronDown className="w-3.5 h-3.5" />
                              ) : (
                                <ChevronRight className="w-3.5 h-3.5" />
                              )}
                            </button>
                            {expandedThinking[event.id] && (
                              <div className="p-3 text-[11px] text-muted bg-ground border-t border-border-soft whitespace-pre-wrap font-mono leading-relaxed max-h-60 overflow-y-auto">
                                {event.payload.data.thinking}
                              </div>
                            )}
                          </div>
                        )}
                        <div className="text-text whitespace-pre-wrap leading-relaxed text-sm">
                          {event.payload.data.content}
                        </div>
                      </div>
                    )}

                    {/* 3. Shell Command */}
                    {event.payload.type === 'shell_command' && (
                      <div className="bg-ink text-ground rounded-lg overflow-hidden">
                        <div className="border-b border-ground/10 px-3 py-1.5 flex items-center justify-between text-[11px]">
                          <div className="flex items-center gap-1.5 truncate">
                            <Terminal className="w-3.5 h-3.5 text-success" />
                            <code className="font-mono font-semibold truncate">
                              $ {event.payload.data.command}
                            </code>
                          </div>
                          <span
                            className={`px-1.5 py-0.5 rounded text-[9px] font-mono font-bold shrink-0 ml-2 ${
                              event.payload.data.exit_code === 0
                                ? 'bg-success-soft text-success'
                                : 'bg-danger-soft text-danger'
                            }`}
                          >
                            EXIT {event.payload.data.exit_code ?? '?'}
                          </span>
                        </div>
                        {event.payload.data.output && (
                          <pre className="p-3 text-[11px] text-ground/80 overflow-x-auto whitespace-pre font-mono max-h-64">
                            {event.payload.data.output}
                          </pre>
                        )}
                      </div>
                    )}

                    {/* 4. File Action / Diff */}
                    {event.payload.type === 'file_action' && (
                      <div className="rounded-lg border border-border bg-surface overflow-hidden">
                        <div className="bg-surface-2 px-3 py-1.5 border-b border-border flex items-center justify-between">
                          <div className="flex items-center gap-2 min-w-0">
                            <FileCode className="w-3.5 h-3.5 text-muted shrink-0" />
                            <span className="font-mono font-semibold text-ink text-xs truncate">{event.payload.data.path}</span>
                            <span className="tag-pill shrink-0">{event.payload.data.action}</span>
                          </div>
                          {event.payload.data.lines_changed !== undefined && (
                            <span className="text-[10px] font-mono text-muted font-semibold shrink-0 ml-2" style={{ fontVariantNumeric: 'tabular-nums' }}>
                              &Delta; {event.payload.data.lines_changed} lines
                            </span>
                          )}
                        </div>
                        {event.payload.data.diff && (
                          <div className="p-2 overflow-x-auto bg-ground max-h-64 font-mono text-[11px] leading-snug">
                            {event.payload.data.diff.split('\n').map((line, lIdx) => {
                              const isAdd = line.startsWith('+') && !line.startsWith('+++');
                              const isDel = line.startsWith('-') && !line.startsWith('---');
                              const isHeader = line.startsWith('@@') || line.startsWith('diff');

                              return (
                                <div
                                  key={lIdx}
                                  className={
                                    isAdd
                                      ? 'bg-success-soft text-success font-medium'
                                      : isDel
                                      ? 'bg-danger-soft text-danger font-medium'
                                      : isHeader
                                      ? 'text-accent'
                                      : 'text-muted'
                                  }
                                >
                                  {line}
                                </div>
                              );
                            })}
                          </div>
                        )}
                      </div>
                    )}

                    {/* 5. Tool Call */}
                    {event.payload.type === 'tool_call' && (
                      <div className="rounded-lg border border-border bg-surface overflow-hidden">
                        <button
                          onClick={() => toggleTool(event.id)}
                          className="w-full px-3 py-1.5 flex items-center justify-between text-left font-mono font-semibold text-ink bg-surface-2 border-b border-border text-xs"
                        >
                          <div className="flex items-center gap-2">
                            <Code className="w-3.5 h-3.5 text-muted" />
                            <span>Tool: {event.payload.data.name}</span>
                          </div>
                          {expandedTools[event.id] ? (
                            <ChevronDown className="w-3.5 h-3.5" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5" />
                          )}
                        </button>
                        {expandedTools[event.id] && (
                          <div className="p-2.5 bg-ground overflow-x-auto">
                            <pre className="text-[10px] text-text font-mono">
                              {typeof event.payload.data.arguments === 'string'
                                ? event.payload.data.arguments
                                : JSON.stringify(event.payload.data.arguments, null, 2)}
                            </pre>
                          </div>
                        )}
                      </div>
                    )}

                    {/* 6. Outcome Evidence */}
                    {event.payload.type === 'outcome_evidence' && (
                      <div className="rounded-lg border border-success-border bg-success-soft p-3 flex items-center justify-between gap-3">
                        <div className="flex items-center gap-2 min-w-0">
                          <CheckCheck className="w-4 h-4 text-success shrink-0" />
                          <span className="font-mono font-semibold text-success uppercase text-xs shrink-0">
                            {event.payload.data.kind.replace(/_/g, ' ')}
                          </span>
                          <span className="text-success/90 text-xs truncate">: {event.payload.data.summary}</span>
                        </div>
                        <span className="status-pill is-good shrink-0">
                          {(event.payload.data.confidence * 100).toFixed(0)}% confidence
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>

          </div>

          {/* Right: Score Breakdown & Provenance Sidebar (4 cols) */}
          <div className="lg:col-span-4 p-4 sm:p-5 overflow-y-auto bg-surface border-t lg:border-t-0 border-border space-y-4">

            {/* Composite Score Card */}
            {trace.score && (
              <div className="rounded-xl border border-border bg-ground p-4">
                <div className="flex items-center justify-between mb-3 border-b border-border-soft pb-2">
                  <span className="font-mono font-semibold text-xs uppercase text-ink tracking-wide">
                    Trace scoring matrix
                  </span>
                  <span className="text-sm font-bold text-ink font-mono bg-surface-3 px-2 py-0.5 rounded" style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {(trace.score.composite_score * 100).toFixed(0)} / 100
                  </span>
                </div>

                {/* Score Dimension Bars */}
                <div>
                  <div className="score-bar">
                    <div className="row">
                      <span>Outcome hierarchy</span>
                      <span className="v">{(trace.score.outcome_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="track"><div className="fill" style={{ width: `${trace.score.outcome_score * 100}%` }} /></div>
                  </div>

                  <div className="score-bar">
                    <div className="row">
                      <span>Verifiability (shell/git)</span>
                      <span className="v">{(trace.score.verifiability_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="track"><div className="fill" style={{ width: `${trace.score.verifiability_score * 100}%` }} /></div>
                  </div>

                  <div className="score-bar">
                    <div className="row">
                      <span>Complexity &amp; edits</span>
                      <span className="v">{(trace.score.complexity_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="track"><div className="fill" style={{ width: `${trace.score.complexity_score * 100}%` }} /></div>
                  </div>

                  <div className="score-bar">
                    <div className="row">
                      <span>Error recovery bonus</span>
                      <span className="v">{(trace.score.recovery_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="track"><div className="fill" style={{ width: `${trace.score.recovery_score * 100}%` }} /></div>
                  </div>

                  <div className="score-bar is-good">
                    <div className="row">
                      <span>Local provenance</span>
                      <span className="v">{(trace.score.provenance_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="track"><div className="fill" style={{ width: `${trace.score.provenance_score * 100}%` }} /></div>
                  </div>
                </div>

                {/* Explanations List */}
                <div className="mt-4 pt-3 border-t border-border-soft space-y-1.5 text-[11px] text-muted">
                  <span className="font-semibold text-ink block text-xs mb-1">Audit explanations</span>
                  {trace.score.explanations.map((exp, idx) => (
                    <div key={idx} className="flex items-start gap-1.5">
                      <span className="text-faint">&middot;</span>
                      <span>{exp}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Token Economics */}
            <div className="rounded-xl border border-border bg-ground p-4">
              <span className="font-mono font-semibold text-xs uppercase text-ink block mb-2 border-b border-border-soft pb-2 tracking-wide">
                Token economics
              </span>
              <div>
                <div className="kv-row">
                  <span className="k">Input tokens</span>
                  <span className="v">{trace.stats.token_usage.input_tokens.toLocaleString()}</span>
                </div>
                <div className="kv-row">
                  <span className="k">Output tokens</span>
                  <span className="v">{trace.stats.token_usage.output_tokens.toLocaleString()}</span>
                </div>
                <div className="kv-row">
                  <span className="k">Cache read</span>
                  <span className="v text-success">{trace.stats.token_usage.cache_read_input_tokens.toLocaleString()}</span>
                </div>
                <div className="kv-row">
                  <span className="k">Cache creation</span>
                  <span className="v">{trace.stats.token_usage.cache_creation_input_tokens.toLocaleString()}</span>
                </div>
                <div className="kv-row" style={{ borderTop: '1px solid var(--mv-border)', paddingTop: 8, marginTop: 2 }}>
                  <span className="k font-semibold text-ink">Total exhaust</span>
                  <span className="v">{formatTokens(totalTokens)}</span>
                </div>
              </div>
            </div>

            {/* Provenance Stamp */}
            <div className="rounded-xl border border-border bg-ground p-4">
              <div className="flex items-center gap-1.5 mb-2 border-b border-border-soft pb-2">
                <Shield className="w-3.5 h-3.5 text-muted" />
                <span className="font-mono font-semibold text-xs uppercase text-ink tracking-wide">
                  Local provenance
                </span>
              </div>
              <div className="space-y-1.5 text-[10px] text-muted break-all font-mono">
                <div>
                  <span className="text-faint block uppercase text-[9px] mb-0.5">Source file</span>
                  <code className="text-text">{trace.provenance.source_path}</code>
                </div>
                <div>
                  <span className="text-faint block uppercase text-[9px] mb-0.5">Fingerprint</span>
                  <code className="text-text">{trace.provenance.fingerprint}</code>
                </div>
                <div className="flex justify-between items-center pt-2 mt-1 border-t border-border-soft">
                  <span>File size: {(trace.provenance.file_size_bytes / 1024).toFixed(1)} KB</span>
                  <span className="status-pill is-good" style={{ padding: '2px 7px' }}>On-disk verified</span>
                </div>
              </div>
            </div>

          </div>

        </div>

      </div>
    </div>
  );
};
