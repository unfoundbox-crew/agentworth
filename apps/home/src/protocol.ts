/**
 * Wire protocol between the home UI and the gateway in `archie serve`.
 * One JSON object per WebSocket frame. Rust mirrors these in
 * apps/cli/src/server/home_gateway.rs; change both or neither.
 */

export const PROTOCOL = 2;

/** Agent lifecycle as herdr reports it. Same five words herdr uses. */
export type Presence = 'idle' | 'working' | 'blocked' | 'done' | 'unknown';

/** Which harness runs behind a persona. Read from herdr, never assumed. */
export type AgentKind = 'claude' | 'codex' | 'grok' | 'gemini' | 'agy' | 'cursor' | 'opencode' | string;

/** A seat at the table. `role` is functional; themes map it to a name and face. */
export type Role = 'chief_of_staff' | 'senior' | 'associate' | 'executor' | 'controller' | 'guest';

export interface Persona {
  id: string;
  role: Role;
  kind: AgentKind;
  /** herdr agent name, e.g. "partner-harvey". */
  agentName: string;
  paneId: string;
  workspaceId: string;
  cwd: string;
  presence: Presence;
  /** herdr terminal title, the agent's own one-line "what I'm on". */
  title: string;
  revision: number;
}

/** Archie's evidence ladder, lowest rung first. A direction is done when it reaches its rung. */
export type Rung = 'said' | 'artifact' | 'test' | 'commit' | 'ci';

/**
 * A direction is a standing intent the human set once. Riders subscribe to it and inherit it;
 * editing it steers every rider. This is the primary unit of the home, not the message.
 */
export interface Direction {
  id: string;
  /** One line. */
  goal: string;
  /** Repo path or worktree the direction owns. */
  area: string;
  /** Done means this rung reached, with evidence. */
  done: Rung;
  /** Highest rung any rider has earned so far, null before any evidence. */
  reached: Rung | null;
  budgetTokens: number;
  spentTokens: number;
  /** Persona ids riding this direction. */
  riders: string[];
  state: 'riding' | 'idle' | 'done' | 'waiting' | 'halted';
  /** Set when state is waiting or halted: why, and since when. */
  exception: { reason: string; since: string } | null;
  createdAt: string;
  updatedAt: string;
}

/** A stop on the track: evidence a rider produced, at the rung it earned. */
export interface Stop {
  id: string;
  directionId: string;
  from: string;
  rung: Rung;
  artifactId: string | null;
  at: string;
}

export type SpaceKind = 'office' | 'room';

export interface Space {
  id: string;
  kind: SpaceKind;
  /** Functional label; the theme may rename it. */
  label: string;
  members: string[];
  unread: number;
  /** One line for the sidebar and, later, for the ear. */
  lastSummary: string | null;
  lastActivity: string | null;
}

/**
 * What appears in the stream. Only `speech` renders as a bubble. `work` is a
 * collapsed one-liner ("edited 3 files, ran tests") that expands into the
 * drawer. Raw tool output never enters the stream.
 */
export type MessageKind = 'speech' | 'work' | 'system';

export interface Message {
  id: string;
  spaceId: string;
  /** Persona id, or "you". */
  from: string;
  kind: MessageKind;
  text: string;
  at: string;
  /** For `work`: ids of artifacts this turn produced. */
  artifacts?: string[];
  /** Persona ids addressed with @, when any. */
  mentions?: string[];
}

export type ArtifactKind = 'diff' | 'file' | 'command' | 'link' | 'note';

export interface Artifact {
  id: string;
  spaceId: string;
  from: string;
  kind: ArtifactKind;
  title: string;
  /** Path, URL, or the command line. */
  ref: string;
  /** Present for diff/file/command. Loaded lazily, may be null until opened. */
  body: string | null;
  at: string;
}

export type ServerFrame =
  | { t: 'hello'; protocol: number; personas: Persona[]; spaces: Space[]; directions: Direction[] }
  | { t: 'direction'; direction: Direction }
  | { t: 'stop'; stop: Stop }
  | { t: 'presence'; personaId: string; presence: Presence; title: string; revision: number }
  | { t: 'message'; message: Message }
  | { t: 'artifact'; artifact: Artifact }
  | { t: 'space'; space: Space }
  | { t: 'backfill'; spaceId: string; messages: Message[]; artifacts: Artifact[] }
  | { t: 'error'; code: string; detail: string };

/** Whether a steer interrupts the rider now or lands after its current step. Never ambiguous. */
export type SteerMode = 'now' | 'after';

export type ClientFrame =
  | { t: 'open'; spaceId: string }
  | { t: 'set_direction'; direction: Omit<Direction, 'reached' | 'spentTokens' | 'state' | 'exception' | 'createdAt' | 'updatedAt'> }
  | { t: 'steer'; directionId: string; text: string; mode: SteerMode; mentions: string[] }
  | { t: 'prompt'; spaceId: string; text: string; mentions: string[] }
  | { t: 'seen'; spaceId: string; upto: string }
  | { t: 'fetch'; artifactId: string };
