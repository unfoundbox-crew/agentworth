import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import https from 'node:https';
import zlib from 'node:zlib';
import { spawnSync, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

/**
 * Returns the normalized platform key (e.g. darwin-arm64, linux-x64, win32-x64).
 *
 * @param {string} [platform=process.platform]
 * @param {string} [arch=process.arch]
 * @returns {string}
 */
export function getPlatformKey(platform = process.platform, arch = process.arch) {
  return `${platform}-${arch}`;
}

/**
 * Maps Node platform and arch to Rust release target triple.
 *
 * @param {string} [platform=process.platform]
 * @param {string} [arch=process.arch]
 * @returns {string | null}
 */
export function getTargetTriple(platform = process.platform, arch = process.arch) {
  if (platform === 'darwin' && arch === 'arm64') return 'aarch64-apple-darwin';
  if (platform === 'darwin' && arch === 'x64') return 'x86_64-apple-darwin';
  if (platform === 'linux' && arch === 'x64') return 'x86_64-unknown-linux-gnu';
  if (platform === 'linux' && arch === 'arm64') return 'aarch64-unknown-linux-gnu';
  // Windows dropped 2026-09-02: release.yml no longer builds x86_64-pc-windows-msvc, so
  // this would 404 against the downloader even if it matched. The recurring failure was
  // GNU tar on Windows misparsing a "C:\..." archive path as a remote "host:path" tar
  // spec -- no clean fix ever stuck (a --force-local attempt just broke macOS's BSD tar
  // instead), and nobody here can test a real Windows box to catch it before it ships. A
  // user who wants this on Windows can still build it themselves: `cargo build --release
  // -p agentworth-cli`.
  return null;
}

/**
 * Reads the package version from package.json.
 *
 * @param {string} [baseDir=__dirname]
 * @returns {string}
 */
export function getPackageVersion(baseDir = __dirname) {
  try {
    const pkgPath = path.resolve(baseDir, '..', 'package.json');
    if (fs.existsSync(pkgPath)) {
      const data = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
      return data.version || '0.1.3';
    }
  } catch {
    // fallback
  }
  return '0.1.3';
}

/**
 * Returns the local cache directory for AgentWorth binaries (~/.agentworth/bin/v{version}/).
 *
 * @param {string} [version]
 * @param {string} [homeDir]
 * @returns {string}
 */
export function getCacheDir(version, homeDir) {
  const v = version || getPackageVersion();
  const home = homeDir || os.homedir();
  return path.join(home, '.agentworth', 'bin', `v${v}`);
}

/**
 * Returns the expected native binary name for the target platform.
 *
 * @param {string} [platform=process.platform]
 * @returns {string}
 */
export function getBinaryName(platform = process.platform) {
  return platform === 'win32' ? 'agentworth.exe' : 'agentworth';
}

/**
 * Resolves CLI arguments, defaulting to `serve --open` when invoked without subcommands.
 *
 * @param {string[]} [rawArgs=process.argv.slice(2)]
 * @returns {string[]}
 */
export function resolveArguments(rawArgs = process.argv.slice(2)) {
  if (!rawArgs || rawArgs.length === 0) {
    return ['serve', '--open'];
  }
  return [...rawArgs];
}

/**
 * Checks if a file path exists and is executable.
 *
 * @param {string} filePath
 * @returns {boolean}
 */
export function isExecutable(filePath) {
  try {
    const stats = fs.statSync(filePath);
    if (!stats.isFile()) {
      return false;
    }
    if (process.platform === 'win32') {
      return true;
    }
    // Check execute permissions for user, group, or others
    return (stats.mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

/**
 * Searches upward for a directory containing a cargo target folder with the agentworth binary.
 *
 * @param {string} startDir
 * @param {string} binName
 * @returns {string | null}
 */
export function findCargoTargetBinary(startDir, binName = getBinaryName()) {
  let currentDir = path.resolve(startDir);
  const root = path.parse(currentDir).root;

  while (true) {
    // Check release first, then debug
    const releaseBin = path.join(currentDir, 'target', 'release', binName);
    if (isExecutable(releaseBin)) {
      return releaseBin;
    }

    const debugBin = path.join(currentDir, 'target', 'debug', binName);
    if (isExecutable(debugBin)) {
      return debugBin;
    }

    if (currentDir === root) {
      break;
    }
    currentDir = path.dirname(currentDir);
  }

  return null;
}

/**
 * Downloads a file following HTTP redirects.
 *
 * @param {string} url
 * @param {string} destPath
 * @param {number} [redirects=5]
 * @returns {Promise<void>}
 */
export function downloadFile(url, destPath, redirects = 5) {
  return new Promise((resolve, reject) => {
    if (redirects < 0) {
      return reject(new Error('Too many redirects while downloading binary.'));
    }

    const request = https.get(url, { headers: { 'User-Agent': 'agentworth-npm-resolver' } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return downloadFile(res.headers.location, destPath, redirects - 1).then(resolve, reject);
      }

      if (res.statusCode !== 200) {
        return reject(new Error(`Failed to download binary: HTTP ${res.statusCode} from ${url}`));
      }

      const fileStream = fs.createWriteStream(destPath);
      res.pipe(fileStream);

      fileStream.on('finish', () => {
        fileStream.close(resolve);
      });

      fileStream.on('error', (err) => {
        fs.unlink(destPath, () => reject(err));
      });
    });

    request.on('error', (err) => {
      reject(err);
    });
  });
}


/**
 * True when the binary at `binPath` reports `expected`. Used to stop a stale
 * install on PATH from answering for an explicitly requested version.
 * Any failure to ask — missing binary, wrong arch, timeout — counts as "no",
 * so we fall through to the download rather than trusting an unknown.
 */
function pathBinaryMatchesVersion(binPath, expected) {
  if (!expected) return true;
  try {
    const out = execFileSync(binPath, ['--version'], {
      encoding: 'utf8',
      timeout: 5000,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    return out.includes(expected);
  } catch (_) {
    return false;
  }
}

/**
 * Downloads and extracts the precompiled native binary from GitHub Releases into the user's local cache.
 *
 * @param {Object} [options={}]
 * @param {string} [options.platform=process.platform]
 * @param {string} [options.arch=process.arch]
 * @param {string} [options.version]
 * @param {string} [options.homeDir]
 * @param {boolean} [options.silent=false]
 * @returns {Promise<string>} Path to extracted binary
 */
export async function downloadAndExtractBinary(options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const version = options.version || getPackageVersion();
  const targetTriple = getTargetTriple(platform, arch);
  const binName = getBinaryName(platform);

  if (!targetTriple) {
    throw new Error(`Unsupported platform/architecture: ${platform}-${arch}`);
  }

  const cacheDir = getCacheDir(version, options.homeDir);
  const cachedBinary = path.join(cacheDir, binName);

  if (isExecutable(cachedBinary)) {
    return cachedBinary;
  }

  fs.mkdirSync(cacheDir, { recursive: true });

  const archiveName = `agentworth-v${version}-${targetTriple}.tar.gz`;
  const url = `https://github.com/unfoundbox-crew/agentworth/releases/download/v${version}/${archiveName}`;

  if (!options.silent) {
    console.error(`\x1b[36m⚡ AgentWorth native binary not found locally. Downloading v${version} for ${platform}-${arch}...\x1b[0m`);
  }

  const archivePath = path.join(cacheDir, archiveName);

  await downloadFile(url, archivePath);

  // No --force-local here: it was a Windows-only GNU tar workaround, and Windows
  // support was dropped in 8b837c3. BSD tar (macOS's default) doesn't recognize
  // the flag and fails every extraction with it present -- v0.1.11 shipped this
  // exact break. Keep it off; there's no platform left where it does anything.
  try {
    execFileSync('tar', ['-xzf', archivePath, '-C', cacheDir]);
  } catch (err) {
    throw new Error(`Failed to extract ${archiveName}: ${err.message}`);
  } finally {
    try {
      if (fs.existsSync(archivePath)) {
        fs.unlinkSync(archivePath);
      }
    } catch {}
  }

  if (process.platform !== 'win32') {
    fs.chmodSync(cachedBinary, 0o755);
  }

  if (!isExecutable(cachedBinary)) {
    throw new Error(`Downloaded binary is not executable: ${cachedBinary}`);
  }

  if (!options.silent) {
    console.error(`\x1b[32m✔ Successfully installed AgentWorth native binary to ${cachedBinary}\x1b[0m\n`);
  }

  return cachedBinary;
}

/**
 * Searches for the agentworth binary in the system PATH.
 *
 * @param {string} [binName=getBinaryName()]
 * @param {string} [pathEnv=process.env.PATH]
 * @param {string} [currentScriptPath]
 * @returns {string | null}
 */
/**
 * True when `candidate` and `selfPath` are the same file, following symlinks.
 */
export function isSelf(candidate, selfPath) {
  if (!selfPath) return false;
  try {
    return fs.realpathSync(candidate) === fs.realpathSync(selfPath);
  } catch {
    return path.resolve(candidate) === path.resolve(selfPath);
  }
}

/**
 * True when `candidate` is a Node script (a bin shim) rather than the native
 * binary. Covers shims that are copies rather than symlinks.
 */
export function looksLikeNodeScript(filePath) {
  try {
    if (/\.(js|cjs|mjs)$/.test(filePath)) return true;
    const fd = fs.openSync(filePath, 'r');
    const buf = Buffer.alloc(64);
    const n = fs.readSync(fd, buf, 0, 64, 0);
    fs.closeSync(fd);
    const first = buf.subarray(0, n).toString('utf8').split('\n')[0];
    return first.startsWith('#!') && /\bnode\b/.test(first);
  } catch {
    return false;
  }
}

export function findPathBinary(binName = getBinaryName(), pathEnv = process.env.PATH, currentScriptPath) {
  if (!pathEnv) {
    return null;
  }

  const entries = pathEnv.split(path.delimiter);
  for (const entry of entries) {
    if (!entry) continue;
    const candidate = path.join(entry, binName);
    if (isExecutable(candidate)) {
      // Avoid circular invocation if PATH points back at this JS wrapper.
      // npm/npx put node_modules/.bin on PATH and the entry there is a SYMLINK
      // to this very file. path.resolve() does not follow symlinks, so this guard
      // missed it and the launcher spawned itself until the OS refused to fork
      // (EAGAIN on macOS, v0.1.4). Compare real paths, and reject Node scripts.
      if (isSelf(candidate, currentScriptPath) || looksLikeNodeScript(candidate)) {
        continue;
      }
      return candidate;
    }
  }

  return null;
}

/**
 * Resolves the location of the native agentworth binary.
 *
 * Search priority:
 * 1. AGENTWORTH_BIN environment variable
 * 2. Platform-specific optional package / vendor directory
 * 3. System PATH
 * 4. Cargo target in cwd directory hierarchy (release or debug)
 * 5. Cargo target in package directory hierarchy
 * 6. CARGO_TARGET_DIR if set
 * 7. User ~/.cargo/bin/
 * 8. User local cache ~/.agentworth/bin/v{version}/
 *
 * @param {Object} [options={}]
 * @param {string} [options.cwd=process.cwd()]
 * @param {string} [options.platform=process.platform]
 * @param {string} [options.arch=process.arch]
 * @param {NodeJS.ProcessEnv} [options.env=process.env]
 * @param {string} [options.baseDir=__dirname]
 * @param {string} [options.homeDir]
 * @returns {{ found: boolean, path?: string, source?: string, error?: string }}
 */
export function resolveBinary(options = {}) {
  const cwd = options.cwd || process.cwd();
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const env = options.env || process.env;
  const baseDir = options.baseDir || __dirname;
  const homeDir = options.homeDir || (env.HOME ? env.HOME : os.homedir());
  const binName = getBinaryName(platform);
  const platformKey = getPlatformKey(platform, arch);

  // 1. Explicit environment variable override
  if (env.AGENTWORTH_BIN && isExecutable(env.AGENTWORTH_BIN)) {
    return {
      found: true,
      path: path.resolve(env.AGENTWORTH_BIN),
      source: 'env:AGENTWORTH_BIN',
    };
  }

  // 2. Prebuilt platform packages (e.g. optionalDependencies @agentworth/darwin-arm64)
  const vendorCandidate = path.resolve(baseDir, '..', 'vendor', platformKey, binName);
  if (isExecutable(vendorCandidate)) {
    return {
      found: true,
      path: vendorCandidate,
      source: 'vendor',
    };
  }

  // 3. System PATH — but only when it agrees with the version we are.
  // Skipped when already inside a launcher: the only thing PATH could offer is us.
  //
  // `npx agentworth@0.1.6` used to run whatever agentworth happened to be on
  // PATH, silently, so pinning a version did nothing for anyone with a local
  // install. A stale 0.1.2 in ~/.cargo/bin answered for 0.1.6 with no warning.
  // A PATH binary now has to report the version this launcher ships before it
  // gets to serve; otherwise we fall through and fetch the matching release.
  const currentBin = path.resolve(baseDir, '..', 'bin', 'agentworth.js');
  if (env.PATH !== undefined && !env.AGENTWORTH_LAUNCHER_ACTIVE) {
    const pathBin = findPathBinary(binName, env.PATH, currentBin);
    if (pathBin && pathBinaryMatchesVersion(pathBin, getPackageVersion(baseDir))) {
      return {
        found: true,
        path: pathBin,
        source: 'path',
      };
    }
  }

  // 4-7 all found a real, executable candidate but said nothing about whether it's the
  // version this launcher is meant to run -- so a debug build from six releases ago,
  // sitting in some worktree's target/ directory upward of wherever the user happened to
  // invoke `agentworth` from, would silently win over the correctly-versioned release cache
  // (8) forever. Same failure mode already fixed for PATH (3) above, same fix here: a
  // candidate only counts if it reports the version this launcher ships.
  const expectedVersion = getPackageVersion(baseDir);

  // 4. Local Cargo target from working directory
  const cwdCargoBin = findCargoTargetBinary(cwd, binName);
  if (cwdCargoBin && pathBinaryMatchesVersion(cwdCargoBin, expectedVersion)) {
    return {
      found: true,
      path: cwdCargoBin,
      source: 'cargo-target-cwd',
    };
  }

  // 5. Local Cargo target from package directory hierarchy
  const pkgCargoBin = findCargoTargetBinary(baseDir, binName);
  if (pkgCargoBin && pathBinaryMatchesVersion(pkgCargoBin, expectedVersion)) {
    return {
      found: true,
      path: pkgCargoBin,
      source: 'cargo-target-pkg',
    };
  }

  // 6. CARGO_TARGET_DIR environment variable if specified
  if (env.CARGO_TARGET_DIR) {
    const targetDirRelease = path.join(env.CARGO_TARGET_DIR, 'release', binName);
    if (isExecutable(targetDirRelease) && pathBinaryMatchesVersion(targetDirRelease, expectedVersion)) {
      return {
        found: true,
        path: targetDirRelease,
        source: 'cargo-target-dir-release',
      };
    }
    const targetDirDebug = path.join(env.CARGO_TARGET_DIR, 'debug', binName);
    if (isExecutable(targetDirDebug) && pathBinaryMatchesVersion(targetDirDebug, expectedVersion)) {
      return {
        found: true,
        path: targetDirDebug,
        source: 'cargo-target-dir-debug',
      };
    }
  }

  // 7. User ~/.cargo/bin directory
  if (homeDir) {
    const cargoHomeBin = path.join(homeDir, '.cargo', 'bin', binName);
    if (isExecutable(cargoHomeBin) && pathBinaryMatchesVersion(cargoHomeBin, expectedVersion)) {
      return {
        found: true,
        path: cargoHomeBin,
        source: 'cargo-home-bin',
      };
    }

    // 8. User local cache (~/.agentworth/bin/v{version}/) -- versioned by construction,
    // nothing to check.
    const cacheDir = getCacheDir(expectedVersion, homeDir);
    const cachedBin = path.join(cacheDir, binName);
    if (isExecutable(cachedBin)) {
      return {
        found: true,
        path: cachedBin,
        source: 'cache',
      };
    }
  }

  return {
    found: false,
    error: `AgentWorth native binary '${binName}' was not found for platform '${platformKey}'.`,
  };
}

/**
 * Formats a friendly missing binary error message with installation instructions.
 *
 * @param {string} [platformKey=getPlatformKey()]
 * @returns {string}
 */
export function formatMissingBinaryMessage(platformKey = getPlatformKey()) {
  return [
    `\x1b[31m✖ Error: AgentWorth native binary not found for ${platformKey}.\x1b[0m`,
    '',
    'AgentWorth requires the native binary to run. You can install or build it via:',
    '',
    '  \x1b[36m• Build from local source:\x1b[0m',
    '      cargo build --release -p agentworth-cli',
    '',
    '  \x1b[36m• Install via Cargo:\x1b[0m',
    '      cargo install --path apps/cli',
    '',
    '  \x1b[36m• Download the release for your platform:\x1b[0m',
    `      https://github.com/unfoundbox-crew/agentworth/releases/latest`,
    '',
    '  \x1b[36m• Set custom binary path:\x1b[0m',
    '      export AGENTWORTH_BIN=/path/to/agentworth',
    '',
  ].join('\n');
}

/**
 * Builds the environment passed to the spawned native binary: the caller's base env plus
 * the launcher markers the binary's `agentworth version`/`agentworth update` commands read
 * to detect an npm-managed install (see `apps/cli/src/commands/version_info.rs`).
 *
 * @param {NodeJS.ProcessEnv} baseEnv
 * @param {string} npmVersion
 * @returns {NodeJS.ProcessEnv}
 */
export function buildChildEnv(baseEnv, npmVersion) {
  return {
    ...baseEnv,
    AGENTWORTH_LAUNCHER_ACTIVE: '1',
    AGENTWORTH_NPM_VERSION: npmVersion,
  };
}

/**
 * Launches the native binary with the given arguments.
 *
 * @param {string[]} [argv=process.argv.slice(2)]
 * @param {Object} [options={}]
 * @returns {number} Exit code
 */
export function run(argv = process.argv.slice(2), options = {}) {
  const resolvedArgs = resolveArguments(argv);
  let binaryResult = resolveBinary(options);

  // If binary not found on clean machine, attempt on-demand download from GitHub Release
  if ((!binaryResult.found || !binaryResult.path) && options.autoDownload !== false) {
    try {
      const resolverModuleUrl = new URL('./resolver.js', import.meta.url).href;
      const syncDownloadScript = `
        import { downloadAndExtractBinary } from '${resolverModuleUrl}';
        await downloadAndExtractBinary({
          platform: ${JSON.stringify(options.platform || process.platform)},
          arch: ${JSON.stringify(options.arch || process.arch)},
          homeDir: ${JSON.stringify(options.homeDir || (options.env && options.env.HOME) || '')}
        });
      `;
      const dlResult = spawnSync(process.execPath, ['--input-type=module', '-e', syncDownloadScript], {
        stdio: 'inherit',
        env: options.env || process.env,
      });

      if (dlResult.status === 0) {
        binaryResult = resolveBinary(options);
      }
    } catch {
      // Fall through to error message below
    }
  }

  if (!binaryResult.found || !binaryResult.path) {
    const message = formatMissingBinaryMessage(getPlatformKey(options.platform, options.arch));
    console.error(message);
    return 1;
  }

  // Belt and braces: a child that somehow re-enters this launcher cannot loop (the
  // recursion guard in findPathBinary checks AGENTWORTH_LAUNCHER_ACTIVE). The npm version
  // is threaded through too, purely so `agentworth version`/`agentworth update` can report
  // it -- this launcher never reads it back itself.
  const childEnv = buildChildEnv(options.env || process.env, getPackageVersion(options.baseDir || __dirname));
  const result = spawnSync(binaryResult.path, resolvedArgs, {
    stdio: 'inherit',
    env: childEnv,
  });

  if (result.error) {
    console.error(`\x1b[31m✖ Failed to execute ${binaryResult.path}:\x1b[0m`, result.error.message);
    return 1;
  }

  if (result.signal) {
    // Process was killed by a signal
    return 128 + 15;
  }

  return result.status !== null ? result.status : 0;
}
