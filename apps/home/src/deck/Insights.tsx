import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { PanelState } from './PanelState';
import {
  DEFAULT_FILTER,
  DEFAULT_MODEL_SORT,
  LADDER,
  modelSortToggle,
  pctDelta,
  sortModels,
  topMovers,
  verifiedRate,
  WEBS,
  fetchDrill,
  fetchInsights,
  type DrillResult,
  type Insights,
  type InsightsFilter,
  type InsightsState,
  type InsightsWindow,
  type ModelSortKey,
} from '../model/insights';
import {
  filteredDimension,
} from '../model/insightsBackend';

/**
 * Insights — the deck's observability view. Datadog grammar, deck brand
 * language: a global filter bar, a KPI strip whose hero tile is the
 * verified-outcome rate (the product thesis in one number), then the
 * evidence-ladder funnel and the hour x weekday heatmap. Secondary widgets
 * (models table, friction bars, Explore drawer) stay collapsed until asked
 * for; progressive disclosure keeps default cognitive load low.
 *
 * Every number carries a one-line so-what. Thin coverage says so honestly:
 * no invented data, no zero-filled grids, colour only where earned.
 *
 * The wire shape is the Rust backend (#191, GET /api/insights); every field
 * name is adapted in ../model/insightsBackend.ts — the one mapping layer.
 */

const DOW = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];

function fmtB(b: number): string {
  if (b >= 100) return `${b.toFixed(0)}B`;
  if (b >= 10) return `${b.toFixed(1)}B`;
  return `${b.toFixed(2)}B`;
}

function deltaLine(cur: number, prevValue: number | null | undefined): string {
  const d = pctDelta(cur, prevValue);
  if (d == null) return 'vs previous window · no base to compare';
  const arrow = d > 0.05 ? '▲' : d < -0.05 ? '▼' : '·';
  return `${arrow} ${d > 0 ? '+' : ''}${d.toFixed(1)}% vs previous window`;
}

function delta(cur: number, prevValue: number | null | undefined): string {
  const d = pctDelta(cur, prevValue);
  if (d == null) return '·';
  return `${d > 0 ? '▲' : d < -0.05 ? '▼' : '·'} ${d > 0 ? '+' : ''}${d.toFixed(1)}%`;
}

/* ---------- skeletons: designed, never blank, matching final layout ---------- */

function Skeleton({ className }: { className: string }) {
  return <div className={`rounded-sm bg-line animate-pulse ${className}`} />;
}

function KpiStripSkeleton() {
  return (
    <div className="grid grid-cols-2 md:grid-cols-6 gap-2" aria-busy>
      {[...Array(6)].map((_, i) => (
        <div key={i} className="border border-line rounded-md p-3 flex flex-col gap-2">
          <Skeleton className="h-3 w-20" />
          <Skeleton className={i === 0 ? 'h-8 w-16' : 'h-6 w-14'} />
          <Skeleton className="h-2 w-16" />
        </div>
      ))}
    </div>
  );
}

function PairSkeleton() {
  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4" aria-busy>
      {[...Array(2)].map((_, i) => (
        <div key={i} className="border border-line rounded-md p-4 flex flex-col gap-3">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-24 w-full" />
        </div>
      ))}
    </div>
  );
}

/* ---------- shared micro-label --- */
function Label({ children }: { children: ReactNode }) {
  return <span className="text-[10px] uppercase tracking-wider text-dim">{children}</span>;
}

/* ---------- KPI strip ---------- */

