import type { Artifact, Direction, Rung, Stop } from '../protocol';

/**
 * Session review: which files a session changed, file by file, each carrying
 * its proof. The proof is session-level and labelled as such -- the index
 * attributes outcomes to sessions, not to individual files, so per-file
 * verification claims would be invented.
 *
 * Proof sources, both read-only and both already derived from the AgentWorth
 * index server-side: `direction.reached` is the highest outcome rung among the
 * direction's sessions (`home_direction_area_reached` in
 * `apps/cli/src/server/home/directions.rs`), and `stops` are the outcome
 * crossings the transcript feed reported for those same sessions. This module
 * never writes to the index.
 */

export type ReviewFileStatus = 'added' | 'modified' | 'deleted';

export interface FileProof {
  rung: Rung;
  label: string;
  /** Test (or stronger) citation, e.g. the command that earned the rung. */
  citation: string;
}

export interface ReviewedFile {
  path: string;
  title: string;
  status: ReviewFileStatus;
  body: string | null;
}

export interface SessionReviewData {
  files: ReviewedFile[];
  /** Null while the session sits below the ladder's first rung: unflown. */
  proof: FileProof | null;
}

export const RUNG_LABEL: Record<Rung, string> = {
  said: 'said',
  artifact: 'diff',
  test: 'test ok',
  commit: 'commit',
  ci: 'ci',
};

const RUNG_ORDER: Rung[] = ['said', 'artifact', 'test', 'commit', 'ci'];

function rungIndex(r: Rung): number {
  return RUNG_ORDER.indexOf(r);
}

/** Unified-diff sniff: `--- /dev/null` means the old side is empty (added). */
export function inferFileStatus(body: string | null): ReviewFileStatus {
  if (!body) return 'modified';
  const addedOld = body.includes('--- /dev/null');
  const deletedNew = body.includes('+++ /dev/null');
  if (addedOld && !deletedNew) return 'added';
  if (deletedNew && !addedOld) return 'deleted';
  if (/^new file/m.test(body)) return 'added';
  if (/^deleted file/m.test(body)) return 'deleted';
  return 'modified';
}

/** Latest stop at test rung or stronger; only those can cite a verification. */
function latestVerifyingStop(stops: Stop[]): Stop | null {
  const verifying = stops.filter((s) => rungIndex(s.rung) >= rungIndex('test'));
  if (verifying.length === 0) return null;
  return verifying.reduce((a, b) => (a.at >= b.at ? a : b));
}

function citationFor(stop: Stop, artifacts: Record<string, Artifact>): string {
  const a = stop.artifactId ? artifacts[stop.artifactId] : undefined;
  if (a) return a.ref && a.ref !== a.title ? `${a.title} · ${a.ref}` : a.title;
  return `${RUNG_LABEL[stop.rung]} · ${stop.from}`;
}

export function buildSessionReview(
  direction: Direction | undefined,
  stops: Stop[],
  artifacts: Record<string, Artifact>,
): SessionReviewData {
  const files = Object.values(artifacts)
    .filter((a) => a.kind === 'diff' || a.kind === 'file')
    .map((a) => ({
      path: a.ref || a.title,
      title: a.title,
      status: inferFileStatus(a.body),
      body: a.body,
    }))
    .sort((x, y) => x.path.localeCompare(y.path));

  const rungs: Rung[] = stops.map((s) => s.rung);
  if (direction?.reached) rungs.push(direction.reached);
  if (rungs.length === 0) return { files, proof: null };
  const top = rungs.reduce((a, b) => (rungIndex(a) >= rungIndex(b) ? a : b));

  const verifying = latestVerifyingStop(stops);
  if (!verifying) {
    return { files, proof: { rung: top, label: RUNG_LABEL[top], citation: '' } };
  }
  return {
    files,
    proof: { rung: top, label: RUNG_LABEL[top], citation: citationFor(verifying, artifacts) },
  };
}

/** Preferred path wins when it is still in the review, else the first file. */
export function selectReviewPath(review: SessionReviewData, preferred: string | null): string | null {
  if (preferred && review.files.some((f) => f.path === preferred)) return preferred;
  return review.files[0]?.path ?? null;
}
