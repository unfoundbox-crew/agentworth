import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

import {
  getPlatformKey,
  getTargetTriple,
  getBinaryName,
  resolveArguments,
  isExecutable,
  findCargoTargetBinary,
  findPathBinary,
  resolveBinary,
  formatMissingBinaryMessage,
  buildChildEnv,
  brandLine,
  downloadLine,
  readBinaryVersion,
  staleBinaryNotice,
  run,
} from '../lib/resolver.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// getPackageVersion() falls back to '0.1.3' when baseDir has no ../package.json to read,
// which is always true for these throwaway temp dirs -- so a mock binary answering with
// this exact version is indistinguishable from "the real deal" to pathBinaryMatchesVersion.
const FALLBACK_VERSION = '0.1.3';

function writeMockBinary(filePath, version = FALLBACK_VERSION) {
  fs.writeFileSync(
    filePath,
    `#!/bin/sh\nif [ "$1" = "--version" ]; then echo "agentworth ${version}"; fi\nexit 0\n`,
    { mode: 0o755 },
  );
}

describe('npm-wrapper / launcher', () => {
  describe('platform & binary naming', () => {
    it('detects OS and architecture platform keys correctly', () => {
      assert.equal(getPlatformKey('darwin', 'arm64'), 'darwin-arm64');
      assert.equal(getPlatformKey('darwin', 'x64'), 'darwin-x64');
      assert.equal(getPlatformKey('linux', 'x64'), 'linux-x64');
      assert.equal(getPlatformKey('linux', 'arm64'), 'linux-arm64');
      assert.equal(getPlatformKey('win32', 'x64'), 'win32-x64');
      assert.equal(getPlatformKey('win32', 'arm64'), 'win32-arm64');
    });

    it('maps platforms to release target triples', () => {
      assert.equal(getTargetTriple('darwin', 'arm64'), 'aarch64-apple-darwin');
      assert.equal(getTargetTriple('darwin', 'x64'), 'x86_64-apple-darwin');
      assert.equal(getTargetTriple('linux', 'x64'), 'x86_64-unknown-linux-gnu');
      assert.equal(getTargetTriple('linux', 'arm64'), 'aarch64-unknown-linux-gnu');
      // Windows dropped 2026-09-02 -- no target triple, so downloadAndExtractBinary's
      // existing "Unsupported platform/architecture" error covers it instead of
      // attempting a download that would 404 against the (no longer built) release asset.
      assert.equal(getTargetTriple('win32', 'x64'), null);
      assert.equal(getTargetTriple('unknown', 'arch'), null);
    });

    it('determines native binary filename by platform', () => {
      assert.equal(getBinaryName('darwin'), 'agentworth');
      assert.equal(getBinaryName('linux'), 'agentworth');
      assert.equal(getBinaryName('win32'), 'agentworth.exe');
    });

    it('resolves a short-alias binary name only when explicitly invoked as one', () => {
      // The release tarball ships all three native binaries (apps/cli/Cargo.toml's three
      // [[bin]] targets); each npm alias must resolve to its own extracted file rather
      // than silently falling back to 'agentworth'.
      assert.equal(getBinaryName('darwin', 'archie'), 'archie');
      assert.equal(getBinaryName('linux', 'archie'), 'archie');
      assert.equal(getBinaryName('win32', 'archie'), 'archie.exe');
      assert.equal(getBinaryName('darwin', 'agwt'), 'agwt');
      assert.equal(getBinaryName('linux', 'agwt'), 'agwt');
      assert.equal(getBinaryName('win32', 'agwt'), 'agwt.exe');
      // Anything else -- undefined, 'agentworth', garbage -- still gets the primary binary.
      assert.equal(getBinaryName('darwin', undefined), 'agentworth');
      assert.equal(getBinaryName('darwin', 'agentworth'), 'agentworth');
      assert.equal(getBinaryName('darwin', 'something-else'), 'agentworth');
    });
  });

  describe('argument parsing & defaults', () => {
    it('defaults to ["serve", "--open"] when no arguments are provided', () => {
      assert.deepEqual(resolveArguments([]), ['serve', '--open']);
      assert.deepEqual(resolveArguments(undefined), ['serve', '--open']);
    });

    it('preserves and forwards subcommands and flags transparently', () => {
      assert.deepEqual(resolveArguments(['scan']), ['scan']);
      assert.deepEqual(resolveArguments(['scan', '--force', '--json']), ['scan', '--force', '--json']);
      assert.deepEqual(resolveArguments(['stats']), ['stats']);
      assert.deepEqual(resolveArguments(['traces', '--limit', '50']), ['traces', '--limit', '50']);
      assert.deepEqual(resolveArguments(['inspect', 'session-abc-123']), ['inspect', 'session-abc-123']);
      assert.deepEqual(resolveArguments(['export', 'session-123', '--redact', '--format', 'atif']), [
        'export',
        'session-123',
        '--redact',
        '--format',
        'atif',
      ]);
      assert.deepEqual(resolveArguments(['--version']), ['--version']);
      assert.deepEqual(resolveArguments(['-v', 'stats']), ['-v', 'stats']);
    });
  });

  describe('launcher markers for the child process (agentworth version/update)', () => {
    it('sets AGENTWORTH_LAUNCHER_ACTIVE and threads the npm version through', () => {
      const childEnv = buildChildEnv({ FOO: 'bar' }, '0.1.9');
      assert.equal(childEnv.AGENTWORTH_LAUNCHER_ACTIVE, '1');
      assert.equal(childEnv.AGENTWORTH_NPM_VERSION, '0.1.9');
      assert.equal(childEnv.FOO, 'bar');
    });

    it('does not mutate the base environment object it was given', () => {
      const base = { FOO: 'bar' };
      buildChildEnv(base, '0.1.9');
      assert.deepEqual(base, { FOO: 'bar' });
    });

    it('overrides a pre-existing AGENTWORTH_LAUNCHER_ACTIVE from the base env', () => {
      // Defense in depth: even if something upstream already set this (e.g. a nested
      // launcher invocation), the real spawn must always mark itself active.
      const childEnv = buildChildEnv({ AGENTWORTH_LAUNCHER_ACTIVE: '0' }, '0.1.9');
      assert.equal(childEnv.AGENTWORTH_LAUNCHER_ACTIVE, '1');
    });
  });

  describe('binary resolution & search order', () => {
    let tempDir;
    let mockBin;

    before(() => {
      tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'agentworth-test-'));
      mockBin = path.join(tempDir, 'agentworth');
      fs.writeFileSync(mockBin, '#!/bin/sh\nexit 0\n', { mode: 0o755 });
    });

    after(() => {
      if (tempDir && fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    });

    it('resolves explicit AGENTWORTH_BIN environment variable', () => {
      const res = resolveBinary({
        cwd: tempDir,
        env: { AGENTWORTH_BIN: mockBin },
        baseDir: tempDir,
        homeDir: tempDir,
      });

      assert.equal(res.found, true);
      assert.equal(res.path, mockBin);
      assert.equal(res.source, 'env:AGENTWORTH_BIN');
    });

    it('resolves local cargo target directory', () => {
      const workspaceDir = path.join(tempDir, 'workspace');
      const targetDebug = path.join(workspaceDir, 'target', 'debug');
      fs.mkdirSync(targetDebug, { recursive: true });
      const targetBin = path.join(targetDebug, 'agentworth');
      writeMockBinary(targetBin);

      const subDir = path.join(workspaceDir, 'nested', 'subproject');
      fs.mkdirSync(subDir, { recursive: true });

      const res = resolveBinary({
        cwd: subDir,
        env: { PATH: '' },
        baseDir: subDir,
        homeDir: tempDir,
      });

      assert.equal(res.found, true);
      assert.equal(res.path, targetBin);
      assert.ok(res.source.startsWith('cargo-target'));
    });

    it('rejects a cargo-target binary whose version does not match', () => {
      const workspaceDir = path.join(tempDir, 'workspace-stale');
      const targetRelease = path.join(workspaceDir, 'target', 'release');
      fs.mkdirSync(targetRelease, { recursive: true });
      const staleBin = path.join(targetRelease, 'agentworth');
      writeMockBinary(staleBin, '0.1.1');

      const res = resolveBinary({
        cwd: workspaceDir,
        env: { PATH: '' },
        baseDir: workspaceDir,
        homeDir: workspaceDir,
      });

      // A stale build from a much earlier version must not silently shadow the launcher's
      // own release forever -- this is the exact bug (a target/release/agentworth left over
      // from an early build kept answering for every later version on the real machine).
      assert.notEqual(res.path, staleBin);
    });

    it('resolves from CARGO_TARGET_DIR environment variable', () => {
      const customTargetDir = path.join(tempDir, 'custom-target');
      const releaseDir = path.join(customTargetDir, 'release');
      fs.mkdirSync(releaseDir, { recursive: true });
      const customBin = path.join(releaseDir, 'agentworth');
      writeMockBinary(customBin);

      const isolatedDir = path.join(tempDir, 'isolated');
      fs.mkdirSync(isolatedDir, { recursive: true });

      const res = resolveBinary({
        cwd: isolatedDir,
        env: { CARGO_TARGET_DIR: customTargetDir, PATH: '' },
        baseDir: isolatedDir,
        homeDir: tempDir,
      });

      assert.equal(res.found, true);
      assert.equal(res.path, customBin);
      assert.equal(res.source, 'cargo-target-dir-release');
    });

    it('resolves from PATH environment variable', () => {
      const binDir = path.join(tempDir, 'custom-bin');
      fs.mkdirSync(binDir, { recursive: true });
      const pathBin = path.join(binDir, 'agentworth');
      writeMockBinary(pathBin);

      const isolatedDir = path.join(tempDir, 'isolated-path');
      fs.mkdirSync(isolatedDir, { recursive: true });

      const res = resolveBinary({
        cwd: isolatedDir,
        env: { PATH: binDir },
        baseDir: isolatedDir,
        homeDir: tempDir,
      });

      assert.equal(res.found, true);
      assert.equal(res.path, pathBin);
      assert.equal(res.source, 'path');
    });

    it('resolves the agwt binary specifically from a cache dir holding both extracted binaries', () => {
      // Mirrors the real extracted layout: `tar -xzf` drops both native binaries from
      // the tarball into the same versioned cache dir (see downloadAndExtractBinary).
      const binDir = path.join(tempDir, 'both-binaries');
      fs.mkdirSync(binDir, { recursive: true });
      writeMockBinary(path.join(binDir, 'agentworth'));
      const agwtBin = path.join(binDir, 'agwt');
      writeMockBinary(agwtBin);

      const isolatedDir = path.join(tempDir, 'isolated-agwt');
      fs.mkdirSync(isolatedDir, { recursive: true });

      const res = resolveBinary({
        cwd: isolatedDir,
        env: { PATH: binDir },
        baseDir: isolatedDir,
        homeDir: tempDir,
        invokedAs: 'agwt',
      });

      assert.equal(res.found, true);
      assert.equal(res.path, agwtBin);
    });

    it('resolves the archie binary specifically from a cache dir holding every extracted binary', () => {
      const binDir = path.join(tempDir, 'all-binaries');
      fs.mkdirSync(binDir, { recursive: true });
      writeMockBinary(path.join(binDir, 'agentworth'));
      writeMockBinary(path.join(binDir, 'agwt'));
      const archieBin = path.join(binDir, 'archie');
      writeMockBinary(archieBin);

      const isolatedDir = path.join(tempDir, 'isolated-archie');
      fs.mkdirSync(isolatedDir, { recursive: true });

      const res = resolveBinary({
        cwd: isolatedDir,
        env: { PATH: binDir },
        baseDir: isolatedDir,
        homeDir: tempDir,
        invokedAs: 'archie',
      });

      assert.equal(res.found, true);
      assert.equal(res.path, archieBin);
    });

    it('returns found: false when binary is not located', () => {
      const emptyDir = path.join(tempDir, 'empty-dir');
      fs.mkdirSync(emptyDir, { recursive: true });

      const res = resolveBinary({
        cwd: emptyDir,
        env: { PATH: '' },
        baseDir: emptyDir,
        homeDir: emptyDir,
      });

      assert.equal(res.found, false);
      assert.ok(res.error);
    });
  });

  describe('execution & fallback handling', () => {
    let tempDir;
    let successBin;
    let failingBin;

    before(() => {
      tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'agentworth-exec-test-'));
      successBin = path.join(tempDir, 'mock-agentworth-success');
      fs.writeFileSync(successBin, '#!/bin/sh\necho "MOCK_SUCCESS: $@"\nexit 0\n', { mode: 0o755 });

      failingBin = path.join(tempDir, 'mock-agentworth-fail');
      fs.writeFileSync(failingBin, '#!/bin/sh\nexit 42\n', { mode: 0o755 });
    });

    after(() => {
      if (tempDir && fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    });

    it('executes successfully and forwards exit code 0', () => {
      const exitCode = run(['stats'], {
        cwd: tempDir,
        env: { AGENTWORTH_BIN: successBin },
      });
      assert.equal(exitCode, 0);
    });

    it('forwards non-zero child process exit codes', () => {
      const exitCode = run(['traces'], {
        cwd: tempDir,
        env: { AGENTWORTH_BIN: failingBin },
      });
      assert.equal(exitCode, 42);
    });

    it('handles fallback gracefully when binary is missing and returns code 1', () => {
      const emptyDir = path.join(tempDir, 'empty');
      fs.mkdirSync(emptyDir, { recursive: true });

      const exitCode = run(['scan'], {
        cwd: emptyDir,
        env: { PATH: '' },
        baseDir: emptyDir,
        homeDir: emptyDir,
        autoDownload: false,
      });
      assert.equal(exitCode, 1);
    });

    it('formats missing binary error message with helpful installation hints', () => {
      // Brew was never a real fallback here (npm launcher, not the README's install
      // matrix) and #55 dropped it from the READMEs too -- this message's actual
      // fallbacks are the standalone install script, Cargo, npx, the GitHub release
      // download, and AGENTWORTH_BIN.
      const msg = formatMissingBinaryMessage('darwin-arm64');
      assert.ok(msg.includes('darwin-arm64'));
      assert.ok(msg.includes('curl -fsSL https://agentworth.dev/install.sh | sh'));
      assert.ok(msg.includes('cargo install agentworth-cli'));
      assert.ok(msg.includes('npx -y agentworth@latest'));
      assert.ok(msg.includes('AGENTWORTH_BIN'));
    });

    it('speaks in the brand line, with no emoji and no glyph outside the CLI set', () => {
      // docs/DESIGN.md's glyph boundary applies to the launcher too: it is the first
      // screen a `npx agentworth` user ever sees. The CLI enforces this with its own test
      // (apps/cli/src/ui/mod.rs); this is the same rule on the Node side.
      const sample = [
        formatMissingBinaryMessage('darwin-arm64'),
        brandLine('*', 'installed', 'archie in ~/.agentworth/bin/v0.1.16'),
        downloadLine(9_000_000, 23_300_000, { cols: 80 }),
        downloadLine(9_000_000, 23_300_000, { cols: 80, unicode: false }),
        downloadLine(0, 0, { cols: 80 }),
      ].join('\n');
      for (const ch of sample) {
        const cp = ch.codePointAt(0);
        const ok =
          cp < 128 ||
          (cp >= 0x2500 && cp <= 0x259f) ||
          ['●', '○', '·', '—', '→'].includes(ch);
        assert.ok(ok, `glyph U+${cp.toString(16).padStart(4, '0')} (${ch}) is outside the allowed set`);
      }
    });
  });

  describe('stale binary notice', () => {
    it('says so when the resolved binary is older than the package running it', () => {
      const line = staleBinaryNotice('0.1.11', '0.1.17');
      assert.ok(line, 'an older binary must produce a line');
      assert.ok(line.includes('0.1.11'), line);
      assert.ok(line.includes('0.1.17'), line);
      assert.ok(line.startsWith(' (-) archie  stale'), line);
    });

    it('stays quiet for an equal or newer binary', () => {
      assert.equal(staleBinaryNotice('0.1.17', '0.1.17'), null);
      // A local build ahead of the published package is not stale.
      assert.equal(staleBinaryNotice('0.2.0', '0.1.17'), null);
      assert.equal(staleBinaryNotice('1.0.0', '0.9.9'), null);
    });

    it('compares each segment numerically, not as text', () => {
      // '0.1.9' sorts after '0.1.17' as a string; it is older as a version.
      assert.ok(staleBinaryNotice('0.1.9', '0.1.17'));
      assert.equal(staleBinaryNotice('0.1.17', '0.1.9'), null);
    });

    it('never claims staleness on a version it could not parse', () => {
      assert.equal(staleBinaryNotice(null, '0.1.17'), null);
      assert.equal(staleBinaryNotice('not-a-version', '0.1.17'), null);
      assert.equal(staleBinaryNotice('0.1.11', 'nonsense'), null);
    });

    it('reads a real binary version and returns null when it cannot ask', () => {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aw-stale-'));
      try {
        const bin = path.join(dir, 'archie');
        writeMockBinary(bin, '0.1.11');
        assert.equal(readBinaryVersion(bin), '0.1.11');
        assert.equal(readBinaryVersion(path.join(dir, 'not-here')), null);
      } finally {
        fs.rmSync(dir, { recursive: true, force: true });
      }
    });
  });

  describe('download progress line', () => {
    it('fits the width it is given, at the narrowest layout and the widest', () => {
      for (const cols of [46, 50, 56, 70, 80, 100]) {
        for (const done of [0, 1, 11_650_000, 23_300_000]) {
          const line = downloadLine(done, 23_300_000, { cols });
          assert.ok(
            line.length <= cols,
            `${cols} columns: ${line.length} wide\n${line}`,
          );
        }
      }
    });

    it('keeps the column count when it drops to ASCII', () => {
      const uni = downloadLine(9_000_000, 23_300_000, { cols: 80 });
      const asc = downloadLine(9_000_000, 23_300_000, { cols: 80, unicode: false });
      assert.equal(uni.length, asc.length);
    });

    it('shows bytes and no bar when the server sent no Content-Length', () => {
      // A bar needs a denominator. Inventing one is a progress indicator that lies.
      const line = downloadLine(9_000_000, 0, { cols: 80 });
      assert.ok(line.includes('8.6 MB'));
      assert.ok(!line.includes('%'));
    });

    it('reads as the one-line Archie form the CLI and the installer draw', () => {
      const line = downloadLine(15_900_000, 23_300_000, { cols: 80 });
      assert.ok(line.startsWith(' (o) archie  downloading  '), line);
      assert.ok(line.endsWith('15.2 / 22.2 MB'), line);
      assert.ok(line.includes(' 68%'), line);
    });
  });
});
