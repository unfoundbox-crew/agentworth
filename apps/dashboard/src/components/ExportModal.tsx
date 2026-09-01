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
  onClose?: () => void;
  /**
   * Renders as a plain pane (no fixed backdrop, no close button) instead of
   * an overlay modal — used by the rail's "Exports" view, which is always
   * on-screen rather than opened/closed. The slideover-triggered export
   * flow (SessionInspector's "Export" button) keeps the modal presentation.
   */
  embedded?: boolean;
}

export const ExportModal: React.FC<ExportModalProps> = ({ trace, onClose, embedded = false }) => {
  const [format, setFormat] = useState<'atif' | 'raw'>('atif');
  const [redact, setRedact] = useState<boolean>(true);
  const [copied, setCopied] = useState<boolean>(false);
  const [isClosing, setIsClosing] = useState(false);

  // Exits are fast and stay in place (design.md "Motion rules"): fade out,
  // then unmount via the real onClose — no travel on the way out.
  const handleClose = () => {
    if (!onClose) return;
    setIsClosing(true);
    setTimeout(onClose, 120);
  };

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

  const outerClass = embedded
    ? 'h-full flex flex-col overflow-hidden'
    : `overlay-backdrop fixed inset-0 z-50 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 ${isClosing ? 'is-closing' : ''}`;

  const panelClass = embedded
    ? 'bg-ground flex-1 flex flex-col overflow-hidden min-h-0'
    : `modal-panel bg-ground border border-border rounded-2xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col overflow-hidden ${isClosing ? 'is-closing' : ''}`;

  return (
    <div className={outerClass}>
      <div className={panelClass}>

        {/* Header */}
        <div className="bg-ink text-ground p-4 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-2">
            <ShieldCheck className="w-4 h-4 text-success" />
            <span className="font-mono font-semibold text-sm uppercase tracking-wide">
              Safe redaction &amp; trajectory export
            </span>
          </div>
          {!embedded && (
            <button
              onClick={handleClose}
              className="text-ground/60 hover:text-ground transition-colors"
              aria-label="Close export modal"
            >
              <X className="w-5 h-5" />
            </button>
          )}
        </div>

        {/* Options Toolbar */}
        <div className="bg-surface border-b border-border p-3 sm:p-4 flex flex-wrap items-center justify-between gap-3 shrink-0">

          {/* Format Selector */}
          <div className="flex items-center gap-2">
            <span className="text-muted font-mono font-semibold text-[11px] uppercase">Format</span>
            <div className="theme-toggle" role="group" aria-label="Export format">
              <button
                type="button"
                onClick={() => setFormat('atif')}
                aria-pressed={format === 'atif'}
              >
                ATIF v1.1.0
              </button>
              <button
                type="button"
                onClick={() => setFormat('raw')}
                aria-pressed={format === 'raw'}
              >
                Trace JSON
              </button>
            </div>
          </div>

          {/* Redaction Toggle */}
          <div className="flex items-center gap-3">
            <label className="flex items-center gap-2 cursor-pointer select-none bg-ground border border-border rounded-lg px-3 py-1.5">
              <input
                type="checkbox"
                checked={redact}
                onChange={(e) => setRedact(e.target.checked)}
                className="w-3.5 h-3.5 rounded cursor-pointer"
                style={{ accentColor: 'var(--mv-accent)' }}
              />
              <span className="font-mono font-semibold text-xs text-ink">
                Redact secrets &amp; paths
              </span>
            </label>

            {redact ? (
              <span className="status-pill is-good">
                <span className="dot" />
                {redactedCount} items scrubbed
              </span>
            ) : (
              <span className="status-pill is-bad">
                <span className="dot" />
                Raw unredacted
              </span>
            )}
          </div>

        </div>

        {/* Redaction Category Summary (when redacted) */}
        {redact && (
          <div className="bg-success-soft border-b border-success-border/60 px-4 py-2 flex flex-wrap items-center gap-4 text-[11px] text-success font-mono shrink-0">
            <span className="font-semibold">Scrubbed categories</span>
            <span>API keys masked: <strong>{categories.api_keys}</strong></span>
            <span>Absolute home paths anonymized: <strong>{categories.file_paths}</strong></span>
            <span>Emails masked: <strong>{categories.emails}</strong></span>
          </div>
        )}

        {/* Code Preview Box */}
        <div className="flex-1 p-4 bg-ink overflow-y-auto max-h-[50vh]">
          <pre className="font-mono text-[11px] leading-snug whitespace-pre overflow-x-auto text-success/90">
            {jsonString}
          </pre>
        </div>

        {/* Footer Actions */}
        <div className="bg-surface border-t border-border p-4 flex items-center justify-between shrink-0">
          <div className="text-[11px] text-muted hidden sm:block font-mono">
            Target schema: <code className="font-semibold text-ink">{format === 'atif' ? 'ATIF v1.1.0' : 'AgentWorthSchema v0.1'}</code>
          </div>

          <div className="flex items-center gap-2.5 ml-auto">
            <button
              onClick={handleCopy}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-ground hover:bg-surface-2 text-ink font-mono text-xs font-semibold border border-border transition-colors"
            >
              {copied ? <Check className="w-3.5 h-3.5 text-success" /> : <Copy className="w-3.5 h-3.5" />}
              <span>{copied ? 'Copied to clipboard' : 'Copy JSON'}</span>
            </button>

            <button
              onClick={handleDownload}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded-lg bg-ink text-ground font-mono text-xs font-semibold hover:opacity-85 transition-opacity"
            >
              <Download className="w-3.5 h-3.5" />
              <span>Download file</span>
            </button>
          </div>
        </div>

      </div>
    </div>
  );
};
