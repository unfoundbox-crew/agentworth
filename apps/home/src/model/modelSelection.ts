import type { CatalogModel } from './catalog';

/**
 * Per-harness model selection, persisted in the deck's existing store family.
 *
 * Survey: `src/model/store.ts` holds directions/personas/spaces in memory from
 * the gateway `hello` frame — nothing persists there. The only persisted deck
 * state is the theme (`agentworth_theme` localStorage in `packages/ui/useTheme.ts`).
 * So localStorage is the convention, and this module uses the sibling key
 * `agentworth_model_selection`: a JSON map of harness id -> selection. Never throws;
 * a corrupt entry reads as empty.
 */

export const SELECTION_STORAGE_KEY = 'agentworth_model_selection';

export interface ModelSelection {
  harnessId: string;
  modelId: string;
  variantId?: string;
}

type SelectionMap = Record<string, ModelSelection>;

function readMap(storage: Pick<Storage, 'getItem'>): SelectionMap {
  try {
    const raw = storage.getItem(SELECTION_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return {};
    return parsed as SelectionMap;
  } catch {
    return {};
  }
}

export function loadSelection(storage: Pick<Storage, 'getItem'>, harnessId: string): ModelSelection | undefined {
  const sel = readMap(storage)[harnessId];
  if (!sel || typeof sel.modelId !== 'string') return undefined;
  return sel;
}

export function saveSelection(storage: Pick<Storage, 'getItem' | 'setItem'>, sel: ModelSelection): void {
  try {
    const map = readMap(storage);
    map[sel.harnessId] = sel;
    storage.setItem(SELECTION_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // Persistence is best-effort; selection still applies in memory.
  }
}

/** Minimal storage shim for environments without `window.localStorage` (SSR/tests). */
export function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => (map.has(k) ? map.get(k)! : null),
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: (i: number) => [...map.keys()][i] ?? null,
    get length() {
      return map.size;
    },
  } as Storage;
}

function browserStorage(): Pick<Storage, 'getItem' | 'setItem'> | undefined {
  try {
    if (typeof window !== 'undefined' && window.localStorage) return window.localStorage;
  } catch {
    // No storage available (SSR) — callers fall back to memory.
  }
  return undefined;
}

export function loadBrowserSelection(harnessId: string): ModelSelection | undefined {
  const s = browserStorage();
  return s ? loadSelection(s, harnessId) : undefined;
}

export function saveBrowserSelection(sel: ModelSelection): void {
  const s = browserStorage();
  if (s) saveSelection(s, sel);
}

// --- Dialog open state -------------------------------------------------------

export interface ModelDialogState {
  openFor: string | null;
}

export type ModelDialogEvent = { type: 'pill_click'; harnessId: string } | { type: 'close' } | { type: 'select' };

/** Clicking a harness pill opens the list; closing or selecting dismisses it. */
export function dialogReducer(_state: ModelDialogState, event: ModelDialogEvent): ModelDialogState {
  switch (event.type) {
    case 'pill_click':
      return { openFor: event.harnessId };
    case 'close':
    case 'select':
      return { openFor: null };
  }
}

// --- Quota / key states ------------------------------------------------------

export type ModelStatus = 'unpaid' | 'over_quota' | 'missing_key';

export interface StatusDialogCopy {
  title: string;
  body: string;
  action: string;
}

/**
 * Machine states always render as dialogs with human copy — never raw errors,
 * never thrown. The copy carries no codes, no HTTP statuses.
 */
export function quotaDialogContent(status: ModelStatus): StatusDialogCopy {
  switch (status) {
    case 'unpaid':
      return {
        title: 'This model needs a paid plan',
        body: 'Your account has no paid seat for this provider. Free models below still ride, or connect a paid provider to continue.',
        action: 'View free models',
      };
    case 'over_quota':
      return {
        title: 'Over quota for now',
        body: 'This provider says the quota is spent. Pick a free model below, or try again after the quota resets.',
        action: 'View free models',
      };
    case 'missing_key':
      return {
        title: 'Missing provider key',
        body: 'No key is set for this provider on this machine. Add one, or ride a model that needs no key.',
        action: 'How to add a key',
      };
  }
}

// --- start_rider args --------------------------------------------------------

/**
 * Maps a selection to `start_rider` args (verbatim passthrough in
 * `gateway.rs::dispatch_start_rider`). Model id rides `--model`; a variant
 * rides its own flag (e.g. `--effort high`). Unknown models yield no args
 * rather than a guess.
 */
export function selectionToArgs(sel: ModelSelection | undefined, catalog: CatalogModel[]): string[] | undefined {
  if (!sel) return undefined;
  const model = catalog.find((m) => m.harness === sel.harnessId && m.id === sel.modelId);
  if (!model) return undefined;
  const args = ['--model', model.id];
  if (sel.variantId) {
    const variant = (model.variants ?? []).find((v) => v.id === sel.variantId);
    if (variant) args.push(variant.flag, variant.id);
  }
  return args;
}
