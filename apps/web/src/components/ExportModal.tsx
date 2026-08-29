import React, { useState, useMemo } from 'react';
import {
  AgentWorthTrace,
} from '../types';
import {
  performClientSideRedaction,
  convertToAtif,
} from '../services/api';
import {
  X,
  Download,
  Copy,
  Check,
  ShieldCheck,
} from 'lucide-react';

interface ExportModalProps {
  trace: AgentWorthTrace;
  onClose: () => void;
}

export const ExportModal: React.FC<ExportModalProps> = ({ trace, onClose }) => {
  const [format, setFormat] = useState<'atif' | 'raw'>('atif');
  const [redact, setRedact] = useState<boolean>(true);
  const [copied, setCopied] = useState<boolean>(false);

  // Compute redaction
  const { redactedTrace, redactedCount, categories } = useMemo(() => {
    return performClientSideRedaction(trace);
  }, [trace]);

  const activeTrace = redact ? redactedTrace : trace;

  // Generate payload string
  const exportData = useMemo(() => {
    if (format === 'atif') {
      return convertToAtif(activeTrace);
    }
    return activeTrace;
  }, [activeTrace, format]);

  const jsonString = useMemo(() => {
    return JSON.stringify(exportData, null, 2);
  }, [exportData]);

  const handleCopy = () => {
    navigator.clipboard.writeText(jsonString);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownload = () => {
    const filename = `${trace.session_id}${redact ? '.redacted' : ''}.${format === 'atif' ? 'atif.json' : 'json'}`;
    const blob = new Blob([jsonString], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4">
      <div className="bg-[#fdfdfd] border-2 border-zinc-900 shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] w-full max-w-4xl max-h-[90vh] flex flex-col font-mono text-xs overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        
        {/* Header */}
        <div className="bg-zinc-900 text-white p-4 flex items-center justify-between border-b-2 border-zinc-950">
          <div className="flex items-center space-x-2">
            <ShieldCheck className="w-4 h-4 text-emerald-400" />
            <span className="font-bold text-sm uppercase text-white">
              SAFE REDACTION &amp; TRAJECTORY EXPORT
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-zinc-400 hover:text-white transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Options Toolbar */}
        <div className="bg-zinc-100 border-b border-zinc-300 p-3 sm:p-4 flex flex-wrap items-center justify-between gap-3">
          
          {/* Format Selector */}
          <div className="flex items-center space-x-2">
            <span className="text-zinc-600 font-bold text-[11px] uppercase">Format:</span>
            <div className="inline-flex border border-zinc-900 bg-white">
              <button
                onClick={() => setFormat('atif')}
                className={`px-3 py-1 text-xs font-bold transition-colors ${
                  format === 'atif'
                    ? 'bg-black text-white'
                    : 'text-zinc-700 hover:bg-zinc-100'
                }`}
              >
                ATIF v1.1.0 Standard
              </button>
              <button
                onClick={() => setFormat('raw')}
                className={`px-3 py-1 text-xs font-bold border-l border-zinc-900 transition-colors ${
                  format === 'raw'
                    ? 'bg-black text-white'
                    : 'text-zinc-700 hover:bg-zinc-100'
                }`}
              >
                AgentWorth Trace JSON
              </button>
            </div>
          </div>

          {/* Redaction Toggle */}
          <div className="flex items-center space-x-3">
            <label className="flex items-center space-x-2 cursor-pointer select-none bg-white border border-zinc-900 px-3 py-1 shadow-[1px_1px_0px_0px_rgba(0,0,0,1)]">
              <input
                type="checkbox"
                checked={redact}
                onChange={(e) => setRedact(e.target.checked)}
                className="w-3.5 h-3.5 accent-black rounded-none cursor-pointer"
              />
              <span className="font-bold text-xs text-black">
                Redact Secrets &amp; Paths
              </span>
            </label>

            {redact ? (
              <span className="px-2 py-0.5 bg-emerald-100 border border-emerald-500 text-emerald-800 text-[10px] font-bold">
                ✓ {redactedCount} items scrubbed
              </span>
            ) : (
              <span className="px-2 py-0.5 bg-red-100 border border-red-400 text-red-800 text-[10px] font-bold">
                ⚠ RAW UNREDACTED
              </span>
            )}
          </div>

        </div>

        {/* Redaction Category Summary (when redacted) */}
        {redact && (
          <div className="bg-emerald-50/80 border-b border-emerald-200 px-4 py-2 flex flex-wrap items-center gap-4 text-[10px] text-emerald-900 font-mono">
            <span className="font-bold">Scrubbed categories:</span>
            <span>API Keys masked: <strong>{categories.api_keys}</strong></span>
            <span>Absolute home paths anonymized: <strong>{categories.file_paths}</strong></span>
            <span>Emails masked: <strong>{categories.emails}</strong></span>
          </div>
        )}

        {/* Code Preview Box */}
        <div className="flex-1 p-4 bg-zinc-950 text-zinc-200 overflow-y-auto max-h-[50vh]">
          <pre className="font-mono text-[11px] leading-snug whitespace-pre overflow-x-auto text-emerald-300">
            {jsonString}
          </pre>
        </div>

        {/* Footer Actions */}
        <div className="bg-zinc-100 border-t border-zinc-300 p-4 flex items-center justify-between">
          <div className="text-[11px] text-zinc-500 hidden sm:block">
            Target schema: <code className="font-bold text-black">{format === 'atif' ? 'ATIF v1.1.0' : 'AgentWorthSchema v0.1'}</code>
          </div>

          <div className="flex items-center space-x-3 ml-auto">
            <button
              onClick={handleCopy}
              className="flex items-center space-x-1 px-3 py-1.5 bg-white hover:bg-zinc-200 text-black font-bold border border-zinc-900 shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] transition-colors"
            >
              {copied ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
              <span>{copied ? 'Copied to Clipboard' : 'Copy JSON'}</span>
            </button>

            <button
              onClick={handleDownload}
              className="flex items-center space-x-1.5 px-4 py-1.5 bg-black hover:bg-zinc-800 text-white font-bold border border-black shadow-[2px_2px_0px_0px_rgba(0,0,0,0.5)] transition-colors"
            >
              <Download className="w-3.5 h-3.5" />
              <span>Download File</span>
            </button>
          </div>
        </div>

      </div>
    </div>
  );
};
