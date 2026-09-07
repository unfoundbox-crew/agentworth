import { useSyncExternalStore } from 'react';
import type { Artifact, Message, Persona, ServerFrame, Space } from '../protocol';
import { plain, type Theme } from './theme';

export type Connection = 'connecting' | 'open' | 'closed';

export interface State {
  connection: Connection;
  theme: Theme;
  personas: Record<string, Persona>;
  spaces: Record<string, Space>;
  spaceOrder: string[];
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
  messages: {},
  artifacts: {},
  currentSpace: null,
  drawer: { open: false, artifactId: null },
};

type Action =
  | { type: 'frame'; frame: ServerFrame }
  | { type: 'connection'; connection: Connection }
  | { type: 'select'; spaceId: string }
  | { type: 'theme'; theme: Theme }
  | { type: 'drawer'; open: boolean; artifactId?: string | null }
  | { type: 'local'; message: Message };

function reduce(s: State, a: Action): State {
  switch (a.type) {
    case 'connection':
      return { ...s, connection: a.connection };
    case 'select':
      return { ...s, currentSpace: a.spaceId, spaces: { ...s.spaces, [a.spaceId]: { ...s.spaces[a.spaceId], unread: 0 } } };
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
      return { ...s, personas, spaces, spaceOrder: f.spaces.map((sp) => sp.id), currentSpace: s.currentSpace ?? f.spaces[0]?.id ?? null };
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
      const sp = s.spaces[m.spaceId];
      const unread = sp && m.spaceId !== s.currentSpace && m.from !== 'you' ? sp.unread + 1 : sp?.unread ?? 0;
      return {
        ...s,
        messages: { ...s.messages, [m.spaceId]: [...list, m] },
        spaces: sp ? { ...s.spaces, [m.spaceId]: { ...sp, unread, lastActivity: m.at, lastSummary: m.kind === 'speech' ? m.text.slice(0, 120) : sp.lastSummary } } : s.spaces,
      };
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
  );
}

export function getState() {
  return state;
}
