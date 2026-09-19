import type { Connection } from '../model/store';

/**
 * Honest gateway connectivity: "connecting" on first open, "reconnecting"
 * when the socket drops (riders keep working -- this is display-only).
 * Renders nothing when open so the quiet state stays one line.
 */
export function ConnectionBar({ connection }: { connection: Connection }) {
  if (connection === 'open') return null;
  if (connection === 'connecting') {
    return (
      <div role="status" aria-live="polite" className="deck-connection">
        connecting&hellip;
      </div>
    );
  }
  return (
    <div role="alert" className="deck-connection deck-connection-lost">
      reconnecting&hellip; your riders keep working
    </div>
  );
}
