import type { ClientFrame, ServerFrame } from '../protocol';
import { dispatch } from '../model/store';

/**
 * One socket, reconnects with backoff, replays `open` for the current space
 * after a reconnect so the stream backfills. Frames go straight to the store.
 */
export class Gateway {
  private ws: WebSocket | null = null;
  private delay = 500;
  private closed = false;
  private opened: string | null = null;

  constructor(private url: string) {}

  connect() {
    this.closed = false;
    dispatch({ type: 'connection', connection: 'connecting' });
    const ws = new WebSocket(this.url);
    this.ws = ws;
    ws.onopen = () => {
      this.delay = 500;
      dispatch({ type: 'connection', connection: 'open' });
      if (this.opened) this.send({ t: 'open', spaceId: this.opened });
    };
    ws.onmessage = (ev) => {
      let frame: ServerFrame;
      try {
        frame = JSON.parse(ev.data);
      } catch {
        return;
      }
      dispatch({ type: 'frame', frame });
    };
    ws.onclose = () => {
      dispatch({ type: 'connection', connection: 'closed' });
      if (this.closed) return;
      setTimeout(() => this.connect(), this.delay);
      this.delay = Math.min(this.delay * 2, 8000);
    };
  }

  open(spaceId: string) {
    this.opened = spaceId;
    this.send({ t: 'open', spaceId });
  }

  send(frame: ClientFrame) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(frame));
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }
}

const proto = location.protocol === 'https:' ? 'wss' : 'ws';
export const gateway = new Gateway(`${proto}://${location.host}/ws`);
