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

  const adapterBadge = getAdapterBadge(trace.adapter);
  const primaryOutcome = trace.outcomes?.[0]?.kind || 'done_claimed';
  const outcomeInfo = getOutcomeBadgeInfo(primaryOutcome);

  const totalTokens =
    (trace.stats?.token_usage?.input_tokens ?? 0) +
    (trace.stats?.token_usage?.output_tokens ?? 0) +
    (trace.stats?.token_usage?.cache_read_input_tokens ?? 0) +
    (trace.stats?.token_usage?.cache_creation_input_tokens ?? 0);

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-xs flex justify-end">
      {/* Slideover Panel */}
      <div className="w-full max-w-5xl bg-[#fdfdfd] h-full shadow-2xl flex flex-col border-l-2 border-zinc-900 font-mono text-xs overflow-hidden animate-in slide-in-from-right duration-200">
        
        {/* Top Header Bar */}
        <div className="bg-zinc-900 text-white p-4 flex items-center justify-between border-b-2 border-zinc-950">
          <div className="flex items-center space-x-3 overflow-hidden">
            <span className={`px-2 py-0.5 text-[10px] uppercase font-bold border ${adapterBadge.borderColor} bg-white text-black`}>
              {adapterBadge.name}
            </span>
            <div className="flex items-center space-x-2 truncate">
              <span className="font-bold text-sm truncate text-white">{trace.session_id}</span>
              <button
                onClick={handleCopyId}
                className="text-zinc-400 hover:text-white transition-colors"
                title="Copy Session ID"
              >
                {copiedId ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
              </button>
            </div>
          </div>

          <div className="flex items-center space-x-3">
            <button
              onClick={onOpenExport}
              className="flex items-center space-x-1.5 px-3 py-1 bg-emerald-600 hover:bg-emerald-500 text-black font-bold border border-emerald-400 transition-colors shadow-[1px_1px_0px_0px_rgba(0,0,0,1)]"
            >
              <Download className="w-3.5 h-3.5" />
              <span>Safe Redact & ATIF Export</span>
            </button>
            <button
              onClick={onClose}
              className="p-1 text-zinc-400 hover:text-white hover:bg-zinc-800 transition-colors"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Sub-Header Metadata Ribbon */}
        <div className="bg-zinc-100 border-b border-zinc-300 p-3 grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-6 gap-2 text-[11px] text-zinc-700">
          <div>
            <span className="text-zinc-500 block text-[9px] uppercase">Started At</span>
            <span className="font-semibold text-black">{formatDate(trace.started_at)}</span>
          </div>
          <div>
            <span className="text-zinc-500 block text-[9px] uppercase">Duration</span>
            <span className="font-semibold text-black">{formatDuration(trace.stats?.duration_seconds)}</span>
          </div>
          <div>
            <span className="text-zinc-500 block text-[9px] uppercase">Total Tokens</span>
            <span className="font-semibold text-black">{formatTokens(totalTokens)}</span>
          </div>
          <div>
            <span className="text-zinc-500 block text-[9px] uppercase">Models Used</span>
            <span className="font-semibold text-black">{trace.stats?.models_used?.join(', ') || '—'}</span>
          </div>
          <div>
            <span className="text-zinc-500 block text-[9px] uppercase">Outcome Evidence</span>
            <span className={`inline-block px-1.5 py-0.5 text-[9px] border font-bold ${outcomeInfo.className}`}>
              {outcomeInfo.label}
            </span>
          </div>
          <div>
            <span className="text-zinc-500 block text-[9px] uppercase">Composite Score</span>
            <span className="font-bold text-black">
              {trace.score ? `${(trace.score.composite_score * 100).toFixed(0)} / 100` : 'N/A'}
            </span>
          </div>
        </div>

        {/* Main Content: Split Timeline & Score Sidebar */}
        <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 overflow-hidden">
          
          {/* Left: Event Stream Timeline (8 cols) */}
          <div className="lg:col-span-8 p-4 sm:p-6 overflow-y-auto space-y-4 border-r border-zinc-300 bg-[#fdfdfd]">
            
            {/* Recovery Loop Alert (if present) */}
            {trace.recoveries && trace.recoveries.length > 0 && (
              <div className="bg-amber-50 border-2 border-amber-600 p-3 shadow-[2px_2px_0px_0px_rgba(217,119,6,1)] space-y-1.5">
                <div className="flex items-center space-x-2 text-amber-900 font-bold text-xs uppercase">
                  <RotateCcw className="w-4 h-4 text-amber-700" />
                  <span>RECOVERY LOOP DETECTED ({trace.recoveries.length} cycles)</span>
                </div>
                {trace.recoveries.map((rec, idx) => (
                  <div key={idx} className="text-[11px] text-amber-950 bg-white/70 p-2 border border-amber-300">
                    <div>
                      <span className="font-bold text-red-700">Failure [Step #{rec.failure_sequence}]: </span>
                      {rec.failure_summary}
                    </div>
                    <div className="mt-1">
                      <span className="font-bold text-emerald-700">Resolution [Step #{rec.recovery_sequence}]: </span>
                      {rec.recovery_summary} ({rec.steps_to_recover} steps, {rec.corrective_actions_count} corrective actions)
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* Events Stream */}
            <div className="space-y-4">
              {trace.events.map((event) => (
                <div key={event.id} className="border border-zinc-900 bg-white shadow-[2px_2px_0px_0px_rgba(0,0,0,1)]">
                  
                  {/* Event Sequence Header */}
                  <div className="bg-zinc-100 border-b border-zinc-300 px-3 py-1.5 flex items-center justify-between text-[11px]">
                    <div className="flex items-center space-x-2">
                      <span className="w-5 h-5 rounded-none bg-black text-white font-bold flex items-center justify-center text-[10px]">
                        #{event.sequence}
                      </span>
                      <span className="font-bold uppercase text-zinc-800">
                        {event.payload.type.replace('_', ' ')}
                      </span>
                    </div>
                    <span className="text-[10px] text-zinc-500">{formatDate(event.timestamp)}</span>
                  </div>

                  {/* Event Payload Rendering */}
                  <div className="p-3">
                    {/* 1. User Message */}
                    {event.payload.type === 'user_message' && (
                      <div className="bg-zinc-50 border border-zinc-300 p-3 text-zinc-900 whitespace-pre-wrap leading-relaxed">
                        <span className="font-bold text-zinc-500 mr-2">&gt;</span>
                        {event.payload.data.content}
                      </div>
                    )}

                    {/* 2. Assistant Message */}
                    {event.payload.type === 'assistant_message' && (
                      <div className="space-y-2">
                        {event.payload.data.thinking && (
                          <div className="border border-zinc-300 bg-zinc-50">
                            <button
                              onClick={() => toggleThinking(event.id)}
                              className="w-full px-2.5 py-1.5 flex items-center justify-between text-left text-zinc-600 hover:text-black font-semibold text-[11px] bg-zinc-100/70"
                            >
                              <div className="flex items-center space-x-1.5">
                                <Brain className="w-3.5 h-3.5 text-zinc-700" />
                                <span>Model Thinking &amp; Reasoning</span>
                              </div>
                              {expandedThinking[event.id] ? (
                                <ChevronDown className="w-3.5 h-3.5" />
                              ) : (
                                <ChevronRight className="w-3.5 h-3.5" />
                              )}
                            </button>
                            {expandedThinking[event.id] && (
                              <div className="p-3 text-[11px] text-zinc-700 bg-zinc-50 border-t border-zinc-200 whitespace-pre-wrap font-mono leading-relaxed max-h-60 overflow-y-auto">
                                {event.payload.data.thinking}
                              </div>
                            )}
                          </div>
                        )}
                        <div className="text-zinc-900 whitespace-pre-wrap leading-relaxed">
                          {event.payload.data.content}
                        </div>
                      </div>
                    )}

                    {/* 3. Shell Command */}
                    {event.payload.type === 'shell_command' && (
                      <div className="bg-zinc-950 text-zinc-200 border border-zinc-900">
                        <div className="bg-zinc-900 border-b border-zinc-800 px-3 py-1.5 flex items-center justify-between text-[11px]">
                          <div className="flex items-center space-x-1.5 truncate">
                            <Terminal className="w-3.5 h-3.5 text-emerald-400" />
                            <code className="text-emerald-400 font-bold truncate">
                              $ {event.payload.data.command}
                            </code>
                          </div>
                          <span
                            className={`px-1.5 py-0.2 text-[9px] font-bold border ${
                              event.payload.data.exit_code === 0
                                ? 'bg-emerald-950 text-emerald-300 border-emerald-700'
                                : 'bg-red-950 text-red-300 border-red-700'
                            }`}
                          >
                            EXIT {event.payload.data.exit_code ?? '?'}
                          </span>
                        </div>
                        {event.payload.data.output && (
                          <pre className="p-3 text-[11px] text-zinc-300 overflow-x-auto whitespace-pre font-mono max-h-64">
                            {event.payload.data.output}
                          </pre>
                        )}
                      </div>
                    )}

                    {/* 4. File Action / Diff */}
                    {event.payload.type === 'file_action' && (
                      <div className="border border-zinc-300 bg-zinc-50">
                        <div className="bg-zinc-100 px-3 py-1.5 border-b border-zinc-300 flex items-center justify-between">
                          <div className="flex items-center space-x-2">
                            <FileCode className="w-3.5 h-3.5 text-zinc-700" />
                            <span className="font-bold text-black">{event.payload.data.path}</span>
                            <span className="text-[10px] uppercase font-semibold px-1 py-0.2 bg-zinc-200 border border-zinc-300">
                              {event.payload.data.action}
                            </span>
                          </div>
                          {event.payload.data.lines_changed !== undefined && (
                            <span className="text-[10px] text-zinc-600 font-semibold">
                              Δ {event.payload.data.lines_changed} lines
                            </span>
                          )}
                        </div>
                        {event.payload.data.diff && (
                          <div className="p-2 overflow-x-auto bg-white max-h-64 font-mono text-[11px] leading-snug">
                            {event.payload.data.diff.split('\n').map((line, lIdx) => {
                              const isAdd = line.startsWith('+') && !line.startsWith('+++');
                              const isDel = line.startsWith('-') && !line.startsWith('---');
                              const isHeader = line.startsWith('@@') || line.startsWith('diff');

                              return (
                                <div
                                  key={lIdx}
                                  className={
                                    isAdd
                                      ? 'bg-emerald-50 text-emerald-800 font-semibold'
                                      : isDel
                                      ? 'bg-red-50 text-red-800 font-semibold'
                                      : isHeader
                                      ? 'text-purple-700 bg-purple-50/50'
                                      : 'text-zinc-700'
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
                      <div className="border border-zinc-300 bg-zinc-50">
                        <button
                          onClick={() => toggleTool(event.id)}
                          className="w-full px-3 py-1.5 flex items-center justify-between text-left font-bold text-zinc-800 bg-zinc-100 border-b border-zinc-300"
                        >
                          <div className="flex items-center space-x-2">
                            <Code className="w-3.5 h-3.5 text-zinc-600" />
                            <span>Tool: {event.payload.data.name}</span>
                          </div>
                          {expandedTools[event.id] ? (
                            <ChevronDown className="w-3.5 h-3.5" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5" />
                          )}
                        </button>
                        <div className="p-2.5 bg-white overflow-x-auto">
                          <pre className="text-[10px] text-zinc-800 font-mono">
                            {typeof event.payload.data.arguments === 'string'
                              ? event.payload.data.arguments
                              : JSON.stringify(event.payload.data.arguments, null, 2)}
                          </pre>
                        </div>
                      </div>
                    )}

                    {/* 6. Outcome Evidence */}
                    {event.payload.type === 'outcome_evidence' && (
                      <div className="bg-emerald-50 border-2 border-emerald-600 p-3 flex items-center justify-between">
                        <div className="flex items-center space-x-2">
                          <CheckCheck className="w-4 h-4 text-emerald-700" />
                          <span className="font-bold text-emerald-950 uppercase">
                            {event.payload.data.kind.replace(/_/g, ' ')}
                          </span>
                          <span className="text-emerald-800">: {event.payload.data.summary}</span>
                        </div>
                        <span className="text-[10px] font-bold text-emerald-800 px-1.5 py-0.5 bg-emerald-200 border border-emerald-400">
                          {(event.payload.data.confidence * 100).toFixed(0)}% CONFIDENCE
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>

          </div>

          {/* Right: Score Breakdown & Provenance Sidebar (4 cols) */}
          <div className="lg:col-span-4 p-4 sm:p-5 overflow-y-auto bg-zinc-50 border-t lg:border-t-0 border-zinc-300 space-y-5">
            
            {/* Composite Score Card */}
            {trace.score && (
              <div className="border-2 border-zinc-900 bg-white p-4 shadow-[3px_3px_0px_0px_rgba(0,0,0,1)]">
                <div className="flex items-center justify-between mb-3 border-b border-zinc-200 pb-2">
                  <span className="font-bold text-xs uppercase text-black">
                    TRACE SCORING MATRIX
                  </span>
                  <span className="text-sm font-extrabold text-black bg-zinc-100 px-2 py-0.5 border border-zinc-900">
                    {(trace.score.composite_score * 100).toFixed(0)} / 100
                  </span>
                </div>

                {/* Score Dimension Bars */}
                <div className="space-y-2.5 text-[11px]">
                  <div>
                    <div className="flex justify-between text-zinc-600 mb-0.5">
                      <span>Outcome Hierarchy</span>
                      <span className="font-bold text-black">{(trace.score.outcome_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="w-full bg-zinc-200 h-2 border border-zinc-400">
                      <div
                        className="bg-black h-full"
                        style={{ width: `${trace.score.outcome_score * 100}%` }}
                      ></div>
                    </div>
                  </div>

                  <div>
                    <div className="flex justify-between text-zinc-600 mb-0.5">
                      <span>Verifiability (Shell/Git)</span>
                      <span className="font-bold text-black">{(trace.score.verifiability_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="w-full bg-zinc-200 h-2 border border-zinc-400">
                      <div
                        className="bg-black h-full"
                        style={{ width: `${trace.score.verifiability_score * 100}%` }}
                      ></div>
                    </div>
                  </div>

                  <div>
                    <div className="flex justify-between text-zinc-600 mb-0.5">
                      <span>Complexity &amp; Edits</span>
                      <span className="font-bold text-black">{(trace.score.complexity_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="w-full bg-zinc-200 h-2 border border-zinc-400">
                      <div
                        className="bg-black h-full"
                        style={{ width: `${trace.score.complexity_score * 100}%` }}
                      ></div>
                    </div>
                  </div>

                  <div>
                    <div className="flex justify-between text-zinc-600 mb-0.5">
                      <span>Error Recovery Bonus</span>
                      <span className="font-bold text-black">{(trace.score.recovery_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="w-full bg-zinc-200 h-2 border border-zinc-400">
                      <div
                        className="bg-black h-full"
                        style={{ width: `${trace.score.recovery_score * 100}%` }}
                      ></div>
                    </div>
                  </div>

                  <div>
                    <div className="flex justify-between text-zinc-600 mb-0.5">
                      <span>Local Provenance</span>
                      <span className="font-bold text-black">{(trace.score.provenance_score * 100).toFixed(0)}%</span>
                    </div>
                    <div className="w-full bg-zinc-200 h-2 border border-zinc-400">
                      <div
                        className="bg-emerald-600 h-full"
                        style={{ width: `${trace.score.provenance_score * 100}%` }}
                      ></div>
                    </div>
                  </div>
                </div>

                {/* Explanations List */}
                <div className="mt-4 pt-3 border-t border-zinc-200 space-y-1.5 text-[10px] text-zinc-600">
                  <span className="font-bold text-black block text-[11px]">Audit Explanations:</span>
                  {trace.score.explanations.map((exp, idx) => (
                    <div key={idx} className="flex items-start space-x-1">
                      <span className="text-zinc-400">•</span>
                      <span>{exp}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Token Economics */}
            <div className="border border-zinc-900 bg-white p-4 shadow-[2px_2px_0px_0px_rgba(0,0,0,1)]">
              <span className="font-bold text-xs uppercase text-black block mb-2 border-b border-zinc-200 pb-1">
                TOKEN ECONOMICS
              </span>
              <div className="space-y-1.5 text-[11px] text-zinc-700">
                <div className="flex justify-between">
                  <span className="text-zinc-500">Input Tokens:</span>
                  <span className="font-bold text-black">{(trace.stats?.token_usage?.input_tokens ?? 0).toLocaleString()}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Output Tokens:</span>
                  <span className="font-bold text-black">{(trace.stats?.token_usage?.output_tokens ?? 0).toLocaleString()}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Cache Read:</span>
                  <span className="font-bold text-emerald-700">{(trace.stats?.token_usage?.cache_read_input_tokens ?? 0).toLocaleString()}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Cache Creation:</span>
                  <span className="font-bold text-zinc-700">{(trace.stats?.token_usage?.cache_creation_input_tokens ?? 0).toLocaleString()}</span>
                </div>
                <div className="flex justify-between pt-1 border-t border-zinc-200 font-bold text-black">
                  <span>Total Exhaust:</span>
                  <span>{formatTokens(totalTokens)}</span>
                </div>
              </div>
            </div>

            {/* Provenance Stamp */}
            <div className="border border-zinc-900 bg-white p-4 shadow-[2px_2px_0px_0px_rgba(0,0,0,1)]">
              <div className="flex items-center space-x-1.5 mb-2 border-b border-zinc-200 pb-1">
                <Shield className="w-3.5 h-3.5 text-zinc-700" />
                <span className="font-bold text-xs uppercase text-black">
                  LOCAL PROVENANCE
                </span>
              </div>
              <div className="space-y-1.5 text-[10px] text-zinc-600 break-all">
                <div>
                  <span className="text-zinc-400 block uppercase text-[9px]">Source File</span>
                  <code className="text-zinc-800">{trace.provenance.source_path}</code>
                </div>
                <div>
                  <span className="text-zinc-400 block uppercase text-[9px]">Fingerprint</span>
                  <code className="text-zinc-800">{trace.provenance.fingerprint}</code>
                </div>
                <div className="flex justify-between pt-1 border-t border-zinc-200">
                  <span>File Size: {(trace.provenance.file_size_bytes / 1024).toFixed(1)} KB</span>
                  <span className="text-emerald-700 font-bold">✓ ON-DISK VERIFIED</span>
                </div>
              </div>
            </div>

          </div>

        </div>

      </div>
    </div>
  );
};
