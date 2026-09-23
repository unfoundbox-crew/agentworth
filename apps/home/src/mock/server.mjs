// Stand-in for the Rust gateway. Serves the same frames from a fixture so the
// UI can be built and reviewed with no Rust build. `npm run mock`, then `npm run dev`.
//
// One port, two surfaces: /ws speaks protocol 2 frames, /api/insights answers
// GET with the synthetic insights fixture (insights-fixture.json) that the
// deck's insights dashboard builds against. The real backend lane ships GET
// /api/insights in the CLI server; this mock exists so neither UI nor
// contract waits on a cargo build.
import { WebSocketServer } from 'ws';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createServer } from 'node:http';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(readFileSync(path.join(here, 'fixture.json'), 'utf8'));
const insights = JSON.parse(readFileSync(path.join(here, 'insights-fixture.json'), 'utf8'));
const port = Number(process.env.PORT ?? 7777);

const httpServer = createServer((req, res) => {
  if (req.url?.startsWith('/api/insights')) {
    if (req.method !== 'GET') {
      res.writeHead(405, { 'content-type': 'text/plain' });
      res.end('method not allowed');
      return;
    }
    // Window variants: the fixture is shaped for 30d; a request naming
    // since/until gets a deterministically scaled copy (7d x0.28, 90d x2.6,
    // everything else x1) with per-bucket jitter so the deck's delta and
    // sparkline rows visibly change when the CEO flips the preset. Same
    // rule as the real lane: ratios drift a little, counts scale, nothing
    // is fabricated beyond the synthetic fixture itself.
    const url = new URL(req.url, 'http://127.0.0.1');
    const sinceMs = Date.parse(url.searchParams.get('since') ?? '');
    const untilMs = Date.parse(url.searchParams.get('until') ?? '');
    const windowed = Number.isFinite(sinceMs);
    const end = Number.isFinite(untilMs) ? untilMs : Date.now();
    const days = windowed ? Math.max(1, Math.round((end - sinceMs) / 86400000)) : 0;
    const bucket = days <= 10 ? '7d' : days <= 45 ? '30d' : '90d';
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify(scaleInsightsWindow(insights, bucket)));
    return;
  }
  res.writeHead(404, { 'content-type': 'text/plain' });
  res.end('not found');
});

/**
 * Deterministic per-window variant of the synthetic fixture. Bucket -> scale
 * factor + seeded jitter, so 7d/30d/90d produce different totals, different
 * deltas and different sparkline shapes — same numbers for the same window,
 * every time.
 */
const BUCKET_SCALE = { '7d': 0.28, '30d': 1, '90d': 2.6 };

function seededJitter(bucket, index, spread) {
  // tiny LCG, deterministic per (bucket, index)
  let x = 0;
  for (const ch of bucket) x = (x * 31 + ch.charCodeAt(0)) | 0;
  x = (x * 1103515245 + 12345 + index * 7919) & 0x7fffffff;
  return 1 + ((x % 2001) - 1000) / 1000 * spread;
}

function scaleNumber(n, scale, spread, bucket, salt = '') {
  if (typeof n !== 'number') return n;
  const scaled = n * scale * seededJitter(bucket + salt, String(n).length, spread);
  return Number(scaled.toFixed(1));
}

function scaleInsightsWindow(base, bucket) {
  if (!bucket || bucket === '30d') return base;
  const scale = BUCKET_SCALE[bucket] ?? 1;
  const j = (n, spread = 0.25) => scaleNumber(n, scale, spread, bucket);
  const out = structuredClone(base);

  if (out.current) {
    const cur = out.current;
    cur.claimed = Math.max(1, Math.round(j(cur.claimed)));
    for (const kpi of cur.kpis ?? []) {
      if (kpi.key === 'verified' || kpi.key === 'friction') {
        // ratios drift a couple of points, directionally constant per bucket
        const drift = seededJitter(bucket + ':kpi:' + kpi.key, kpi.value, 0.03);
        kpi.value = Number((kpi.value * drift).toFixed(1));
      } else {
        kpi.value = Math.max(0, Math.round(j(kpi.value)));
      }
      if (Array.isArray(kpi.series)) {
        kpi.series = kpi.series.map((v) => Math.max(0, Math.round(j(v, 0.45))));
      }
    }
    for (const row of cur.ladder ?? []) row.sessions = Math.max(0, Math.round(j(row.sessions)));
    for (const row of cur.by_adapter ?? []) { row.sessions = Math.max(0, Math.round(j(row.sessions))); row.tokens_tb = j(row.tokens_tb); }
    for (const row of cur.models ?? []) {
      row.sessions = Math.max(0, Math.round(j(row.sessions)));
      row.tb = j(row.tb);
      row.it = j(row.it);
      row.ot = j(row.ot);
    }
    for (const row of cur.buckets ?? []) row.sessions = Math.max(0, Math.round(j(row.sessions)));
    for (const row of cur.repos ?? []) { row.sessions = Math.max(0, Math.round(j(row.sessions))); row.events = Math.max(0, Math.round(j(row.events))); }
    for (const row of cur.vocabulary ?? []) row.sessions = Math.max(0, Math.round(j(row.sessions)));
    for (const row of cur.big_sessions ?? []) { row.tokens = Math.max(0, Math.round(j(row.tokens))); row.events = Math.max(0, Math.round(j(row.events))); }
    // day_hour: JSX heatmap cells scale too, so the grid visibly changes with the preset
    for (const cell of cur.day_hour ?? []) cell.sessions = Math.max(0, Math.round(j(cell.sessions, 0.4)));

    if (out.previous) {
      const prev = out.previous;
      prev.claimed = Math.max(1, Math.round(j(prev.claimed)));
      for (const kpi of prev.kpis ?? []) {
        if (kpi.key === 'verified' || kpi.key === 'friction') {
          kpi.value = Number((kpi.value * seededJitter(bucket + ':pkpi:' + kpi.key, kpi.value, 0.03)).toFixed(1));
        } else {
          kpi.value = Math.max(0, Math.round(j(kpi.value)));
        }
      }
    }
  }
  return out;
}

const wss = new WebSocketServer({ server: httpServer, path: '/ws' });

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

httpServer.listen(port, '127.0.0.1', () => {
  console.log(`mock gateway ws://127.0.0.1:${port}/ws · insights http://127.0.0.1:${port}/api/insights`);
});
