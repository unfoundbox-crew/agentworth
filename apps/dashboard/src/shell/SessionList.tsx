import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useSessions } from '../hooks/useSessions';
import { useResizableWidth } from '../hooks/useResizableWidth';
import { SessionSummary } from '../types';
import { determineReachedLevel } from './OutcomeLadder';
import { formatTokens } from '../utils/formatters';
import {
  flattenGroups,
  groupSessions,
  type GroupMode,
  type ListRow,
} from '../utils/sessionGrouping';

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
  { key: 'failed', label: 'Unverified' },
  { key: 'claude_code', label: 'Claude Code' },
];

type SortKey =
  | 'started_desc'
  | 'started_asc'
  | 'score_desc'
  | 'tokens_desc'
  | 'tokens_asc'
  | 'duration_desc'
  | 'events_desc';

const SORT_OPTIONS: { key: SortKey; label: string }[] = [
  { key: 'started_desc', label: 'Newest first' },
  { key: 'started_asc', label: 'Oldest first' },
  { key: 'score_desc', label: 'Highest score' },
  { key: 'tokens_desc', label: 'Most tokens' },
  { key: 'tokens_asc', label: 'Least tokens' },
  { key: 'duration_desc', label: 'Longest duration' },
  { key: 'events_desc', label: 'Most events' },
];

const GROUP_OPTIONS: { key: GroupMode; label: string }[] = [
  { key: 'none', label: 'Flat list' },
  { key: 'repo', label: 'By repo' },
  { key: 'worktree', label: 'By worktree' },
  { key: 'subagent', label: 'By subagent' },
];

type Density = 'compact' | 'comfortable';

const COMPACT_ROW_HEIGHT = 30;
const COMFORTABLE_ROW_HEIGHT = 48;
const HEADER_ROW_HEIGHT = 26;

const LIST_WIDTH_KEY = 'agentworth.listWidth';
const LIST_WIDTH_DEFAULT = 380;
const LIST_WIDTH_MIN = 260;
const LIST_WIDTH_MAX = 720;

function dotClass(level: number): string {
  // Confidence, not alarm. Nothing here means failure — see list.css.
  if (level >= 4) return 'ol-dot-success'; // externally verified
  if (level === 3) return 'ol-dot-warn';   // machine-checked, local
  if (level === 2) return 'ol-dot-danger'; // something moved on disk
  return 'ol-dot-none';                    // the agent's word alone
}

function formatScore(score?: number): string {
  if (score === undefined || score === null || Number.isNaN(score)) return '—';
  return (score * 100).toFixed(0);
}

/**
 * Compact duration for the narrow row column — "42s" / "44m" / "2h15m".
 * Deliberately drops the finer unit once a coarser one is nonzero: a
 * scannable list needs "44m" at a glance, not "44m 42s" fighting for a
 * 36px-wide column.
 */
function formatDurationCell(seconds?: number): string {
  if (seconds === undefined || seconds === null || Number.isNaN(seconds)) return '—';
  const total = Math.floor(seconds);
  if (total < 60) return `${total}s`;
  const totalMinutes = Math.floor(total / 60);
  if (totalMinutes < 60) return `${totalMinutes}m`;
  const hours = Math.floor(totalMinutes / 60);
  const mins = totalMinutes % 60;
  return mins > 0 ? `${hours}h${mins}m` : `${hours}h`;
}

function formatTokensCell(tokens: number): string {
  if (!tokens || tokens <= 0) return '—';
  return formatTokens(tokens);
}

/**
 * Sorts a copy of `items` by `key`. The three optional fields (score,
 * duration) always sink missing values to the end regardless of sort
 * direction — treating "no data" as zero would silently rank unscored
 * sessions as the worst score or shortest run instead of showing they were
 * never measured.
 */
function sortSessions(items: SessionSummary[], key: SortKey): SessionSummary[] {
  const byNullableDesc = (getValue: (s: SessionSummary) => number | undefined) =>
    [...items].sort((a, b) => {
      const av = getValue(a);
      const bv = getValue(b);
      const aMissing = av === undefined || av === null || Number.isNaN(av);
      const bMissing = bv === undefined || bv === null || Number.isNaN(bv);
      if (aMissing && bMissing) return 0;
      if (aMissing) return 1;
      if (bMissing) return -1;
      return bv! - av!;
    });

  switch (key) {
    case 'started_desc':
      return [...items].sort(
        (a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime()
      );
    case 'started_asc':
      return [...items].sort(
        (a, b) => new Date(a.started_at).getTime() - new Date(b.started_at).getTime()
      );
    case 'score_desc':
      return byNullableDesc((s) => s.composite_score);
    case 'duration_desc':
      return byNullableDesc((s) => s.duration_seconds);
    case 'tokens_desc':
      return [...items].sort((a, b) => b.total_tokens - a.total_tokens);
    case 'tokens_asc':
      return [...items].sort((a, b) => a.total_tokens - b.total_tokens);
    case 'events_desc':
      return [...items].sort((a, b) => b.total_events - a.total_events);
    default:
      return items;
  }
}

function CompactIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
      <line x1="4" y1="5" x2="16" y2="5" />
      <line x1="4" y1="10" x2="16" y2="10" />
      <line x1="4" y1="15" x2="16" y2="15" />
    </svg>
  );
}

function ComfortableIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
      <line x1="4" y1="6" x2="16" y2="6" />
      <line x1="4" y1="9.5" x2="11" y2="9.5" />
      <line x1="4" y1="14" x2="16" y2="14" />
      <line x1="4" y1="17.5" x2="11" y2="17.5" />
    </svg>
  );
}

export function SessionList({ selectedId, onSelect, registerNav, liveTail, reloadSignal }: SessionListProps) {
  const { sessions, loading, error, refetch } = useSessions(reloadSignal);
  const [filterText, setFilterText] = useState('');
  const [chip, setChip] = useState<ChipKey>('all');
  const [sortKey, setSortKey] = useState<SortKey>('started_desc');
  const [groupMode, setGroupMode] = useState<GroupMode>('none');
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [density, setDensity] = useState<Density>('compact');
  const scrollRef = useRef<HTMLDivElement>(null);

  const listWidth = useResizableWidth({
    storageKey: LIST_WIDTH_KEY,
    initial: LIST_WIDTH_DEFAULT,
    min: LIST_WIDTH_MIN,
    max: LIST_WIDTH_MAX,
    label: 'Resize session list',
  });

  const filtered = useMemo(() => {
    const text = filterText.trim().toLowerCase();
    return sessions.filter((s) => {
      const level = determineReachedLevel(s.primary_outcome);
      if (chip === 'ci' && level !== 5) return false;
      if (chip === 'failed' && level > 2) return false;
      if (chip === 'claude_code' && s.adapter !== 'claude_code') return false;
      if (text) {
        const haystack = `${s.session_id} ${s.adapter} ${s.models_used.join(' ')} ${s.prompt_preview ?? ''}`.toLowerCase();
        if (!haystack.includes(text)) return false;
      }
      return true;
    });
  }, [sessions, filterText, chip]);

  const sorted = useMemo(() => sortSessions(filtered, sortKey), [filtered, sortKey]);

  const groups = useMemo(() => groupSessions(sorted, groupMode), [sorted, groupMode]);
  const rows = useMemo(() => flattenGroups(groups, collapsed), [groups, collapsed]);

  const rowHeight = density === 'comfortable' ? COMFORTABLE_ROW_HEIGHT : COMPACT_ROW_HEIGHT;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    // Headers and session rows differ in height, so size is per-index rather
    // than one constant — a single estimate would misplace every row below the
    // first header.
    estimateSize: (index) => (rows[index]?.kind === 'header' ? HEADER_ROW_HEIGHT : rowHeight),
    overscan: 12,
  });

  useEffect(() => {
    // Row sizing is computed, not measured from the DOM, so anything that
    // changes a row's height or the mix of row kinds needs an explicit
    // remeasure — the virtualizer otherwise keeps already-laid-out rows at
    // their previous offsets.
    virtualizer.measure();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [density, groupMode, rows.length]);

  const toggleGroup = useCallback((key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }, []);

  // Selection moves between sessions only. Group headers occupy list indices
  // but are not selectable, so j/k steps over them rather than stalling.
  const sessionRowIndices = useMemo(
    () => rows.reduce<number[]>((acc, row, i) => (row.kind === 'session' ? (acc.push(i), acc) : acc), []),
    [rows]
  );

  useEffect(() => {
    const step = (direction: 1 | -1) => {
      if (!sessionRowIndices.length) return;
      const current = sessionRowIndices.findIndex(
        (i) => (rows[i] as Extract<ListRow, { kind: 'session' }>).session.session_id === selectedId
      );
      const nextPos =
        current === -1
          ? 0
          : Math.min(sessionRowIndices.length - 1, Math.max(0, current + direction));
      const row = rows[sessionRowIndices[nextPos]] as Extract<ListRow, { kind: 'session' }>;
      onSelect(row.session.session_id);
    };
    registerNav({ next: () => step(1), prev: () => step(-1) });
  }, [rows, sessionRowIndices, selectedId, onSelect, registerNav]);

  useEffect(() => {
    if (!selectedId) return;
    const idx = rows.findIndex((r) => r.kind === 'session' && r.session.session_id === selectedId);
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

  const isFiltered = filterText.trim() !== '' || chip !== 'all';

  return (
    <section
      className="shell-list-pane"
      style={{ width: listWidth.width, flexBasis: listWidth.width }}
    >
      <div className="shell-list-controls">
        <input
          type="text"
          className="shell-filter-input"
          data-shell-filter
          placeholder="Search prompt, model, session ID, or adapter…"
          autoComplete="off"
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
        />

        <div className="list-toolbar">
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

          <div className="list-toolbar-right">
            <div className="list-sort">
              <label htmlFor="list-group-select" className="list-sort-label">
                Group
              </label>
              <select
                id="list-group-select"
                className="list-sort-select"
                value={groupMode}
                onChange={(e) => setGroupMode(e.target.value as GroupMode)}
              >
                {GROUP_OPTIONS.map((opt) => (
                  <option key={opt.key} value={opt.key}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="list-sort">
              <label htmlFor="list-sort-select" className="list-sort-label">
                Sort
              </label>
              <select
                id="list-sort-select"
                className="list-sort-select"
                value={sortKey}
                onChange={(e) => setSortKey(e.target.value as SortKey)}
              >
                {SORT_OPTIONS.map((opt) => (
                  <option key={opt.key} value={opt.key}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="list-density" role="group" aria-label="Row density">
              <button
                type="button"
                className="list-density-btn"
                aria-pressed={density === 'compact'}
                onClick={() => setDensity('compact')}
                title="Compact rows"
              >
                <CompactIcon />
              </button>
              <button
                type="button"
                className="list-density-btn"
                aria-pressed={density === 'comfortable'}
                onClick={() => setDensity('comfortable')}
                title="Comfortable rows"
              >
                <ComfortableIcon />
              </button>
            </div>
          </div>
        </div>

        {isFiltered && (
          <div className="list-result-count">
            {sorted.length.toLocaleString()} of {sessions.length.toLocaleString()}
          </div>
        )}
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

        {!loading && !error && sessions.length > 0 && sorted.length === 0 && (
          <div className="shell-list-empty">
            <p>No sessions match this filter.</p>
          </div>
        )}

        {!loading && !error && sorted.length > 0 && (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualizer.getVirtualItems().map((vRow) => {
              const row = rows[vRow.index];
              if (!row) return null;
              const position = {
                position: 'absolute' as const,
                top: 0,
                left: 0,
                width: '100%',
                height: vRow.size,
                transform: `translateY(${vRow.start}px)`,
              };

              if (row.kind === 'header') {
                const { group, collapsed: isCollapsed } = row;
                return (
                  <button
                    key={row.key}
                    type="button"
                    className="shell-group-header"
                    style={position}
                    aria-expanded={!isCollapsed}
                    onClick={() => toggleGroup(group.key)}
                  >
                    <span className={`shell-group-caret${isCollapsed ? ' is-collapsed' : ''}`} aria-hidden="true" />
                    <span className="shell-group-label" title={group.key}>
                      {group.label}
                    </span>
                    {group.detail && <span className="shell-group-detail">{group.detail}</span>}
                    <span className="shell-group-count">{group.sessions.length.toLocaleString()}</span>
                  </button>
                );
              }

              const s: SessionSummary = row.session;
              const level = determineReachedLevel(s.primary_outcome);
              const selected = s.session_id === selectedId;
              return (
                <div
                  key={row.key}
                  data-row-id={s.session_id}
                  className={`shell-row shell-row--${density}${selected ? ' shell-row-selected' : ''}${row.indented ? ' shell-row--grouped' : ''}`}
                  style={position}
                  onClick={() => onSelect(s.session_id)}
                >
                  <span className={`shell-row-dot ${dotClass(level)}`} />
                  <div className="shell-row-body">
                    <div className="shell-row-line">
                      <span className="shell-row-id" title={s.session_id}>
                        {s.session_id}
                      </span>
                      <span className="shell-row-agent" title={s.adapter}>
                        {s.adapter}
                      </span>
                      <span className="shell-row-duration">{formatDurationCell(s.duration_seconds)}</span>
                      <span className="shell-row-tokens">{formatTokensCell(s.total_tokens)}</span>
                      <span className="shell-row-score">{formatScore(s.composite_score)}</span>
                    </div>
                    {density === 'comfortable' && (
                      <div className="shell-row-preview" title={s.prompt_preview}>
                        {s.prompt_preview || '—'}
                      </div>
                    )}
                  </div>
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

      <div
        className={`shell-list-resizer${listWidth.dragging ? ' is-dragging' : ''}`}
        title="Drag to resize · double-click to reset"
        {...listWidth.handleProps}
      />
    </section>
  );
}
