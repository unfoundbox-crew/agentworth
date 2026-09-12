import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';

import { downloadAndExtractBinary, downloadFile, run } from '../lib/resolver.js';

// These tests exercise the real network path (a local node:http server, no mocking) against
// #173: a slow/truncated download left a partial, unsigned binary in the cache dir that every
// later invocation ran and died with exit 137, and concurrent first runs (one per open agent
// session) raced into the same version directory with no lock at all.
//
// Every test pins platform/arch to 'linux'/'x64' regardless of the host this suite runs on
// (a Mac while writing it, ubuntu-latest in CI): downloadAndExtractBinary never actually
// executes the extracted files, it only checks they exist and are chmod +x, so the choice of
// target triple only has to be consistent between the built fixture archive and the call --
// it does not need to match the real OS running the test.
const TEST_PLATFORM = 'linux';
const TEST_ARCH = 'x64';
const TEST_TRIPLE = 'x86_64-unknown-linux-gnu'; // must match getTargetTriple(linux, x64)

function tmpDir(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

/** Builds a real tar.gz containing the three stub native binaries, plus its true sha256. */
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
  return { archiveName, archiveBuffer, sha256Text: `${sha256}  ${archiveName}\n` };
}

/**
 * Serves an archive + its `.sha256` sidecar over plain HTTP on 127.0.0.1, routed by
 * basename (the launcher's real URLs carry a `/v<version>/` prefix; this double only cares
 * what file is being asked for, which is enough to stand in for the release CDN).
 *
 * `archiveBytes` and `shaText` can be swapped per-request via the returned `state` object,
 * so a single server can simulate "first attempt corrupt, retry also corrupt" or similar.
 */
function startFixtureServer({ archiveName, archiveBytes, shaText }) {
  const counts = {};
  const state = { archiveBytes, shaText };
  const server = http.createServer((req, res) => {
    const name = path.basename((req.url || '').split('?')[0]);
    counts[name] = (counts[name] || 0) + 1;
    if (name === archiveName) {
      res.writeHead(200, { 'Content-Length': String(state.archiveBytes.length) });
      res.end(state.archiveBytes);
      return;
    }
    if (name === `${archiveName}.sha256`) {
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      res.end(state.shaText);
      return;
    }
    res.writeHead(404);
    res.end('not found');
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({ server, port, counts, state, baseUrl: `http://127.0.0.1:${port}` });
    });
  });
}

function closeServer(server) {
  return new Promise((resolve) => server.close(resolve));
}

