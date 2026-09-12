import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';

import { downloadAndExtractBinary, parseSha256Line } from '../lib/resolver.js';

// Six review findings on the #173 fix (PR #174), addressed here in order. Same approach as
// launcher-download.test.js: a real local node:http server, no mocking of the resolver's own
// download functions.
const TEST_PLATFORM = 'linux';
const TEST_ARCH = 'x64';
const TEST_TRIPLE = 'x86_64-unknown-linux-gnu';

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

function writeGarbageBinary(filePath) {
  fs.writeFileSync(filePath, '#!/bin/sh\necho "GARBAGE, NOT THE REAL THING"\nexit 137\n', { mode: 0o755 });
}

describe('#174 review findings', () => {
  describe('1. a truncated-but-executable cache is never served, and gets repaired', () => {
    it('three executable garbage files with no marker trigger exactly one download and end with the marker + real binaries', async () => {
      const workDir = tmpDir('finding1-');
      const version = '9.8.1';
      const { archiveName, archiveBuffer, sha256Text, sha256 } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const cacheDir = path.join(homeDir, '.agentworth', 'bin', `v${version}`);
      fs.mkdirSync(cacheDir, { recursive: true });
      // This is exactly what #173 left behind: present, correctly-permissioned, no marker --
      // and, per the real incident, could even be the same byte size as a good binary.
      for (const name of ['agentworth', 'archie', 'agwt']) {
        writeGarbageBinary(path.join(cacheDir, name));
      }
      assert.equal(fs.existsSync(path.join(cacheDir, '.installed')), false);

      try {
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });

        assert.equal(fixture.counts[archiveName], 1, JSON.stringify(fixture.counts));
        assert.equal(result, path.join(cacheDir, 'agentworth'));

        const marker = fs.readFileSync(path.join(cacheDir, '.installed'), 'utf8').trim();
        assert.equal(marker, sha256);

        // The garbage content is gone -- these are the real extracted binaries now.
        for (const name of ['agentworth', 'archie', 'agwt']) {
          const content = fs.readFileSync(path.join(cacheDir, name), 'utf8');
          assert.ok(content.includes(`agentworth ${version}`), content);
          assert.ok(!content.includes('GARBAGE'), content);
        }

        // cacheDir holds only the complete install: no leftover archive or .part (finding 6).
        const entries = fs.readdirSync(cacheDir).sort();
        assert.deepEqual(entries, ['.installed', 'agentworth', 'agwt', 'archie']);
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('2. lock staleness is pid-aware, and the holder keeps its lock alive', () => {
    it('a lock file holding a dead pid is taken over within roughly one poll interval', async () => {
      const workDir = tmpDir('finding2-dead-');
      const version = '9.8.2';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const binDir = path.join(homeDir, '.agentworth', 'bin');
      fs.mkdirSync(binDir, { recursive: true });
      const lockFile = path.join(binDir, `v${version}.lock`);
      // An unassigned-looking pid: process.kill(pid, 0) must throw ESRCH for this to work. A
      // fresh mtime proves staleness here is decided by pid liveness, not by age.
      fs.writeFileSync(lockFile, '2147483647');

      try {
        const started = Date.now();
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });
        const elapsedMs = Date.now() - started;

        assert.ok(fs.existsSync(result));
        // Nowhere near the old 15-minute staleness window -- a handful of poll intervals at
        // most, dominated by the (tiny, local) download itself.
        assert.ok(elapsedMs < 5000, `took ${elapsedMs}ms, expected a fast takeover`);
      } finally {
        await closeServer(fixture.server);
      }
    });

    it('a lock file holding this process\'s own live pid and a fresh mtime is respected until released', async () => {
      const workDir = tmpDir('finding2-live-');
      const version = '9.8.3';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const binDir = path.join(homeDir, '.agentworth', 'bin');
      fs.mkdirSync(binDir, { recursive: true });
      const lockFile = path.join(binDir, `v${version}.lock`);
      // Our own test process's pid is guaranteed alive for the duration of this test.
      fs.writeFileSync(lockFile, String(process.pid));

      try {
        const promise = downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });

        let settled = false;
        promise.then(
          () => (settled = true),
          () => (settled = true),
        );

        await new Promise((resolve) => setTimeout(resolve, 900));
        assert.equal(settled, false, 'must still be waiting on the live-pid lock after 900ms');
        assert.equal(fixture.counts[archiveName], undefined, 'must not have downloaded while the lock is held');

        // Release the lock as the "other holder" finishing would -- the waiter must then
        // proceed and finish normally instead of being stuck.
        fs.unlinkSync(lockFile);
        const result = await promise;
        assert.ok(fs.existsSync(result));
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('3. only the lock holder ever touches the final dir, rechecked after acquiring the lock', () => {
    it('a dead-pid lock plus an already-complete install never triggers a download or a rewrite of the final dir', async () => {
      const workDir = tmpDir('finding3-');
      const version = '9.8.4';
      const { archiveName, archiveBuffer, sha256Text, sha256 } = buildGoodArchive(workDir, version);
      // A server that would fail the test if the archive is ever requested.
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      const cacheDir = path.join(homeDir, '.agentworth', 'bin', `v${version}`);
      const binDir = path.dirname(cacheDir);
      fs.mkdirSync(cacheDir, { recursive: true });

      // Simulate "another process already finished the install" -- real binaries, real marker.
      const sentinel = 'SENTINEL-ALREADY-INSTALLED';
      for (const name of ['agentworth', 'archie', 'agwt']) {
        fs.writeFileSync(path.join(cacheDir, name), `#!/bin/sh\necho "${sentinel}"\nexit 0\n`, { mode: 0o755 });
      }
      fs.writeFileSync(path.join(cacheDir, '.installed'), `${sha256}\n`);

      // ...but leave behind a dead-pid lock, as if that finishing process crashed right after
      // installing instead of cleanly unlinking its lock.
      fs.mkdirSync(binDir, { recursive: true });
      const lockFile = path.join(binDir, `v${version}.lock`);
      fs.writeFileSync(lockFile, '2147483647');

      try {
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });

        assert.equal(fixture.counts[archiveName], undefined, 'a complete install must never be re-downloaded');
        const content = fs.readFileSync(result, 'utf8');
        assert.ok(content.includes(sentinel), 'the pre-existing complete install must be left untouched');
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('4. error text never carries the home directory', () => {
    it('a failed extraction reports no /Users/ or /home/ substring, nor the actual homeDir path', async () => {
      const workDir = tmpDir('finding4-');
      const version = '9.8.5';
      const archiveName = `agentworth-v${version}-${TEST_TRIPLE}.tar.gz`;
      // Checksum-valid but not a real gzip/tar -- passes verification, fails extraction, so
      // the thrown message is built from tar's own error text (which embeds the full paths).
      const archiveBuffer = Buffer.from('not a real gzip archive, just some bytes, for finding 4');
      const sha256 = crypto.createHash('sha256').update(archiveBuffer).digest('hex');
      const sha256Text = `${sha256}  ${archiveName}\n`;
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });

      // A homeDir that deliberately looks like a real machine's, regardless of where this
      // suite's own OS temp directory happens to live (CI's /tmp doesn't naturally contain
      // "/Users/", so this constructs the case on purpose rather than hoping for it).
      const homeDir = path.join(workDir, 'Users', 'reviewer');
      fs.mkdirSync(homeDir, { recursive: true });

      try {
        await assert.rejects(
          downloadAndExtractBinary({
            platform: TEST_PLATFORM,
            arch: TEST_ARCH,
            version,
            homeDir,
            silent: true,
            releaseBaseUrl: fixture.baseUrl,
          }),
          (err) => {
            assert.ok(err.message.includes('Failed to extract'), err.message);
            assert.ok(!err.message.includes(homeDir), err.message);
            assert.ok(!err.message.includes('/Users/'), err.message);
            assert.ok(!err.message.includes('/home/'), err.message);
            return true;
          },
        );
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('5. the checksum sidecar must name the archive it describes', () => {
    it('parseSha256Line throws when the sidecar names a different file', () => {
      const hash = 'a'.repeat(64);
      assert.throws(
        () => parseSha256Line(`${hash}  some-other-archive.tar.gz`, 'agentworth-v1.0.0-x86_64-unknown-linux-gnu.tar.gz'),
        /expected/,
      );
    });

    it('parseSha256Line still returns the hash when the names match (including a leading path)', () => {
      const hash = 'b'.repeat(64);
      const archiveName = 'agentworth-v1.0.0-x86_64-unknown-linux-gnu.tar.gz';
      assert.equal(parseSha256Line(`${hash}  ${archiveName}`, archiveName), hash);
      assert.equal(parseSha256Line(`${hash}  ./dist/${archiveName}`, archiveName), hash);
      assert.equal(parseSha256Line(`${hash} *${archiveName}`, archiveName), hash);
    });

    it('a real download fails checksum verification end-to-end when the sidecar names the wrong archive', async () => {
      const workDir = tmpDir('finding5-e2e-');
      const version = '9.8.6';
      const { archiveName, archiveBuffer } = buildGoodArchive(workDir, version);
      const wrongName = `agentworth-v${version}-x86_64-apple-darwin.tar.gz`;
      const sha256 = crypto.createHash('sha256').update(archiveBuffer).digest('hex');
      // Hash is correct for the bytes served, but the sidecar names a different archive.
      const fixture = await startFixtureServer({
        archiveName,
        archiveBytes: archiveBuffer,
        shaText: `${sha256}  ${wrongName}\n`,
      });
      const homeDir = path.join(workDir, 'home');

      try {
        await assert.rejects(
          downloadAndExtractBinary({
            platform: TEST_PLATFORM,
            arch: TEST_ARCH,
            version,
            homeDir,
            silent: true,
            releaseBaseUrl: fixture.baseUrl,
          }),
          /expected/,
        );
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('6. the .part file and archive never land in cacheDir', () => {
    it('cacheDir contains only the completed install after a normal download, never an archive or .part', async () => {
      const workDir = tmpDir('finding6-');
      const version = '9.8.7';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');

      try {
        await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });

        const cacheDir = path.join(homeDir, '.agentworth', 'bin', `v${version}`);
        const entries = fs.readdirSync(cacheDir).sort();
        assert.deepEqual(entries, ['.installed', 'agentworth', 'agwt', 'archie']);
        for (const entry of entries) {
          assert.ok(!entry.endsWith('.part'), entry);
          assert.ok(!entry.endsWith('.tar.gz'), entry);
        }

        // And no leftover .tmp- install directory beside it either.
        const binDir = path.dirname(cacheDir);
        const leftoverTmp = fs.readdirSync(binDir).filter((n) => n.startsWith('.v'));
        assert.deepEqual(leftoverTmp, []);
      } finally {
        await closeServer(fixture.server);
      }
    });
  });
});
