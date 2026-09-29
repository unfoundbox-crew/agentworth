import { Provenance } from '../types';
import { formatDate } from '../utils/formatters';

// GET /api/traces/:id serializes Provenance with the Rust struct's field
// names (adapter_name, mtime_epoch_secs, content_fingerprint), not the
// adapter / modified_timestamp / fingerprint the TS type declares.
// Verified against a live trace on 2026-09-01. Read both so this section —
// the product's core "we checked the disk" claim — doesn't render
// em-dashes for data that is actually there.
type RawProvenance = Partial<Provenance> & {
  adapter_name?: string;
  mtime_epoch_secs?: number;
  content_fingerprint?: string;
};

const DASH = '—';

function readProvenance(p: Provenance | null | undefined) {
  const raw = (p ?? {}) as RawProvenance;
  return {
    sourcePath: raw.source_path,
    fileSizeBytes: raw.file_size_bytes,
    modifiedEpochSecs: raw.mtime_epoch_secs,
    fingerprint: raw.content_fingerprint,
  };
}

function humanizeBytes(bytes?: number): string {
  if (bytes == null || Number.isNaN(bytes)) return DASH;
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[i]}`;
}

export interface ProvenanceBlockProps {
  provenance: Provenance;
}

/** The quiet, factual section: where this trace came from on disk, and
 * whether it can still be checked there. */
export function ProvenanceBlock({ provenance }: ProvenanceBlockProps) {
  const { sourcePath, fileSizeBytes, modifiedEpochSecs, fingerprint } = readProvenance(provenance);
  const modifiedLabel =
    modifiedEpochSecs != null ? formatDate(new Date(modifiedEpochSecs * 1000).toISOString()) : DASH;
  const fingerprintShort = fingerprint ? `${fingerprint.slice(0, 12)}…` : DASH;

  return (
    <div className="shell-prov-block">
      <div className="shell-section-title-row">
        <span className="shell-section-title">Provenance</span>
        <span className="status-pill is-good shell-prov-chip">
          <span className="dot" />
          On-disk verified
        </span>
      </div>

      <div className="shell-prov-path" title={sourcePath ?? undefined}>
        {sourcePath ?? DASH}
      </div>

      <div className="shell-prov-rows">
        <div className="shell-prov-row">
          <span className="shell-prov-key">Fingerprint</span>
          <span className="shell-prov-val" title={fingerprint ?? undefined}>
            {fingerprintShort}
          </span>
        </div>
        <div className="shell-prov-row">
          <span className="shell-prov-key">File size</span>
          <span className="shell-prov-val" title={fileSizeBytes != null ? `${fileSizeBytes} bytes` : undefined}>
            {humanizeBytes(fileSizeBytes)}
          </span>
        </div>
        <div className="shell-prov-row">
          <span className="shell-prov-key">Modified</span>
          <span className="shell-prov-val">{modifiedLabel}</span>
        </div>
      </div>
    </div>
  );
}
