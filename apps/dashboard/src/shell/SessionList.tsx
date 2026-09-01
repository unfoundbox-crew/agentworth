import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useSessions } from '../hooks/useSessions';
import { SessionSummary } from '../types';
import { determineReachedLevel } from './OutcomeLadder';

export interface ShellNav {
  next: () => void;
  prev: () => void;
}

export interface SessionListProps {
  selectedId: string | null;
  onSelect: (id: string) => void;
  registerNav: (nav: ShellNav) => void;
  liveTail: boolean;
  /** Bumped by the shell after a rescan, to re-read the index. */
  reloadSignal?: number;
}

type ChipKey = 'all' | 'ci' | 'failed' | 'claude_code';

const CHIPS: { key: ChipKey; label: string }[] = [
  { key: 'all', label: 'All' },
  { key: 'ci', label: 'CI Green' },
  { key: 'failed', label: 'Failed' },
  { key: 'claude_code', label: 'Claude Code' },
];

const ROW_HEIGHT = 30;

function dotClass(level: number): string {
  if (level >= 4) return 'ol-dot-success';
  if (level === 3) return 'ol-dot-warn';
  return 'ol-dot-danger';
}

function formatScore(score?: number): string {
  if (score === undefined || score === null || Number.isNaN(score)) return '—';
  return (score * 100).toFixed(0);
}

export function SessionList({ selectedId, onSelect, registerNav, liveTail, reloadSignal }: SessionListProps) {
  const { sessions, loading, error, refetch } = useSessions(reloadSignal);
  const [filterText, setFilterText] = useState('');
  const [chip, setChip] = useState<ChipKey>('all');
  const scrollRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    const text = filterText.trim().toLowerCase();
    return sessions.filter((s) => {
      const level = determineReachedLevel(s.primary_outcome);
      if (chip === 'ci' && level !== 5) return false;
      if (chip === 'failed' && level > 2) return false;
      if (chip === 'claude_code' && s.adapter !== 'claude_code') return false;
      if (text) {
        const haystack = `${s.session_id} ${s.adapter} ${s.models_used.join(' ')}`.toLowerCase();
        if (!haystack.includes(text)) return false;
      }
      return true;
    });
  }, [sessions, filterText, chip]);

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  useEffect(() => {
    const nav: ShellNav = {
      next: () => {
        if (!filtered.length) return;
        const idx = filtered.findIndex((s) => s.session_id === selectedId);
        const nextIdx = idx === -1 ? 0 : Math.min(idx + 1, filtered.length - 1);
        onSelect(filtered[nextIdx].session_id);
      },
      prev: () => {
        if (!filtered.length) return;
        const idx = filtered.findIndex((s) => s.session_id === selectedId);
        const prevIdx = idx === -1 ? 0 : Math.max(idx - 1, 0);
        onSelect(filtered[prevIdx].session_id);
      },
    };
    registerNav(nav);
  }, [filtered, selectedId, onSelect, registerNav]);

  useEffect(() => {
    if (!selectedId) return;
    const idx = filtered.findIndex((s) => s.session_id === selectedId);
    if (idx === -1) return;
    virtualizer.scrollToIndex(idx, { align: 'auto' });
    requestAnimationFrame(() => {
      const rowEl = scrollRef.current?.querySelector<HTMLElement>(
        `[data-row-id="${CSS.escape(selectedId)}"]`
      );
      rowEl?.scrollIntoView({ block: 'nearest' });
    });
    // Only re-run when the selection changes, not on every re-render caused
    // by scrolling itself.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  function handleChipClick(key: ChipKey) {
    setChip((prev) => (prev === key ? 'all' : key));
  }

  return (
    <section className="shell-list-pane">
      <div className="shell-list-controls">
        <input
          type="text"
          className="shell-filter-input"
          data-shell-filter
          placeholder="Filter sessions…"
          autoComplete="off"
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
        />
        <div className="shell-chip-row">
          {CHIPS.map((c) => (
            <button
              key={c.key}
              type="button"
              className="shell-chip"
              aria-pressed={c.key === 'all' ? chip === 'all' : chip === c.key}
              onClick={() => handleChipClick(c.key)}
            >
              {c.label}
            </button>
          ))}
        </div>
      </div>

      <div className="shell-list-scroll" ref={scrollRef}>
        {loading && (
          <div className="shell-list-skeleton" aria-hidden="true">
            {Array.from({ length: 14 }).map((_, i) => (
              <div key={i} className="shell-skeleton-row" />
            ))}
          </div>
        )}

        {!loading && error && (
          <div className="shell-list-error">
            <p>{error}</p>
            <button type="button" className="shell-retry-btn" onClick={refetch}>
              Retry
            </button>
          </div>
        )}

        {!loading && !error && sessions.length === 0 && (
          <div className="shell-list-empty">
            <p>No sessions indexed yet.</p>
            <p className="shell-list-empty-hint">
              Run <code>agentworth scan</code> to index coding-agent session logs on this machine.
            </p>
          </div>
        )}

        {!loading && !error && sessions.length > 0 && filtered.length === 0 && (
          <div className="shell-list-empty">
            <p>No sessions match this filter.</p>
          </div>
        )}

        {!loading && !error && filtered.length > 0 && (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualizer.getVirtualItems().map((vRow) => {
              const s: SessionSummary = filtered[vRow.index];
              const level = determineReachedLevel(s.primary_outcome);
              const selected = s.session_id === selectedId;
              return (
                <div
                  key={s.session_id}
                  data-row-id={s.session_id}
                  className={`shell-row${selected ? ' shell-row-selected' : ''}`}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    height: vRow.size,
                    transform: `translateY(${vRow.start}px)`,
                  }}
                  onClick={() => onSelect(s.session_id)}
                >
                  <span className={`shell-row-dot ${dotClass(level)}`} />
                  <span className="shell-row-id">{s.session_id}</span>
                  <span className="shell-row-agent">{s.adapter}</span>
                  <span className="shell-row-score">{formatScore(s.composite_score)}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="shell-list-footer">
        <span className="shell-hint">
          <kbd>j</kbd>
          <kbd>k</kbd> move
        </span>
        <span className="shell-hint">
          <kbd>&#8629;</kbd> inspect
        </span>
        <span className="shell-hint">
          <kbd>/</kbd> filter
        </span>
        <span className="shell-hint">
          <kbd>&#8984;K</kbd> palette
        </span>
        <span className="shell-hint">
          <kbd>esc</kbd> back
        </span>
        {liveTail && <span className="shell-hint shell-hint-live">live tail on</span>}
      </div>
    </section>
  );
}
