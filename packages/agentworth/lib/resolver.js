import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
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
 * Searches for the agentworth binary in the system PATH.
 *
 * @param {string} [binName=getBinaryName()]
 * @param {string} [pathEnv=process.env.PATH]
 * @param {string} [currentScriptPath]
 * @returns {string | null}
 */
export function findPathBinary(binName = getBinaryName(), pathEnv = process.env.PATH, currentScriptPath) {
  if (!pathEnv) {
    return null;
  }

  const entries = pathEnv.split(path.delimiter);
  for (const entry of entries) {
    if (!entry) continue;
    const candidate = path.join(entry, binName);
    if (isExecutable(candidate)) {
      // Avoid circular invocation if PATH points to this JS wrapper script
      if (currentScriptPath && path.resolve(candidate) === path.resolve(currentScriptPath)) {
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
 * 3. Cargo target in cwd directory hierarchy (release or debug)
 * 4. Cargo target in package directory hierarchy
 * 5. CARGO_TARGET_DIR if set
 * 6. User ~/.cargo/bin/
 * 7. System PATH
 *
 * @param {Object} [options={}]
 * @param {string} [options.cwd=process.cwd()]
 * @param {string} [options.platform=process.platform]
 * @param {string} [options.arch=process.arch]
 * @param {NodeJS.ProcessEnv} [options.env=process.env]
 * @param {string} [options.baseDir=__dirname]
 * @returns {{ found: boolean, path?: string, source?: string, error?: string }}
 */
export function resolveBinary(options = {}) {
  const cwd = options.cwd || process.cwd();
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const env = options.env || process.env;
  const baseDir = options.baseDir || __dirname;
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

  // 3. Local Cargo target from working directory
  const cwdCargoBin = findCargoTargetBinary(cwd, binName);
  if (cwdCargoBin) {
    return {
      found: true,
      path: cwdCargoBin,
      source: 'cargo-target-cwd',
    };
  }

  // 4. Local Cargo target from package directory hierarchy
  const pkgCargoBin = findCargoTargetBinary(baseDir, binName);
  if (pkgCargoBin) {
    return {
      found: true,
      path: pkgCargoBin,
      source: 'cargo-target-pkg',
    };
  }

  // 5. CARGO_TARGET_DIR environment variable if specified
  if (env.CARGO_TARGET_DIR) {
    const targetDirRelease = path.join(env.CARGO_TARGET_DIR, 'release', binName);
    if (isExecutable(targetDirRelease)) {
      return {
        found: true,
        path: targetDirRelease,
        source: 'cargo-target-dir-release',
      };
    }
    const targetDirDebug = path.join(env.CARGO_TARGET_DIR, 'debug', binName);
    if (isExecutable(targetDirDebug)) {
      return {
        found: true,
        path: targetDirDebug,
        source: 'cargo-target-dir-debug',
      };
    }
  }

  // 6. User ~/.cargo/bin directory
  const cargoHomeBin = path.join(os.homedir(), '.cargo', 'bin', binName);
  if (isExecutable(cargoHomeBin)) {
    return {
      found: true,
      path: cargoHomeBin,
      source: 'cargo-home-bin',
    };
  }

  // 7. System PATH
  const currentBin = path.resolve(baseDir, '..', 'bin', 'agentworth.js');
  const pathBin = findPathBinary(binName, env.PATH, currentBin);
  if (pathBin) {
    return {
      found: true,
      path: pathBin,
      source: 'path',
    };
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
    '  \x1b[36m• Install via Homebrew:\x1b[0m',
    '      brew install agentworth',
    '',
    '  \x1b[36m• Install via Shell script:\x1b[0m',
    '      curl -fsSL https://agentworth.dev/install.sh | sh',
    '',
    '  \x1b[36m• Set custom binary path:\x1b[0m',
    '      export AGENTWORTH_BIN=/path/to/agentworth',
    '',
  ].join('\n');
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
  const binaryResult = resolveBinary(options);

  if (!binaryResult.found || !binaryResult.path) {
    const message = formatMissingBinaryMessage(getPlatformKey(options.platform, options.arch));
    console.error(message);
    return 1;
  }

  const result = spawnSync(binaryResult.path, resolvedArgs, {
    stdio: 'inherit',
    env: options.env || process.env,
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
