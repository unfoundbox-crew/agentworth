import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';

import { downloadAndExtractBinary, downloadFile, readLockPid } from '../lib/resolver.js';

// Second-review findings on the #173/#174 lock code: (A) a takeover race where two waiters
// can both end up believing they're the confirmed holder, (B) a killed holder's own temp
// extract dir never getting swept, and (C) a stalled-with-no-timeout download holding a live
// lock hostage until the 15-minute staleness window, which lands it back in finding A's race.
const TEST_PLATFORM = 'linux';
const TEST_ARCH = 'x64';
const TEST_TRIPLE = 'x86_64-unknown-linux-gnu';
const DEAD_PID = 2147483647; // out of any real pid range -- process.kill(pid, 0) throws ESRCH

function tmpDir(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

function buildGoodArchive(workDir, version) {
  const srcDir = path.join(workDir, 'src');
  fs.mkdirSync(srcDir, { recursive: true });
  for (const name of ['agentworth', 'archie', 'agwt']) {
    fs.writeFileSync(path.join(srcDir, name), `#!/bin/sh\necho "agentworth ${version}"\nexit 0\n`, {
      mode: 0o755,
    });
  }
  const archiveName = `agentworth-v${version}-${TEST_TRIPLE}.tar.gz`;
  const archivePath = path.join(workDir, archiveName);
  execFileSync('tar', ['-czf', archivePath, '-C', srcDir, 'agentworth', 'archie', 'agwt']);
  const archiveBuffer = fs.readFileSync(archivePath);
  const sha256 = crypto.createHash('sha256').update(archiveBuffer).digest('hex');
  return { archiveName, archiveBuffer, sha256Text: `${sha256}  ${archiveName}\n`, sha256 };
}

/** Extracts the good archive straight into cacheDir -- stands in for "a real concurrent holder finished". */
function installWinnerSync(cacheDir, archiveBuffer, sha256) {
  fs.mkdirSync(cacheDir, { recursive: true });
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'winner-extract-'));
  const archivePath = path.join(tmp, 'winner.tar.gz');
  fs.writeFileSync(archivePath, archiveBuffer);
  execFileSync('tar', ['-xzf', archivePath, '-C', cacheDir]);
  for (const name of ['agentworth', 'archie', 'agwt']) {
    fs.chmodSync(path.join(cacheDir, name), 0o755);
  }
  fs.writeFileSync(path.join(cacheDir, '.installed'), `${sha256}\n`);
  fs.rmSync(tmp, { recursive: true, force: true });
}

function startFixtureServer({ archiveName, archiveBytes, shaText }) {
  const counts = {};
  const server = http.createServer((req, res) => {
    const name = path.basename((req.url || '').split('?')[0]);
    counts[name] = (counts[name] || 0) + 1;
    if (name === archiveName) {
      res.writeHead(200, { 'Content-Length': String(archiveBytes.length) });
      res.end(archiveBytes);
      return;
    }
    if (name === `${archiveName}.sha256`) {
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      res.end(shaText);
      return;
    }
    res.writeHead(404);
    res.end('not found');
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({ server, port, counts, baseUrl: `http://127.0.0.1:${port}` });
    });
  });
}

function closeServer(server) {
  return new Promise((resolve) => server.close(resolve));
}

function lockFileFor(homeDir, version) {
  return path.join(homeDir, '.agentworth', 'bin', `v${version}.lock`);
}

function cacheDirFor(homeDir, version) {
  return path.join(homeDir, '.agentworth', 'bin', `v${version}`);
}

