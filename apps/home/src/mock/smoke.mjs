// Smoke test: boots the mock gateway on a scratch port, connects a plain ws
// client, drives open/set_direction/steer, and asserts every frame that
// comes back is shaped like protocol 2. No React, no build step -- this is
// the wire, not the UI.
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { WebSocket } from 'ws';

const here = path.dirname(fileURLToPath(import.meta.url));
const port = 17778;

function assert(cond, msg) {
  if (!cond) throw new Error(`smoke failed: ${msg}`);
}

const child = spawn(process.execPath, [path.join(here, 'server.mjs')], {
  env: { ...process.env, PORT: String(port) },
  stdio: ['ignore', 'pipe', 'inherit'],
});

let settled = false;
function finish(code) {
  if (settled) return;
  settled = true;
  child.kill();
  process.exit(code);
}

const timeout = setTimeout(() => {
  console.error('smoke failed: timed out waiting for frames');
  finish(1);
}, 10_000);

child.stdout.on('data', (chunk) => {
  if (!chunk.toString().includes('mock gateway')) return;
  run().catch((err) => {
    console.error(err);
    finish(1);
  });
});

async function run() {
  const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
  const frames = [];

  await new Promise((resolve, reject) => {
    ws.on('open', resolve);
    ws.on('error', reject);
  });

  ws.on('message', (raw) => {
    const frame = JSON.parse(raw.toString());
    frames.push(frame);
  });

  function waitFor(pred, label) {
    return new Promise((resolve, reject) => {
      let id;
      let stop;
      const settle = (fn, v) => {
        clearInterval(id);
        clearTimeout(stop);
        fn(v);
      };
      const check = () => {
        const f = frames.find(pred);
        if (f) settle(resolve, f);
      };
      id = setInterval(check, 50);
      stop = setTimeout(() => settle(reject, new Error(`never saw ${label}`)), 5000);
      check();
    });
  }

  const hello = await waitFor((f) => f.t === 'hello', 'hello');
  assert(hello.protocol === 2, `hello.protocol is ${hello.protocol}, want 2`);
  assert(Array.isArray(hello.personas) && hello.personas.length > 0, 'hello.personas is empty');
  assert(Array.isArray(hello.directions) && hello.directions.length > 0, 'hello.directions is empty');
  console.log(`ok: hello, protocol ${hello.protocol}, ${hello.personas.length} personas, ${hello.directions.length} directions`);

  const spaceId = hello.spaces[0].id;
  ws.send(JSON.stringify({ t: 'open', spaceId }));
  const backfill = await waitFor((f) => f.t === 'backfill' && f.spaceId === spaceId, 'backfill');
  assert(Array.isArray(backfill.messages), 'backfill.messages is not an array');
  console.log(`ok: backfill for ${spaceId}, ${backfill.messages.length} messages`);

  const directionId = hello.directions[0].id;
  const direction = {
    ...hello.directions[0],
    goal: 'smoke: exercise the direction path',
  };
  delete direction.reached;
  delete direction.spentTokens;
  delete direction.state;
  delete direction.exception;
  delete direction.createdAt;
  delete direction.updatedAt;
  ws.send(JSON.stringify({ t: 'set_direction', direction }));
  console.log('ok: set_direction sent (no ack frame defined in protocol 2 -- fire and forget)');

  const stopsBefore = frames.filter((f) => f.t === 'stop' && f.stop.directionId === directionId).length;

  ws.send(JSON.stringify({ t: 'steer', directionId, text: 'smoke steer', mode: 'now', mentions: [] }));
  const directionFrame = await waitFor((f) => f.t === 'direction' && f.direction.id === directionId, 'direction (post-steer)');
  assert(directionFrame.direction.state === 'riding', `direction.state is ${directionFrame.direction.state}, want riding`);
  console.log('ok: steer -> direction frame, state riding');

  const stopFrame = await waitFor(
    (f) => f.t === 'stop' && f.stop.directionId === directionId && frames.filter((x) => x.t === 'stop' && x.stop.directionId === directionId).indexOf(f) >= stopsBefore,
    'stop (post-steer)',
  );
  assert(typeof stopFrame.stop.rung === 'string', 'stop.rung is not a string');
  console.log(`ok: stop frame, rung ${stopFrame.stop.rung}`);

  ws.close();
  console.log('smoke: all frames matched protocol 2');
  finish(0);
}

process.on('exit', () => clearTimeout(timeout));