describe('npm-wrapper / launcher download hardening (#173)', () => {
  describe('1. checksum before extract', () => {
    it('rejects a truncated download, retries once, then throws naming the archive and both hash prefixes -- no binary left behind', async () => {
      const workDir = tmpDir('agentworth-checksum-');
      const version = '9.9.1';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);

      // Simulate exactly what #173 saw: the server hands back fewer bytes than the real
      // archive, but the sidecar still names the *correct* full-file hash -- so the
      // downloaded bytes can never match it.
      const truncated = archiveBuffer.subarray(0, Math.floor(archiveBuffer.length / 2));
      const fixture = await startFixtureServer({ archiveName, archiveBytes: truncated, shaText: sha256Text });

      const homeDir = path.join(workDir, 'home');
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
            assert.ok(err.message.includes(archiveName), err.message);
            const fullHash = sha256Text.split(/\s+/)[0];
            const truncHash = crypto.createHash('sha256').update(truncated).digest('hex');
            assert.ok(err.message.includes(fullHash.slice(0, 12)), err.message);
            assert.ok(err.message.includes(truncHash.slice(0, 12)), err.message);
            assert.ok(/rm -rf/.test(err.message), err.message);
            return true;
          },
        );

        // Retried once: two attempts at the archive route, not one and not unbounded.
        assert.equal(fixture.counts[archiveName], 2, JSON.stringify(fixture.counts));

        // No binary, no leftover .part file, anywhere under the cache dir.
        const cacheDir = path.join(homeDir, '.agentworth', 'bin', `v${version}`);
        if (fs.existsSync(cacheDir)) {
          const leftover = fs.readdirSync(cacheDir);
          assert.deepEqual(leftover, [], `cache dir should be empty, found: ${leftover}`);
        }
      } finally {
        await closeServer(fixture.server);
      }
    });

    it('downloadFile mocking point still works standalone (real HTTP, no checksum layer)', async () => {
      // Sanity check that the plain downloader this test suite mocks around in the rest of
      // launcher.test.js also works end-to-end against a real socket, per the brief's note
      // to exercise the real downloadFile against localhost.
      const workDir = tmpDir('agentworth-plain-download-');
      const bytes = Buffer.from('hello from the fixture server');
      const fixture = await startFixtureServer({ archiveName: 'plain.bin', archiveBytes: bytes, shaText: '' });
      try {
        const dest = path.join(workDir, 'out.bin');
        await downloadFile(`${fixture.baseUrl}/plain.bin`, dest);
        assert.deepEqual(fs.readFileSync(dest), bytes);
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('2. atomic install', () => {
    it('after success, all three binaries exist and are executable', async () => {
      const workDir = tmpDir('agentworth-atomic-ok-');
      const version = '9.9.2';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      fs.mkdirSync(homeDir, { recursive: true });

      try {
        const result = await downloadAndExtractBinary({
          platform: TEST_PLATFORM,
          arch: TEST_ARCH,
          version,
          homeDir,
          silent: true,
          releaseBaseUrl: fixture.baseUrl,
        });

        const cacheDir = path.join(homeDir, '.agentworth', 'bin', `v${version}`);
        assert.equal(result, path.join(cacheDir, 'agentworth'));
        for (const name of ['agentworth', 'archie', 'agwt']) {
          const p = path.join(cacheDir, name);
          assert.ok(fs.existsSync(p), `${p} missing`);
          const mode = fs.statSync(p).mode;
          assert.ok((mode & 0o111) !== 0, `${p} not executable`);
        }

        // No temp install dir left behind.
        const binDir = path.dirname(cacheDir);
        const leftoverTmp = fs.readdirSync(binDir).filter((n) => n.startsWith('.v'));
        assert.deepEqual(leftoverTmp, []);
      } finally {
        await closeServer(fixture.server);
      }
    });

    it('after a failed extract, nothing exists at the final path and no .tmp- dirs remain', async () => {
      const workDir = tmpDir('agentworth-atomic-fail-');
      const version = '9.9.3';
      const archiveName = `agentworth-v${version}-${TEST_TRIPLE}.tar.gz`;
      // Checksum-valid but not actually a gzip/tar file -- passes verification, fails extraction.
      const archiveBuffer = Buffer.from('not a real gzip archive, just some bytes');
      const sha256 = crypto.createHash('sha256').update(archiveBuffer).digest('hex');
      const sha256Text = `${sha256}  ${archiveName}\n`;
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
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
          /Failed to extract/,
        );

        const cacheDir = path.join(homeDir, '.agentworth', 'bin', `v${version}`);
        assert.equal(fs.existsSync(path.join(cacheDir, 'agentworth')), false);
        const binDir = path.dirname(cacheDir);
        const leftoverTmp = fs.existsSync(binDir) ? fs.readdirSync(binDir).filter((n) => n.startsWith('.v')) : [];
        assert.deepEqual(leftoverTmp, []);
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('3. one download per version', () => {
    it('8 concurrent downloadAndExtractBinary calls make exactly 1 request for the archive and all resolve', async () => {
      const workDir = tmpDir('agentworth-lock-');
      const version = '9.9.4';
      const { archiveName, archiveBuffer, sha256Text } = buildGoodArchive(workDir, version);
      const fixture = await startFixtureServer({ archiveName, archiveBytes: archiveBuffer, shaText: sha256Text });
      const homeDir = path.join(workDir, 'home');
      fs.mkdirSync(homeDir, { recursive: true });

      try {
        const calls = Array.from({ length: 8 }, () =>
          downloadAndExtractBinary({
            platform: TEST_PLATFORM,
            arch: TEST_ARCH,
            version,
            homeDir,
            silent: true,
            releaseBaseUrl: fixture.baseUrl,
          }),
        );
        const results = await Promise.all(calls);

        assert.equal(fixture.counts[archiveName], 1, JSON.stringify(fixture.counts));
        assert.equal(results.length, 8);
        for (const r of results) {
          assert.equal(fs.existsSync(r), true);
          const mode = fs.statSync(r).mode;
          assert.ok((mode & 0o111) !== 0);
        }
      } finally {
        await closeServer(fixture.server);
      }
    });
  });

  describe('4. hooks never download', () => {
    it('run(["hook", ...]) with an empty cache and no PATH binary returns 0 quickly and starts no download', async () => {
      const workDir = tmpDir('agentworth-hook-');
      // A server that would fail the test if it receives any request at all.
      let requestCount = 0;
      const server = http.createServer((req, res) => {
        requestCount += 1;
        res.writeHead(200);
        res.end('should not have been called');
      });
      await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
      const { port } = server.address();

      const emptyDir = path.join(workDir, 'empty');
      fs.mkdirSync(emptyDir, { recursive: true });

      const originalWrite = process.stdout.write;
      let stdout = '';
      process.stdout.write = (chunk, ...rest) => {
        stdout += chunk;
        return true;
      };

      let exitCode;
      const started = Date.now();
      try {
        exitCode = run(['hook', 'print', 'claude'], {
          cwd: emptyDir,
          env: { PATH: '', AGENTWORTH_RELEASE_BASE_URL: `http://127.0.0.1:${port}` },
          baseDir: emptyDir,
          homeDir: emptyDir,
        });
      } finally {
        process.stdout.write = originalWrite;
        await new Promise((resolve) => server.close(resolve));
      }
      const elapsedMs = Date.now() - started;

      assert.equal(exitCode, 0);
      assert.equal(stdout, '');
      assert.equal(requestCount, 0, 'hook invocation must never touch the network');
      assert.ok(elapsedMs < 2000, `hook path took ${elapsedMs}ms, expected a fast fail`);
    });
  });
});
