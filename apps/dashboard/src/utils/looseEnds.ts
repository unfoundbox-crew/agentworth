import type { NormalizedEvent } from '../types';

/**
 * An intent the assistant stated. Kept deliberately narrow: "let me" was tried
 * and dropped, because it almost always narrates something being done in the
 * same breath rather than promising it for later.
 */
const INTENT = /\b(i'?ll|i will|i'?m going to|i am going to)\b/i;

/**
 * A gated intent is not a loose end.
 *
 * This is the filter that makes the whole thing usable. Measured across five
 * real sessions, 120 of 212 stated intents are gated — conditional offers
 * ("say go and I'll write both") or deliberate deferrals ("I'll report once
 * they land"). Both were promised *subject to something*, so neither was
 * dropped, and reporting them as misses is what would make the output feel
 * accusatory and get ignored.
 */
const GATED =
  /\b(if|once|unless|when|after|until|pending|whenever|assuming|provided)\b|\b(say the word|say go|say which|let me know|want me to|shall i|would you like|approve|tell me|your call|happy to|say-so)\b|^say\b/i;

/**
 * "Paste me the error and I'll take it from there" is an offer waiting on the
 * user, not a commitment that was dropped — the same class as GATED, but
 * phrased as an imperative rather than a conditional, so the `if`/`once`
 * vocabulary misses it entirely.
 */
const AWAITING_USER =
  /\b(paste|send|share|give|drop|hand|point|show|run|confirm|approve|pick|choose)\s+(me|it|them|that|those|this|us)\b/i;

/** Events that count as the assistant actually doing something. */
const ACTIONS = new Set(['tool_call', 'shell_command', 'file_action']);

/** Too short to carry a commitment; too long to be one sentence. */
const MIN_LEN = 25;
const MAX_LEN = 240;

export interface LooseEnd {
  /** The sentence, verbatim. Attribution is only worth what it can quote. */
  text: string;
  /** Event id of the message that said it, for deep-linking. */
  eventId: string;
  sequence: number;
  timestamp: string | null;
  /** Model that said it, when the trace records one nearby. */
  model: string | null;
}

function splitSentences(text: string): string[] {
  return text
    .split(/(?<=[.!?])\s+|\n+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/**
 * Finds commitments the assistant stated and then handed control back without
 * acting on.
 *
 * The signal is deliberately crude and the spec is right that it does not need
 * to be clever: an assistant states an intent, emits no tool call, and the next
 * event is a user turn. Anything gated on a reply or a later event is excluded.
 *
 * This reports what has no evidence of happening. It does not claim the work
 * was forgotten — about half of what any such detector finds is work the user
 * cancelled, which is why the surface calls these loose ends rather than
 * misses.
 */
export function findLooseEnds(events: NormalizedEvent[]): LooseEnd[] {
  const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
  const out: LooseEnd[] = [];

  // Most recent model seen, so a loose end can name who said it.
  let currentModel: string | null = null;

  for (let i = 0; i < ordered.length; i++) {
    const event = ordered[i];
    const payload = event.payload as { type?: string; data?: Record<string, unknown> } | undefined;
    const type = payload?.type;

    if (type === 'model_invocation') {
      const model = (payload?.data ?? {}).model;
      if (typeof model === 'string') currentModel = model;
      continue;
    }
    if (type !== 'assistant_message') continue;

    const content = (payload?.data ?? {}).content;
    if (typeof content !== 'string' || content.length === 0) continue;

    const candidates = splitSentences(content).filter(
      (s) => s.length >= MIN_LEN && s.length <= MAX_LEN && INTENT.test(s) && !GATED.test(s) && !AWAITING_USER.test(s)
    );
    if (candidates.length === 0) continue;

    // Did anything actually happen before control went back to the user?
    let acted = false;
    for (let j = i + 1; j < ordered.length; j++) {
      const next = (ordered[j].payload as { type?: string } | undefined)?.type;
      if (next && ACTIONS.has(next)) {
        acted = true;
        break;
      }
      if (next === 'user_message') break;
    }
    if (acted) continue;

    for (const text of candidates) {
      out.push({
        text,
        eventId: event.id,
        sequence: event.sequence,
        timestamp: typeof event.timestamp === 'string' ? event.timestamp : null,
        model: currentModel,
      });
    }
  }

  return out;
}

/**
 * Builds the text a developer hands to whatever already has the repo open.
 *
 * Deliberately a prompt and not a patch: writing the fix would mean being right
 * about the fix, from something that read a transcript and never saw the
 * codebase. Being right about what is missing is the answerable half.
 */
export function looseEndsPrompt(ends: LooseEnd[], sessionId: string): string {
  const lines = ends.map((e) => `- ${e.text}`);
  return [
    `In an earlier session (${sessionId}) the following were said and have no evidence of being done:`,
    '',
    ...lines,
    '',
    'Check each against the current state of the repository. Some may have been done since, and some I may have cancelled — ask me about anything ambiguous rather than assuming. Then do the ones that are still outstanding.',
  ].join('\n');
}
