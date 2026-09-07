import { useSyncExternalStore } from 'react';
import type { Artifact, Direction, Message, Persona, ServerFrame, Space, Stop } from '../protocol';
import { plain, type Theme } from './theme';

export type Connection = 'connecting' | 'open' | 'closed';

export interface State {
  connection: Connection;
  theme: Theme;
  personas: Record<string, Persona>;
  spaces: Record<string, Space>;
  spaceOrder: string[];
  directions: Record<string, Direction>;
  stops: Record<string, Stop[]>;
  selectedDirection: string | null;
  consoleOpen: boolean;
  mute: Set<string>;
  solo: Set<string>;
  messages: Record<string, Message[]>;
  artifacts: Record<string, Artifact>;
  currentSpace: string | null;
  drawer: { open: boolean; artifactId: string | null };
}

const initial: State = {
  connection: 'connecting',
  theme: plain,
  personas: {},
  spaces: {},
  spaceOrder: [],
  directions: {},
  stops: {},
  selectedDirection: null,
  consoleOpen: false,
  mute: new Set(),
  solo: new Set(),
  messages: {},
  artifacts: {},
  currentSpace: null,
  drawer: { open: false, artifactId: null },
};

type Action =
  | { type: 'frame'; frame: ServerFrame }
  | { type: 'connection'; connection: Connection }
  | { type: 'select'; spaceId: string }
  | { type: 'select_direction'; directionId: string | null }
  | { type: 'open_console'; directionId: string }
  | { type: 'close_console' }
  | { type: 'toggle_mute'; personaId: string }
  | { type: 'toggle_solo'; personaId: string }
  | { type: 'theme'; theme: Theme }
  | { type: 'drawer'; open: boolean; artifactId?: string | null }
  | { type: 'local'; message: Message };

function reduce(s: State, a: Action): State {
  switch (a.type) {
    case 'connection':
      return { ...s, connection: a.connection };
    case 'select':
      return { ...s, currentSpace: a.spaceId, spaces: { ...s.spaces, [a.spaceId]: { ...s.spaces[a.spaceId], unread: 0 } } };
    case 'select_direction':
      return { ...s, selectedDirection: a.directionId };
    case 'open_console':
      return { ...s, selectedDirection: a.directionId, consoleOpen: true };
    case 'close_console':
      return { ...s, consoleOpen: false };
    case 'toggle_mute': {
      const next = new Set(s.mute);
      if (next.has(a.personaId)) next.delete(a.personaId);
      else next.add(a.personaId);
      return { ...s, mute: next };
    }
    case 'toggle_solo': {
      const next = new Set(s.solo);
      if (next.has(a.personaId)) next.delete(a.personaId);
      else next.add(a.personaId);
      return { ...s, solo: next };
    }
    case 'theme':
      return { ...s, theme: a.theme };
    case 'drawer':
      return { ...s, drawer: { open: a.open, artifactId: a.artifactId ?? s.drawer.artifactId } };
    case 'local':
      return { ...s, messages: { ...s.messages, [a.message.spaceId]: [...(s.messages[a.message.spaceId] ?? []), a.message] } };
    case 'frame':
      return applyFrame(s, a.frame);
  }
}

function applyFrame(s: State, f: ServerFrame): State {
  switch (f.t) {
    case 'hello': {
      const personas = Object.fromEntries(f.personas.map((p) => [p.id, p]));
      const spaces = Object.fromEntries(f.spaces.map((sp) => [sp.id, sp]));
      const directions = Object.fromEntries(f.directions.map((d) => [d.id, d]));
      return { ...s, personas, spaces, directions, spaceOrder: f.spaces.map((sp) => sp.id), currentSpace: s.currentSpace ?? f.spaces[0]?.id ?? null };
    }
    case 'presence': {
      const p = s.personas[f.personaId];
      if (!p) return s;
      return { ...s, personas: { ...s.personas, [p.id]: { ...p, presence: f.presence, title: f.title, revision: f.revision } } };
    }
    case 'message': {
      const m = f.message;
      const list = s.messages[m.spaceId] ?? [];
      if (list.some((x) => x.id === m.id)) return s;
      // The gateway's own record of a steer carries the same `clientId` the dock's optimistic
      // local echo used -- replace that echo in place instead of appending a second copy of the
      // same line (see `dispatch_steer`/`post_you_message` in gateway.rs and `Dock.tsx`'s `send`).
      const echoIndex = m.clientId && m.from === 'you' ? list.findIndex((x) => x.clientId === m.clientId && x.from === 'you') : -1;
      const nextList = echoIndex === -1 ? [...list, m] : [...list.slice(0, echoIndex), m, ...list.slice(echoIndex + 1)];
      const sp = s.spaces[m.spaceId];
      const unread = sp && m.spaceId !== s.currentSpace && m.from !== 'you' ? sp.unread + 1 : sp?.unread ?? 0;
      return {
        ...s,
        messages: { ...s.messages, [m.spaceId]: nextList },
        spaces: sp ? { ...s.spaces, [m.spaceId]: { ...sp, unread, lastActivity: m.at, lastSummary: m.kind === 'speech' ? m.text.slice(0, 120) : sp.lastSummary } } : s.spaces,
      };
    }
    case 'direction':
      return { ...s, directions: { ...s.directions, [f.direction.id]: f.direction } };
    case 'stop': {
      const list = s.stops[f.stop.directionId] ?? [];
      if (list.some((x) => x.id === f.stop.id)) return s;
      return { ...s, stops: { ...s.stops, [f.stop.directionId]: [...list, f.stop] } };
    }
    case 'artifact':
      return { ...s, artifacts: { ...s.artifacts, [f.artifact.id]: f.artifact } };
    case 'space':
      return { ...s, spaces: { ...s.spaces, [f.space.id]: f.space }, spaceOrder: s.spaceOrder.includes(f.space.id) ? s.spaceOrder : [...s.spaceOrder, f.space.id] };
    case 'backfill':
      return {
        ...s,
        messages: { ...s.messages, [f.spaceId]: f.messages },
        artifacts: { ...s.artifacts, ...Object.fromEntries(f.artifacts.map((a) => [a.id, a])) },
      };
    case 'error':
      console.warn('gateway', f.code, f.detail);
      return s;
  }
}

let state = initial;
const listeners = new Set<() => void>();

export function dispatch(a: Action) {
  const next = reduce(state, a);
  if (next === state) return;
  state = next;
  listeners.forEach((l) => l());
}

export function useHome<T>(select: (s: State) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => select(state),
    () => select(state),
  );
}

export function getState() {
  return state;
}

const RUNGS = ['said', 'artifact', 'test', 'commit', 'ci'] as const;

/** Exceptions first, oldest wait first; then the rest by last update. The strip board's order. */
export function orderDirections(ds: Direction[]): Direction[] {
  const waiting = ds.filter((d) => d.exception).sort((a, b) => a.exception!.since.localeCompare(b.exception!.since));
  const rest = ds.filter((d) => !d.exception).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  return [...waiting, ...rest];
}

export function rungIndex(r: Direction['reached']): number {
  return r ? RUNGS.indexOf(r) : -1;
}