function Sparkline({ series, width = 88 }: { series: number[]; width?: number }) {
  if (series.length < 2) return null;
  const max = Math.max(...series);
  const min = Math.min(...series);
  const h = 14;
  const points = series
    .map((v, i) => {
      const x = (i / (series.length - 1)) * (width - 2);
      const y = max === min ? h / 2 : h - 2 - ((v - min) / (max - min)) * (h - 4);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');
  return (
    <svg width={width} height={h} viewBox={`0 0 ${width} ${h}`} aria-hidden role="presentation">
      <polyline points={points} fill="none" stroke="var(--mv-accent)" strokeWidth={1} opacity={0.8} />
    </svg>
  );
}

function KpiTile({ kpi, fromPrev }: {
  kpi: InsightsWindow['kpis'][number];
  fromPrev: number | null | undefined;
}) {
  const hero = kpi.key === 'verified';
  return (
    <div
      className={`border rounded-md p-3 flex flex-col gap-1 min-h-[92px] ${hero ? 'border-ink' : 'border-line'}`}
      title={`${kpi.label}: ${kpi.value.toLocaleString()}${kpi.unit ?? ''} — ${deltaLine(kpi.value, fromPrev)}`}
    >
      <Label>{kpi.label}</Label>
      {hero && kpi.key === 'verified' && (
        <div className="text-[9px] uppercase tracking-wider text-dim">hero · product thesis</div>
      )}
      <div
        className={`font-mono tabular-nums text-ink ${hero ? 'text-2xl' : 'text-lg'}`}
        aria-label={`${kpi.label}: ${kpi.value.toLocaleString()}${kpi.unit ?? ''}`}
      >
        {kpi.value.toLocaleString()}
        {kpi.unit && <span className="text-[10px] text-dim">{kpi.unit}</span>}
      </div>
      <div className="text-[10px] text-dim tabular-nums">{delta(kpi.value, fromPrev)}</div>
      {kpi.series && kpi.series.length > 1 && <Sparkline series={kpi.series} />}
    </div>
  );
}

function KpiStrip({ win, prevWin }: { win: InsightsWindow; prevWin: InsightsWindow | null }) {
  const kpis = win.kpis;
  if (kpis.length === 0) {
    return <PanelState kind="empty" title="no numbers in this window" hint="the insight query found nothing to headline" />;
  }
  return (
    <div className="grid grid-cols-2 md:grid-cols-6 gap-2">
      {kpis.map((k) => {
        const prevValue = prevWin?.kpis.find((p) => p.key === k.key)?.value ?? undefined;
        return <KpiTile key={k.key} kpi={k} fromPrev={prevValue} />;
      })}
    </div>
  );
}

/* ---------- ladder funnel (horizontal SVG funnel, not a pie) ---------- */

export function LadderFunnel({ win, onDrill }: { win: InsightsWindow; onDrill: (key: string) => void }) {
  const counts = new Map(win.ladder.map((r) => [r.outcome, r.sessions]));
  const top = Math.max(1, ...win.ladder.map((r) => r.sessions));
  const width = 100;
  const rowH = 16;
  const h = LADDER.length * (rowH - 1) + 2;

  return (
    <div className="border border-line rounded-md p-4" style={{ minWidth: 220 }}>
      <Label>evidence ladder · sessions by rung</Label>
      <svg width={'100%'} height={h} viewBox={`0 0 ${width} ${h}`} preserveAspectRatio="none" className="mt-2 max-h-24">
        {LADDER.map(({ outcome }, i) => {
          const n = counts.get(outcome) ?? 0;
          const w = top === 0 ? 0 : (n / top) * width;
          const y = i * (rowH - 1);
          // A hairline presence keeps 0 from vanishing silently — visible, still honest
          // because the paired count below says the real number.
          const flat = n === 0 ? 0 : Math.max(w, 1);
          return (
            <rect
              key={outcome}
              x={0}
              y={y}
              width={flat}
              height={rowH - 4}
              fill={outcome === 'no_outcome_evidence' ? 'var(--mv-border)' : 'var(--mv-accent)'}
              opacity={outcome === 'no_outcome_evidence' ? 0.7 : 0.9}
              rx={1.5}
              tabIndex={0}
              role="button"
              aria-label={`drill into ${LADDER[i].label}: ${n} sessions`}
              className="outline-none cursor-pointer"
            >
              <title>{`${LADDER[i].label}: ${n.toLocaleString()} sessions · click to drill through`}</title>
            </rect>
          );
        })}
      </svg>
      <div className="mt-2 grid grid-cols-2 gap-x-3">
        {LADDER.map(({ outcome, label }) => {
          const n = counts.get(outcome) ?? 0;
          return (
            <button
              key={outcome}
              type="button"
              className="flex items-baseline justify-between gap-1 py-px cursor-pointer hover:text-ink text-left"
              onClick={() => n > 0 && onDrill(outcome)}
              title={n > 0 ? `drill into the ${n} session(s) behind ${label}` : 'nothing on this rung'}
              disabled={n === 0}
            >
              <span className={`text-[10px] ${outcome === 'no_outcome_evidence' ? 'text-dim' : 'text-muted'}`}>{label}</span>
              <span className="font-mono text-[10px] tabular-nums text-dim">
                {n.toLocaleString()}
              </span>
            </button>
          );
        })}
      </div>
      <div className="mt-2 text-[10px] text-dim">
        rungs are evidence, not success — the bottom rung is not flunking · rows drill through {verifiedRate(win).toFixed(1)}% sessions cleared test-or-better
      </div>
    </div>
  );
}

/* ---------- heatmap (hour x dow) ---------- */

function Heatmap({ win, onDrill, filteredRepos }: { win: InsightsWindow; onDrill: (key: string) => void; filteredRepos: number | null }) {
  const cell = (dow: number, hour: number) =>
    win.day_hour.find((c) => c.dow === dow && c.hour === hour)?.sessions ?? 0;
  const peak = Math.max(1, ...win.day_hour.map((c) => c.sessions));
  const colour = (n: number) =>
    n === 0 ? 'var(--mv-border-soft)' : `color-mix(in srgb, var(--mv-accent) ${15 + Math.min((n / peak) * 85, 100)}%, transparent)`;

  return (
    <div className="border border-line rounded-md p-4">
      <Label>when this machine works · sessions</Label>
      <div className="mt-2 text-[9px] text-dim">
        hours 00-23 (utc){filteredRepos != null && ' · heatmap is turns on filtered sessions'}
      </div>
      <div className="mt-1 flex flex-col gap-[2px]">
        {DOW.map((day, dow) => (
          <div key={day} className="flex items-center gap-[2px]">
            <span className="w-8 text-[9px] text-dim uppercase tracking-wide">{day}</span>
            {Array.from({ length: 24 }, (_, h) => {
              const n = cell(dow, h);
              return (
                <button
                  type="button"
                  key={h}
                  className="h-2.5 flex-1 rounded-[2px] border-0 p-0 cursor-pointer"
                  style={{ background: colour(n) }}
                  title={`${day} ${String(h).padStart(2, '0')}:00 · ${n.toLocaleString()} sessions · click to drill through`}
                  onClick={() => n > 0 && onDrill(`${dow}-${h}`)}
                >
                  <span className="sr-only">{`${day} ${h}:00 ${n} sessions, drill through`}</span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

/* ---------- models table ---------- */

function ModelTable({ win, filter, onSetModel }: {
  win: InsightsWindow;
  filter: InsightsFilter;
  onSetModel: (m: string) => void;
}) {
  const [sort, setSort] = useState(DEFAULT_MODEL_SORT);
  const rows = sortModels(win.models, sort);
  function head(label: string, key: ModelSortKey) {
    const active = sort.key === key;
    return (
      <button
        type="button"
        onClick={() => setSort((s) => modelSortToggle(s, key))}
        aria-sort={active ? (sort.dir === -1 ? 'descending' : 'ascending') : 'none'}
        className="text-[10px] text-dim hover:text-ink text-right"
      >
        {label}
        {active ? (sort.dir === -1 ? ' ↓' : ' ↑') : ''}
      </button>
    );
  }
  return (
    <div className="border border-line rounded-md p-4">
      <Label>models · sortable</Label>
      <div className="mt-2">
        <div className="grid grid-cols-[1fr_56px_56px_56px] gap-1 border-b border-line pb-1">
          <Label>model</Label>
          <span className="text-right">{head('sess', 'sessions')}</span>
          <span className="text-right">{head('tok', 'tb')}</span>
          <span className="text-right">{head('cr%', 'cr_perc')}</span>
        </div>
        {rows.length === 0 && <div className="text-[10px] text-dim">no models touched this window</div>}
        {rows.map((m) => (
          <button
            key={m.model}
            type="button"
            className="grid grid-cols-[1fr_56px_56px_56px] gap-1 py-0.5 text-left w-full hover:bg-ground cursor-pointer"
            onClick={() => onSetModel(m.model)}
            title={`click to filter every widget by ${m.model} · full drill into a session list is a follow-up`}
            aria-pressed={filter.model === m.model}
          >
            <span className="truncate text-[11px] text-text" title={m.model}>{m.model}</span>
            <span className="text-[11px] text-muted text-right tabular-nums">{m.sessions.toLocaleString()}</span>
            <span className="text-[11px] text-muted text-right tabular-nums">{fmtB(m.tb)}</span>
            <span className="text-[11px] text-muted text-right tabular-nums">{m.cr_perc.toFixed(0)}%</span>
          </button>
        ))}
      </div>
    </div>
  );
}

/* ---------- friction bars (direct value labels) ---------- */

function FrictionBars({ win, prevWin, onDrill }: { win: InsightsWindow; prevWin: InsightsWindow | null; onDrill: (key: string) => void }) {
  const rows = [...win.friction].sort((a, b) => b.sessions - a.sessions);
  const prevMap = new Map(prevWin?.friction.map((r) => [r.trigger, r.sessions]) ?? []);
  const peak = Math.max(1, ...rows.map((r) => r.sessions));
  return (
    <div className="border border-line rounded-md p-4">
      <Label>friction by trigger · humans stepped in</Label>
      {rows.length === 0 && (
        <div className="text-[10px] text-dim mt-2">no friction detected in this window — either quiet, or not yet measured</div>
      )}
      <div className="mt-2 flex flex-col gap-1.5">
        {rows.map((r) => (
          <button
            key={r.trigger}
            type="button"
            className="flex items-baseline gap-2 text-left cursor-pointer hover:bg-ground"
            onClick={() => onDrill(r.trigger)}
            title={`${r.trigger}: ${r.sessions.toLocaleString()} turns — friction is fuel for recovery; click to drill through`}
          >
            <span className="text-[10px] text-muted w-[110px] truncate shrink-0">{r.trigger.replace(/_/g, ' ')}</span>
            <span
              className="h-3 rounded-sm bg-accent grow-none"
              style={{ width: `${Math.max((r.sessions / peak) * 100, 1.5)}%`, minWidth: 2 }}
            />
            <span className="text-[10px] text-dim tabular-nums whitespace-nowrap">
              {r.sessions.toLocaleString()}
              {prevMap.has(r.trigger) && (
                <span className="ml-1" title="vs previous window">({delta(r.sessions, prevMap.get(r.trigger))})</span>
              )}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

/* ---------- Explore drawer (progressive disclosure) ---------- */
export function ExploreDrawer({ win, open, onToggle }: { win: InsightsWindow; open: boolean; onToggle: () => void }) {
  if (open) {
    return (
      <div className="border border-line rounded-md">
        <div className="px-4 py-2 border-b border-line flex justify-between items-baseline">
          <Label>explore · the deep rows</Label>
          <button type="button" className="text-[10px] text-dim hover:text-ink" onClick={onToggle}>hide ✕</button>
        </div>
        <div className="p-4 grid grid-cols-1 md:grid-cols-3 gap-4">
          <section>
            <Label>top repos</Label>
            <div className="mt-1 flex flex-col gap-1">
              {win.repos.length === 0 && <span className="text-[10px] text-dim">nothing in this window</span>}
              {win.repos.map((r) => (
                <span key={r.repo} className="flex justify-between text-[10px]">
                  <span className="text-text truncate" title={r.repo}>{r.repo}</span>
                  <span className="tabular-nums text-dim">{r.sessions.toLocaleString()} sess</span>
                </span>
              ))}
            </div>
          </section>
          <section>
            <Label>biggest sessions</Label>
            <div className="mt-1 flex flex-col gap-1">
              {win.big_sessions.length === 0 && <span className="text-[10px] text-dim">nothing big in this window</span>}
              {win.big_sessions.map((s, i) => (
                <span key={`${s.adapter}-${s.date}-${i}`} className="flex justify-between text-[10px]">
                  <span className="text-muted">{s.date} · {s.adapter.slice(0,6)}</span>
                  <span className="tabular-nums text-dim" title={`${s.tokens.toLocaleString()} tokens over ${s.events.toLocaleString()} events`}>{fmtB(s.tokens / 1e9)}</span>
                </span>
              ))}
            </div>
          </section>
          <section>
            <Label>repeated vocabulary</Label>
            <div className="mt-1 flex flex-wrap gap-1">
              {win.vocabulary.length === 0 && <span className="text-[10px] text-dim">nothing scanned yet</span>}
              {win.vocabulary.map((v) => (
                <span key={v.term} className="text-[10px] text-muted border border-line rounded-full px-2 py-[1px]" title={`${v.term} appears on ${v.sessions.toLocaleString()} sessions`}>
                  {v.term} <span className="text-dim tabular-nums">{v.sessions}</span>
                </span>
              ))}
            </div>
          </section>
        </div>
      </div>
    );
  }
  return (
    <button type="button" className="border border-line rounded-md px-4 py-2 text-left text-[10px] text-dim hover:text-ink cursor-pointer" onClick={onToggle}>
      explore · deep rows, repeated vocabulary, biggest sessions ▸
    </button>
  );
}

/* ---------- drill drawer (the sessions behind one cut, the deck's feed surface) ---------- */

export interface DrillView {
  view: 'ladder' | 'day_hour' | 'friction';
  key: string;
}

/**
 * DrillDrawer — the deck's own session rows (same columns the explore drawer's
 * biggest-sessions list already renders, plus outcome + id), fed by
 * /api/insights/drill. Escape closes (Deck.tsx owns Escape globally; clicking
 * ✕ is the mouse path). Rows are stats-shaped only by backend contract.
 */
export function DrillDrawer({ drill, result, error, loading, onClose }: {
  drill: DrillView | null;
  result: (DrillResult & { of: string }) | null;
  error: string | null;
  loading: boolean;
  onClose: () => void;
}) {
  if (!drill) return null;
  const title: Record<DrillView['view'], string> = {
    ladder: 'evidence ladder',
    day_hour: 'heatmap cell',
    friction: 'friction trigger',
  };
  if (loading) {
    return <PanelState kind="empty" title="drilling…" hint={`${drill.view} · ${drill.key}`} />;
  }
  if (error) {
    return <PanelState kind="error" title="drill failed" hint={error} />;
  }
  if (!result || result.of !== `${drill.view}|${drill.key}`) return null;
  return (
    <div className="border border-line rounded-md" aria-label="drill through">
      <div className="px-4 py-2 border-b border-line flex justify-between items-baseline">
        <Label>{`drill · ${title[drill.view]} · ${result.count.toLocaleString()} session(s)`}</Label>
        <button type="button" className="text-[10px] text-dim hover:text-ink" onClick={onClose}>hide ✕</button>
      </div>
      {result.count === 0 ? (
        <div className="p-4 text-[10px] text-dim">no sessions on this cut in this window</div>
      ) : (
        <ul className="p-4 flex flex-col gap-1 max-h-56 overflow-y-auto">
          {result.rows.map((r) => (
            <li key={r.session_id} className="flex justify-between gap-2 text-[10px] py-0.5" title={r.session_id}>
              <span className="text-muted truncate">{r.repo_label} · {r.adapter.slice(0, 6)}</span>
              <span className="text-dim">{r.started_at.slice(0, 10)}</span>
              <span className="text-muted tabular-nums">{(r.total_tokens / 1e9).toFixed(2)}B tok</span>
              <span className="w-10 text-right text-dim truncate tabular-nums">{r.primary_outcome ?? 'unflown'}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/* ---------- auto-insight callout ---------- */

function CalloutStrip({ win, prevWin }: { win: InsightsWindow; prevWin: InsightsWindow | null }) {
  return (
    <div className="flex items-center gap-2 border border-line rounded-md px-3 py-1.5 bg-panel">
      <span className="text-[10px] uppercase tracking-wider text-dim shrink-0">auto-insight</span>
      <span className="text-[11px] text-text truncate" title={`${win.claimed.toLocaleString()} sessions in this window; movers are computed client-side from current vs previous`}>
        {topMovers(win, prevWin)}
      </span>
    </div>
  );
}

/* ---------- filter bar ---------- */

function FilterBar({ filter, onFilter, options }: {
  filter: InsightsFilter;
  onFilter: (f: InsightsFilter) => void;
  options: { adapters: string[]; models: string[]; repos: string[] };
}) {
  const cls = 'bg-panel border border-line text-[11px] text-muted px-2 py-1 rounded-sm';
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="inline-flex border border-line rounded-sm overflow-hidden">
        {WEBS.map((w) => (
          <button
            key={w}
            type="button"
            aria-pressed={filter.web === w}
            className={`px-2 py-1 text-[11px] cursor-pointer ${filter.web === w ? 'bg-surface-3' : ''}`}
            style={{ background: filter.web === w ? 'var(--mv-surface-3)' : undefined }}
            onClick={() => onFilter({ ...filter, web: w })}
          >
            {w}
          </button>
        ))}
      </div>
      {([['adapter', options.adapters], ['model', options.models], ['repo', options.repos]] as [string, string[]][])
        .map(([label, options_]) => (
        <select
          key={label}
          className={cls}
          aria-label={`filter by ${label}`}
          value={String((filter as unknown as Record<string, string | null>)[label] ?? '')}
          onChange={(e) => onFilter({ ...filter, [label as 'adapter' | 'model' | 'repo']: e.target.value || null })}
        >
          <option value="">{label} · all</option>
          {options_.map((v) => (
            <option key={v} value={v}>{v}</option>
          ))}
        </select>
      ))}
      {(filter.adapter || filter.model || filter.repo) && (
        <button type="button" className="text-[10px] text-dim hover:text-ink" onClick={() => onFilter({ ...filter, adapter: null, model: null, repo: null })}>
          clear ✕
        </button>
      )}
    </div>
  );
}

/* ---------- hooks + shell ---------- */

function hasFilter(f: Pick<InsightsFilter, 'adapter' | 'model' | 'repo'>): boolean {
  return !!(f.adapter || f.model || f.repo);
}

function useInsights(filter: InsightsFilter, open: boolean): InsightsState {
  const [state, setState] = useState<InsightsState>({ status: 'ready', data: null });
  const key = useMemo(() => `${filter.web}|${filter.adapter}|${filter.model}|${filter.repo}`, [filter]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setState((s) => ({ ...s, status: 'loading' }));
    fetchInsights(filter)
      .then((data) => { if (!cancelled) setState({ status: 'ready', data }); })
      .catch((err: Error) => {
        if (!cancelled) {
          setState({
            status: err.message.startsWith('insights endpoint') ? 'run_in_progress' : 'error',
            data: null,
          });
        }
      });
    return () => { cancelled = true; };
  }, [open, key]);

  return state;
}

export function InsightsBody({ data, filter, onFilter, detailsOpen, onToggleDetails, drawerOpen, onToggleDrawer }: {
  data: Insights;
  filter: InsightsFilter;
  onFilter: (f: InsightsFilter) => void;
  detailsOpen: boolean;
  onToggleDetails: () => void;
  drawerOpen: boolean;
  onToggleDrawer: () => void;
}) {
  const { current: rawCurrent, previous: rawPrevious } = data;
  // Dimension filters are backend query params now (the slice-and-drill lane): the payload
  // ALREADY recomputed every aggregate — KPIs, ladder, heatmap, friction, deltas — from the
  // slice, so no client-side narrowing stands between the wire and the widgets, and Δ now
  // compares slice to slice. A zero-session slice arrives pre-noticed on data.filter.notice.
  // Nothing hidden: the mapped `previous` window rides the real deltas block, whatever the
  // slice. The honest-Δ change above is the whole point of the backend recompute.
  const current = rawCurrent;
  const previous = rawPrevious;
  const filtered = hasFilter(filter);
  const sliceNotice = data.echo?.notice ?? null;
  const adapterOptions = useMemo(() => rawCurrent.by_adapter.map((a) => a.adapter), [rawCurrent]);
  const modelOptions = useMemo(() => rawCurrent.models.map((m) => m.model), [rawCurrent]);
  const repoOptions = useMemo(() => rawCurrent.repos.map((r) => r.repo), [rawCurrent]);

  const [drill, setDrill] = useState<DrillView | null>(null);
  const [drillState, setDrillState] = useState<{
    loading: boolean;
    error: string | null;
    result: (DrillResult & { of: string }) | null;
  }>({ loading: false, error: null, result: null });

  useEffect(() => {
    if (!drill) return;
    let cancelled = false;
    setDrillState({ loading: true, error: null, result: null });
    fetchDrill(filter, drill)
      .then((rows) => {
        if (!cancelled) {
          setDrillState({ loading: false, error: null, result: { ...rows, of: `${drill.view}|${drill.key}` } });
        }
      })
      .catch((err: Error) => {
        if (!cancelled) setDrillState({ loading: false, error: err.message, result: null });
      });
    return () => { cancelled = true; };
  }, [drill]);

  const openDrill = useCallback((view: DrillView['view'], key: string) => {
    setDrill(key ? { view, key } : null);
  }, []);

  return (
    <div className="h-full overflow-y-auto" aria-label="insights">
      <div className="px-5 py-3 flex flex-col gap-3 max-w-full">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <div className="flex items-baseline gap-3">
            <span className="text-[13px] text-ink tracking-tight">insights</span>
            <span
              className="text-[10px] text-dim"
              title={`window ${data.since} → ${data.until}`}
            >
              {filter.web}{data.since && ` · since ${data.since.slice(0, 10)}`}
              {filtered && ` · aggregates recomputed on ${sliceNotice ? 'empty slice' : filteredDimension(filter)}`}
              {sliceNotice && <span title={sliceNotice}> · empty slice ⚑</span>}
            </span>
          </div>
          <FilterBar filter={filter} onFilter={(f) => { setDrill(null); onFilter(f); }} options={{ adapters: adapterOptions, models: modelOptions, repos: repoOptions }} />
        </div>

        <CalloutStrip win={current} prevWin={previous} />
        <KpiStrip win={current} prevWin={previous} />

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <LadderFunnel win={current} onDrill={(k) => openDrill('ladder', k)} />
          <Heatmap win={current} onDrill={(k) => openDrill('day_hour', k)} filteredRepos={hasFilter(filter) ? 1 : null} />
        </div>

        <button type="button" className="text-[10px] text-dim hover:text-ink self-start cursor-pointer" onClick={onToggleDetails}>
          {detailsOpen ? 'fewer details −' : 'more details +'}
        </button>

        {detailsOpen && (
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <ModelTable win={current} filter={filter} onSetModel={(m) => onFilter({ ...filter, model: filter.model === m ? null : m })} />
              <FrictionBars win={current} prevWin={previous} onDrill={(k) => openDrill('friction', k)} />
            </div>
            <ExploreDrawer win={current} open={drawerOpen} onToggle={onToggleDrawer} />
          </div>
        )}

        <DrillDrawer
          drill={drill}
          result={drillState.result}
          error={drillState.error}
          loading={drillState.loading}
          onClose={() => setDrill(null)}
        />
      </div>
    </div>
  );
}

/* ---------- the opened phase ---------- */

/**
 * A place the human visits on purpose, not an alarm surface: opened with `i`
 * from anywhere the deck shows, left with Escape (Deck.tsx owns both). Nothing
 * here interrupts a working rider.
 */
export function Insights({ open }: { open: boolean }) {
  const [filter, setFilter] = useState<InsightsFilter>(DEFAULT_FILTER);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const { status, data } = useInsights(filter, open);

  if (!open) return null;
  if (status === 'run_in_progress') {
    return (
      <div className="h-full flex items-center justify-center">
        <PanelState kind="empty" title="insights are not on this build yet" hint="GET /api/insights lands via the Rust lane · this panel opens once it ships" />
      </div>
    );
  }
  if (status === 'error') {
    return (
      <div className="h-full flex items-center justify-center">
        <PanelState kind="error" title="could not read the index" hint="the insights endpoint did not answer · reopen with i to retry" />
      </div>
    );
  }
  if (status === 'loading' || !data) {
    return (
      <div className="h-full overflow-y-auto">
        <div className="px-5 py-3 flex flex-col gap-3">
          <Skeleton className="h-8 w-full" />
          <KpiStripSkeleton />
          <PairSkeleton />
        </div>
      </div>
    );
  }
  return <InsightsBody data={data} filter={filter} onFilter={setFilter} detailsOpen={detailsOpen} onToggleDetails={() => setDetailsOpen((o) => !o)} drawerOpen={drawerOpen} onToggleDrawer={() => setDrawerOpen((o) => !o)} />;
}