describe('second review: lock race, tmp-dir leak, stall timeout', () => {
  describe('A. two waiters racing the same dead lock', () => {
    it('readLockPid reflects only the true final winner after a W1-then-W2 takeover sequence', () => {
      // Runs the exact sequence from the finding, step by step, directly against the lock
      // file -- no async scheduling to get lucky or unlucky with.
      const dir = tmpDir('race-a-primitive-');
      const lockFile = path.join(dir, 'v1.0.0.lock');

      // Both W1 and W2 have already independently decided a prior dead-pid lock is stale and
      // unlinked it (or found it already gone) -- this test picks up right at the `wx` race.
      const fakeW1Pid = 111111;
      const fakeW2Pid = 222222;

      // W1 wins the open, writes, closes.
      let fd = fs.openSync(lockFile, 'wx');
      fs.writeSync(fd, String(fakeW1Pid));
      fs.closeSync(fd);

      // Before W1 gets to its confirm-read, W2 -- still acting on the *original* staleness
      // read -- unlinks what is now W1's fresh lock and creates its own.
      fs.unlinkSync(lockFile);
      fd = fs.openSync(lockFile, 'wx');
      fs.writeSync(fd, String(fakeW2Pid));
      fs.closeSync(fd);

      // W1's confirm-read must now see W2, not itself -- the fix's whole point.
      assert.equal(readLockPid(lockFile), fakeW2Pid);
      assert.notEqual(readLockPid(lockFile), fakeW1Pid);
    });

    it('downloadAndExtractBinary: losing the confirm-read race falls back to waiting and returns the real holder\'s result', async () => {
      const workDir = tmpDir('race-a-e2e-');
      const version = '9.6.1';
      const { archiveName, archiveBuffer, sha256Text, sha256 } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const lockFile = lockFileFor(homeDir, version);
      const cacheDir = cacheDirFor(homeDir, version);
      fs.mkdirSync(path.dirname(lockFile), { recursive: true });
      // A dead lock, so the very first loop iteration attempts a takeover.
      fs.writeFileSync(lockFile, String(DEAD_PID));

      let hookRan = false;
      try {
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
          // Fires right after this call writes+closes its own fresh lock, before its
          // confirm-read -- exactly the window finding A identified. Simulates a second
          // acquirer stealing the lock and finishing the whole install before we get to
          // read it back.
          __testAfterLockWrite: () => {
            if (hookRan) return;
            hookRan = true;
            fs.unlinkSync(lockFile);
            const fd = fs.openSync(lockFile, 'wx');
            fs.writeSync(fd, String(process.pid)); // "the other holder" -- alive, it's us
            fs.closeSync(fd);
            installWinnerSync(cacheDir, archiveBuffer, sha256);
            fs.unlinkSync(lockFile); // the other holder releases when done
          },
        });

        assert.equal(hookRan, true, 'the injected race window must actually have fired');
        assert.equal(result, path.join(cacheDir, 'agentworth'));
        assert.ok(fs.existsSync(result));
        // This call never became a second holder: it never touched the network itself.
        assert.equal(fixture.counts[archiveName], undefined, JSON.stringify(fixture.counts));
        const marker = fs.readFileSync(path.join(cacheDir, '.installed'), 'utf8').trim();
        assert.equal(marker, sha256);
      } finally {
        await closeServer(fixture.server);
      }
    });

    it('the finally block never unlinks a lock some other process has since taken over', async () => {
      const workDir = tmpDir('race-a-finally-');
      const version = '9.6.2';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const lockFile = lockFileFor(homeDir, version);
      const otherPid = 424242;
      let hookRan = false;

      try {
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
          // Fires right before the real holder's own finally-unlink -- simulates the lock
          // having aged out and been taken over by someone else in the narrow window before
          // release (the other half of finding A).
          __testBeforeUnlock: () => {
            if (hookRan) return;
            hookRan = true;
            fs.writeFileSync(lockFile, String(otherPid));
          },
        });

        assert.equal(hookRan, true);
        assert.ok(fs.existsSync(result), 'the real holder must still have finished its own install');
        const lockContent = fs.readFileSync(lockFile, 'utf8').trim();
        assert.equal(lockContent, String(otherPid), 'finally must not remove a lock it no longer owns');
      } finally {
        try {
          fs.unlinkSync(lockFile);
        } catch {}
        await closeServer(fixture.server);
      }
    });
  });

  describe('B. a dead holder\'s leftover tmp extract dir is swept, a live one is left alone', () => {
    it('sweeps a dead-pid .tmp- dir on the next install; leaves a live-pid one untouched', async () => {
      const workDir = tmpDir('leak-b-');
      const version = '9.6.3';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const binDir = path.join(homeDir, '.agentworth', 'bin');
      fs.mkdirSync(binDir, { recursive: true });

      const deadTmp = path.join(binDir, `.v${version}.tmp-${DEAD_PID}`);
      fs.mkdirSync(deadTmp, { recursive: true });
      fs.writeFileSync(path.join(deadTmp, 'leftover.txt'), 'a killed holder never cleaned this up');

      // A genuinely separate, genuinely alive process, so its pid is real and distinct from
      // this test's own pid -- proving the sweep discriminates by liveness, not just identity.
      const liveChild = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)']);
      await new Promise((resolve, reject) => {
        liveChild.once('spawn', resolve);
        liveChild.once('error', reject);
      });
      const liveTmp = path.join(binDir, `.v${version}.tmp-${liveChild.pid}`);
      fs.mkdirSync(liveTmp, { recursive: true });
      fs.writeFileSync(path.join(liveTmp, 'inflight.txt'), 'a real concurrent installer, still working');

      try {
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });

        assert.ok(fs.existsSync(result));
        assert.equal(fs.existsSync(deadTmp), false, 'the dead-pid tmp dir must be swept');
        assert.equal(fs.existsSync(liveTmp), true, 'the live-pid tmp dir must be left alone');
      } finally {
        liveChild.kill();
        await closeServer(fixture.server);
      }
    });
  });

  describe('C. a stalled download times out instead of holding the lock for 15 minutes', () => {
    it('downloadFile rejects a connection that stalls after headers, within the given timeout', async () => {
      const server = http.createServer((req, res) => {
        res.writeHead(200, { 'Content-Length': '1000' });
        // Deliberately never write a body or call res.end() -- an idle, stalled connection.
      });
      await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
      const { port } = server.address();

      try {
        const dest = path.join(tmpDir('stall-download-'), 'out.bin');
        const started = Date.now();
        await assert.rejects(downloadFile(`http://127.0.0.1:${port}/x`, dest, 5, null, 300), /stalled|timed out/i);
        const elapsedMs = Date.now() - started;
        assert.ok(elapsedMs < 2000, `took ${elapsedMs}ms, expected the ~300ms timeout to fire`);
      } finally {
        await new Promise((resolve) => server.close(resolve));
      }
    });

    it('downloadAndExtractBinary releases its lock after a stall times out, instead of holding it', async () => {
      const workDir = tmpDir('stall-e2e-');
      const version = '9.6.4';
      const archiveName = `agentworth-v${version}-${TEST_TRIPLE}.tar.gz`;
      const server = http.createServer((req, res) => {
        const name = path.basename((req.url || '').split('?')[0]);
        if (name === `${archiveName}.sha256`) {
          res.writeHead(200, { 'Content-Type': 'text/plain' });
          res.end(`${'a'.repeat(64)}  ${archiveName}\n`);
          return;
        }
        res.writeHead(200, { 'Content-Length': '1000' });
        // Never end() -- the archive route stalls forever, as if the connection hung.
      });
      await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
      const { port } = server.address();
      const homeDir = path.join(workDir, 'home');
      const lockFile = lockFileFor(homeDir, version);

      try {
        const started = Date.now();
        await assert.rejects(
          downloadAndExtractBinary({
            platform: TEST_PLATFORM,
            arch: TEST_ARCH,
            version,
            homeDir,
            silent: true,
            releaseBaseUrl: `http://127.0.0.1:${port}`,
            downloadTimeoutMs: 300,
          }),
        );
        const elapsedMs = Date.now() - started;
        // One retry after the first stall, so roughly 2x the per-attempt timeout -- still
        // nowhere near the old 15-minute staleness window.
        assert.ok(elapsedMs < 5000, `took ${elapsedMs}ms`);
        assert.equal(fs.existsSync(lockFile), false, 'the lock must be released after the timeout failure');
      } finally {
        await new Promise((resolve) => server.close(resolve));
      }
    });
  });
});
