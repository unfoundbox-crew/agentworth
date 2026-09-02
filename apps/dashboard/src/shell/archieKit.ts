import { useSyncExternalStore } from 'react';
import {
  ARCHIE_DEFAULT_ACCESSORY,
  ARCHIE_DEFAULT_COLOURWAY,
  isAccessory,
  isColourway,
  type ArchieAccessory,
  type ArchieColourway,
} from '@ui/Archie';

/**
 * Where Archie's kit and colourway live, for the whole shell.
 *
 * The binary persists them in the same config.toml `agentworth config set` writes, so a
 * choice made here survives a restart and matches what the CLI draws. Served as a static
 * demo there is no API to talk to, and the choice falls back to this browser's own
 * storage — which the picker says out loud rather than pretending it saved.
 */
export type ArchieSource = 'api' | 'local';

export interface ArchieSettings {
  accessory: ArchieAccessory;
  colourway: ArchieColourway;
  source: ArchieSource;
  loaded: boolean;
}

const ACCESSORY_KEY = 'agentworth_archie_accessory';
const COLOURWAY_KEY = 'agentworth_archie_colourway';

let state: ArchieSettings = {
  accessory: ARCHIE_DEFAULT_ACCESSORY,
  colourway: ARCHIE_DEFAULT_COLOURWAY,
  source: 'api',
  loaded: false,
};

const listeners = new Set<() => void>();

/** The default is stamped before anything asks for it, so a surface that reads the
 *  attribute never sees it missing. */
queueMicrotask(() => applyToDocument());

function emit(next: Partial<ArchieSettings>) {
  state = { ...state, ...next };
  applyToDocument();
  listeners.forEach((l) => l());
}

/** The kit as a root attribute, so anything on the page can read the current choice
 *  without threading a prop through every pane. */
function applyToDocument() {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-accessory', state.accessory);
}

function readLocal(): Partial<ArchieSettings> {
  try {
    const accessory = localStorage.getItem(ACCESSORY_KEY);
    const colourway = localStorage.getItem(COLOURWAY_KEY);
    return {
      ...(isAccessory(accessory) ? { accessory } : {}),
      ...(isColourway(colourway) ? { colourway } : {}),
    };
  } catch {
    // Private window, or site data blocked. The picker still works for this visit.
    return {};
  }
}

function writeLocal(next: Partial<ArchieSettings>) {
  try {
    if (next.accessory) localStorage.setItem(ACCESSORY_KEY, next.accessory);
    if (next.colourway) localStorage.setItem(COLOURWAY_KEY, next.colourway);
  } catch {
    /* nothing to do: the choice holds for this visit and is gone on the next. */
  }
}

let loading: Promise<void> | null = null;

/** Reads the persisted config once. Called when the picker opens, not on shell mount:
 *  nothing else on the dashboard needs it, so nothing else should pay for the request. */
export function loadArchieSettings(): Promise<void> {
  if (loading) return loading;
  loading = fetch('/api/config')
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
    .then((body: Record<string, unknown>) => {
      const accessory = body['archie.accessory'];
      const colourway = body['archie.colourway'];
      emit({
        accessory: isAccessory(accessory) ? accessory : ARCHIE_DEFAULT_ACCESSORY,
        colourway: isColourway(colourway) ? colourway : ARCHIE_DEFAULT_COLOURWAY,
        source: 'api',
        loaded: true,
      });
    })
    .catch(() => {
      emit({ ...readLocal(), source: 'local', loaded: true });
    });
  return loading;
}

/** Applies the choice immediately, then tries to persist it. A write that cannot reach
 *  the API is kept in this browser and reported as such — never silently dropped. */
export async function setArchieSetting(next: Partial<ArchieSettings>): Promise<void> {
  emit(next);

  const body: Record<string, string> = {};
  if (next.accessory) body['archie.accessory'] = next.accessory;
  if (next.colourway) body['archie.colourway'] = next.colourway;

  try {
    const response = await fetch('/api/config', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw new Error(String(response.status));
    emit({ source: 'api' });
  } catch {
    writeLocal(next);
    emit({ source: 'local' });
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useArchieSettings(): ArchieSettings {
  return useSyncExternalStore(
    subscribe,
    () => state,
    () => state
  );
}
