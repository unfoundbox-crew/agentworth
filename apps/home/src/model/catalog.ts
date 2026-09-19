/**
 * Static model catalog for the deck's harness pill row.
 *
 * Survey (in-lane, 2026-09-19):
 * - Gateway `hello` carries only harness id/label/bin (`detect_harnesses` in
 *   `apps/cli/src/server/home/protocol.rs`) — no models, no variants, no keys.
 * - No `models.yaml`, no spacepilot registry, no harness-reported model list
 *   exists anywhere in this repo (searched `models.yaml`, `model.*registry`,
 *   `variant`, `effort` outside adapters/ATIF).
 * - Cheapest local source is therefore a static fallback compiled into the UI,
 *   with a live-override seam (`resolveCatalog`) for a harness-reported list
 *   when one answers (`window.__agentworth_models`). Static wins when nothing
 *   answers, which is today.
 *
 * Reference only (never copied): opencode's `dialog-select-model.tsx`
 * (group by provider, free/cost tags, per-model visibility) and `prompt-input.tsx`
 * (variant row shown only when `variant.list().length > 0`).
 */

export interface ModelVariant {
  id: string;
  label: string;
  /** Harness flag the variant maps to (e.g. `--effort`), used for `start_rider` args. */
  flag: string;
}

export interface CatalogModel {
  id: string;
  name: string;
  provider: string;
  /** Harness id from `HomeEnv.harnesses` (`claude`, `codex`, `agy`, `opencode`, `cursor`, `gemini`). */
  harness: string;
  /** Per-model visibility: invisible models never render and never match search. */
  visible: boolean;
  /** `true` = known-free, `false` = known-paid, `undefined` = unknown. */
  free?: boolean;
  variants?: ModelVariant[];
}

export const STATIC_CATALOG: CatalogModel[] = [
  { id: 'sonnet', name: 'Sonnet', provider: 'anthropic', harness: 'claude', visible: true, free: false },
  { id: 'haiku', name: 'Haiku', provider: 'anthropic', harness: 'claude', visible: true, free: false },
  { id: 'opus', name: 'Opus', provider: 'anthropic', harness: 'claude', visible: true, free: false },
  {
    id: 'gpt-5',
    name: 'GPT-5',
    provider: 'openai',
    harness: 'codex',
    visible: true,
    free: false,
    variants: [
      { id: 'low', label: 'Low effort', flag: '--effort' },
      { id: 'medium', label: 'Medium effort', flag: '--effort' },
      { id: 'high', label: 'High effort', flag: '--effort' },
    ],
  },
  { id: 'gpt-5-mini', name: 'GPT-5 mini', provider: 'openai', harness: 'codex', visible: true, free: false },
  { id: 'gemini-flash', name: 'Gemini Flash', provider: 'google', harness: 'agy', visible: true, free: true },
  { id: 'gemini-pro', name: 'Gemini Pro', provider: 'google', harness: 'agy', visible: true, free: false },
  { id: 'gemini-flash', name: 'Gemini Flash', provider: 'google', harness: 'gemini', visible: true, free: true },
  { id: 'zen-free', name: 'Zen Free', provider: 'opencode', harness: 'opencode', visible: true, free: true },
  { id: 'zen-paid', name: 'Zen Paid', provider: 'opencode', harness: 'opencode', visible: true, free: false },
  { id: 'composer', name: 'Composer', provider: 'cursor', harness: 'cursor', visible: true, free: false },
];

/** Live harness-reported list wins when present; static fallback otherwise. Never throws. */
export function resolveCatalog(): CatalogModel[] {
  try {
    const live =
      typeof window !== 'undefined'
        ? (window as unknown as { __agentworth_models?: CatalogModel[] }).__agentworth_models
        : undefined;
    if (Array.isArray(live) && live.length > 0) return live;
  } catch {
    // Fall through to static.
  }
  return STATIC_CATALOG;
}

/** Only models the deck may show. */
export function visibleModels(models: CatalogModel[]): CatalogModel[] {
  return models.filter((m) => m.visible);
}

/** Case-insensitive substring match over name, id, and provider. */
export function filterModels(models: CatalogModel[], query: string): CatalogModel[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...models];
  return models.filter(
    (m) => m.name.toLowerCase().includes(q) || m.id.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q),
  );
}

export interface ModelGroup {
  provider: string;
  models: CatalogModel[];
}

/** Group by provider, providers and models both sorted by name for a stable list. */
export function groupModels(models: CatalogModel[]): ModelGroup[] {
  const byProvider = new Map<string, CatalogModel[]>();
  for (const m of models) {
    byProvider.set(m.provider, [...(byProvider.get(m.provider) ?? []), m]);
  }
  return [...byProvider.entries()]
    .map(([provider, items]) => ({
      provider,
      models: [...items].sort((a, b) => a.name.localeCompare(b.name)),
    }))
    .sort((a, b) => a.provider.localeCompare(b.provider));
}

/** The second variant row renders only when the model actually has variants. */
export function shouldShowVariantRow(model: CatalogModel): boolean {
  return (model.variants ?? []).length > 0;
}

/** Tag text for the free/cost chip; `null` when cost is unknown. */
export function costTag(model: CatalogModel): 'free' | 'paid' | null {
  if (model.free === true) return 'free';
  if (model.free === false) return 'paid';
  return null;
}
