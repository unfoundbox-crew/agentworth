// Stand-in for the Rust gateway. Serves the same frames from a fixture so the
// UI can be built and reviewed with no Rust build. `npm run mock`, then `npm run dev`.
import { WebSocketServer } from 'ws';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(readFileSync(path.join(here, 'fixture.json'), 'utf8'));
const port = Number(process.env.PORT ?? 7777);
const wss = new WebSocketServer({ port, path: '/ws' });

let seq = 1000;
const now = () => new Date().toISOString();
const id = (p) => `${p}-${seq++}`;

// Stand-in for `detect_env` (protocol.rs): fixed rather than probed, since the mock has no
// real PATH or herdr socket to check.
const env = {
  cwd: '/repo',
  repo: '/repo',
  harnesses: [
    { id: 'claude', label: 'Claude', bin: 'claude' },
    { id: 'codex', label: 'Codex', bin: 'codex' },
  ],
  herdr: 'ok',
  budgetDefaultTokens: 5_000_000,
};

wss.on('connection', (ws) => {
  const send = (f) => ws.readyState === 1 && ws.send(JSON.stringify(f));
  send({ t: 'hello', protocol: 2, personas: fixture.personas, spaces: fixture.spaces, directions: fixture.directions, env });
  for (const stop of fixture.stops) send({ t: 'stop', stop });

  ws.on('message', (raw) => {
    let f;
    try { f = JSON.parse(raw); } catch { return; }
    if (f.t === 'open') {
      send({ t: 'backfill', spaceId: f.spaceId, messages: fixture.messages.filter((m) => m.spaceId === f.spaceId), artifacts: fixture.artifacts.filter((a) => a.spaceId === f.spaceId) });
    }
    if (f.t === 'prompt') {
      const space = fixture.spaces.find((s) => s.id === f.spaceId);
      const who = f.mentions[0] ?? space?.members[0];
      if (!who) return;
      send({ t: 'presence', personaId: who, presence: 'working', title: 'thinking', revision: seq });
      setTimeout(() => {
        const art = { id: id('art'), spaceId: f.spaceId, from: who, kind: 'diff', title: 'src/example.ts', ref: 'src/example.ts', body: '--- a/src/example.ts\n+++ b/src/example.ts\n@@ -1 +1,2 @@\n-export const x = 1;\n+export const x = 2;\n+export const y = x * 2;\n', at: now() };
        send({ t: 'artifact', artifact: art });
        send({ t: 'message', message: { id: id('m'), spaceId: f.spaceId, from: who, kind: 'work', text: 'edited 1 file', at: now(), artifacts: [art.id] } });
        send({ t: 'message', message: { id: id('m'), spaceId: f.spaceId, from: who, kind: 'speech', text: `On it. ${f.text.length > 40 ? 'That is a bigger ask than it looks; I will start with the smallest piece.' : 'Done, one file changed.'}`, at: now() } });
        send({ t: 'presence', personaId: who, presence: 'idle', title: 'idle', revision: seq });
      }, 1200);
    }
    if (f.t === 'steer') {
      const d = fixture.directions.find((x) => x.id === f.directionId);
      if (!d) return;
      send({
        t: 'message',
        message: { id: id('m'), spaceId: `office-${d.riders[0]}`, from: 'you', kind: 'speech', text: f.text, at: now(), clientId: f.clientId },
      });
      const next = { ...d, state: 'riding', exception: null, updatedAt: now() };
      Object.assign(d, next);
      send({ t: 'direction', direction: next });
      setTimeout(() => send({ t: 'stop', stop: { id: id('s'), directionId: d.id, from: d.riders[0], rung: 'artifact', artifactId: null, at: now() } }), 1500);
    }
    if (f.t === 'fetch') {
      const a = fixture.artifacts.find((x) => x.id === f.artifactId);
      if (a) send({ t: 'artifact', artifact: a });
    }
    if (f.t === 'start_rider') {
      const harness = env.harnesses.find((h) => h.id === f.harness);
      if (!harness) {
        send({ t: 'error', code: 'herdr_error', detail: `no such harness: ${f.harness}` });
        return;
      }
      const shortId = f.directionId.replace(/-/g, '').slice(0, 8);
      const personaId = `${harness.id}-${shortId}`;
      const persona = {
        id: personaId,
        role: 'executor',
        kind: harness.id,
        agentName: personaId,
        paneId: `mock:${personaId}`,
        workspaceId: 'mock',
        cwd: env.repo ?? env.cwd,
        presence: 'idle',
        title: '',
        revision: 0,
      };
      send({ t: 'persona', persona });
      const d = fixture.directions.find((x) => x.id === f.directionId) ?? { id: f.directionId, riders: [] };
      if (!d.riders.includes(personaId)) d.riders.push(personaId);
      send({ t: 'direction', direction: { ...d, riders: d.riders, updatedAt: now() } });
      setTimeout(() => {
        send({
          t: 'message',
          message: { id: id('m'), spaceId: `office-${personaId}`, from: personaId, kind: 'speech', text: 'reading the repo now.', at: now() },
        });
      }, 400);
    }
    if (f.t === 'answer') {
      const persona = fixture.personas.find((p) => p.id === f.personaId);
      if (!persona || persona.presence !== 'blocked') {
        send({ t: 'error', code: 'not_blocked', detail: `${f.personaId} is not blocked; nothing to answer` });
        return;
      }
      send({
        t: 'message',
        message: {
          id: id('m'),
          spaceId: `office-${persona.id}`,
          from: 'you',
          kind: 'system',
          text: `you answered ${f.key} on ${persona.agentName}`,
          at: now(),
        },
      });
    }
  });

  // Ambient presence so the sidebar breathes.
  const tick = setInterval(() => {
    const p = fixture.personas[Math.floor(Math.random() * fixture.personas.length)];
    const states = ['idle', 'working', 'working', 'blocked', 'done'];
    send({ t: 'presence', personaId: p.id, presence: states[Math.floor(Math.random() * states.length)], title: p.title, revision: seq++ });
  }, 4000);
  ws.on('close', () => clearInterval(tick));
});

console.log(`mock gateway ws://127.0.0.1:${port}/ws`);
