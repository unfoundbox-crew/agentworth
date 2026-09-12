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

/** A harness this machine can seat a rider on. `id` matches `herdr agent start --kind`. */
export type HarnessId = 'claude' | 'codex' | 'gemini' | 'agy' | 'opencode' | 'cursor' | string;

/** One row in the first-run "who rides" chip strip. `bin` is the executable found on PATH. */
export interface Harness {
  id: HarnessId;
  label: string;
  bin: string;
}

/** Whether herdr, required to seat any rider, is usable from here. */
export type HerdrStatus = 'ok' | 'missing' | 'no_socket';

/** What the server already knows about where it runs, detected once at startup. */
export interface HomeEnv {
  cwd: string;
  /** Git toplevel of `cwd`, or null outside a repo. */
  repo: string | null;
  harnesses: Harness[];
  herdr: HerdrStatus;
  /** A new direction's starting budget: this machine's own p90 token spend, or a plain 5M fallback. */
  budgetDefaultTokens: number;
}

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
  /**
   * Set on a `you` message so the deck can dedupe its own optimistic local echo against the
   * gateway's own record of the same steer: the local copy shows immediately for latency, then
   * is replaced (not appended to) when a `message` frame with the same `clientId` and
   * `from: 'you'` arrives.
   */
  clientId?: string;
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
  | { t: 'hello'; protocol: number; personas: Persona[]; spaces: Space[]; directions: Direction[]; env: HomeEnv }
  | { t: 'direction'; direction: Direction }
  | { t: 'stop'; stop: Stop }
  | { t: 'presence'; personaId: string; presence: Presence; title: string; revision: number }
  /** A persona the gateway just discovered off `herdr agent list` -- e.g. a rider `start_rider` just seated. */
  | { t: 'persona'; persona: Persona }
  | { t: 'message'; message: Message }
  | { t: 'artifact'; artifact: Artifact }
  | { t: 'space'; space: Space }
  | { t: 'backfill'; spaceId: string; messages: Message[]; artifacts: Artifact[] }
  | { t: 'error'; code: string; detail: string };

/** Whether a steer interrupts the rider now or lands after its current step. Never ambiguous. */
export type SteerMode = 'now' | 'after';

/**
 * A key press for `answer`: the numbered choices a CLI permission prompt offers (`1`/`2`/`3`),
 * `esc` to cancel, or `y`/`n` for a plain yes/no prompt. Matches what `herdr agent send-keys`
 * accepts verbatim.
 */
export type AnswerKey = '1' | '2' | '3' | 'esc' | 'y' | 'n';

export type ClientFrame =
  | { t: 'open'; spaceId: string }
  | { t: 'set_direction'; direction: Omit<Direction, 'reached' | 'spentTokens' | 'state' | 'exception' | 'createdAt' | 'updatedAt'> }
  | { t: 'steer'; directionId: string; text: string; mode: SteerMode; mentions: string[]; clientId?: string }
  | { t: 'prompt'; spaceId: string; text: string; mentions: string[] }
  | { t: 'seen'; spaceId: string; upto: string }
  | { t: 'fetch'; artifactId: string }
  /** Answers a blocked pane's prompt from the alert plate. Refused unless `personaId` is presently `blocked`. */
  | { t: 'answer'; directionId: string; personaId: string; key: AnswerKey; clientId?: string }
  /**
   * Seats a rider on a direction: a fresh herdr pane, the harness started in it, the
   * direction's goal sent as its first prompt. Refused if herdr is not `ok`. `args`, when
   * given, are passed through to `herdr agent start ... -- <args>` verbatim (e.g. `['--model',
   * 'haiku']`) -- not surfaced in the first-run UI, which never picks a harness's own flags for
   * the human; it exists for scripted/test seating.
   */
  | { t: 'start_rider'; directionId: string; harness: HarnessId; args?: string[]; clientId?: string };
