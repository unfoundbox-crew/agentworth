# AgentWorth (npm package)

Official npm launcher for **AgentWorth** — discover, normalize, and understand AI-agent histories locally.

```bash
npx -y agentworth@latest
```

When run with no arguments, `npx -y agentworth@latest` defaults to launching the local web UI (`serve --open`). All native subcommands and flags are forwarded transparently to the native binary.

---

## Usage

### Quick Start

Launch the local interactive UI:

```bash
npx -y agentworth@latest
```

### CLI Subcommands

Scan and index local agent histories across 11 agent adapters:

```bash
npx -y agentworth@latest scan
```

View summary statistics across all indexed traces:

```bash
npx -y agentworth@latest stats
```

Inspect token rollups, costs, and rolling pacing:

```bash
npx -y agentworth@latest usage --period day
npx -y agentworth@latest usage --pacing
```

Trace file modifications back to the AI agent session and prompt:

```bash
npx -y agentworth@latest blame src/main.rs
```

List indexed sessions with filtering by adapter or model:

```bash
npx -y agentworth@latest traces --limit 20
npx -y agentworth@latest traces --adapter claude_code --json
```

Inspect a specific session with timeline and outcome analysis:

```bash
npx -y agentworth@latest inspect <session-id>
```

Export traces safely with automatic secret and path redaction:

```bash
npx -y agentworth@latest export <session-id> --redact --format atif --output session.atif.json
```

---

## How It Works

`agentworth` resolves and executes the high-performance native Rust binary on your machine.

### Binary Resolution Order

The launcher searches for the native binary in the following priority order:

1. **`AGENTWORTH_BIN` environment variable**: Explicit path to binary.
2. **Pre-bundled / platform packages**: Pre-compiled binary for current OS and architecture (e.g. `darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`, `win32-x64`).
3. **Local Cargo build artifacts**: `target/release/agentworth` or `target/debug/agentworth` found by ascending from working directory or package root.
4. **`CARGO_TARGET_DIR`**: Custom cargo target output directory if set.
5. **User Cargo Bin**: `~/.cargo/bin/agentworth`.
6. **System `PATH`**: Any `agentworth` executable in your `PATH`.
7. **Local cache**: `~/.agentworth/bin/v<version>/`, populated by an on-demand download the first time a version isn't found anywhere above.

### On-demand download

When no source above has the binary, the launcher downloads the matching release archive into
`~/.agentworth/bin/v<version>/`. That download is checksum-verified against the release's
published `.sha256` before extraction, extracted into a temporary directory and only moved into
place once complete (so the version directory is always either fully installed or absent, never
half-extracted), and guarded by a lock file so multiple concurrent first runs share one download
instead of racing each other. `agentworth hook` never triggers this download -- a hook only uses
a binary that's already present, so it can fail fast and quietly instead of blocking on a fetch.

---

## Alternative Installation Methods

You can also install the native AgentWorth binary directly:

| Method | Command | Description |
| :--- | :--- | :--- |
| **Standalone Script** | `curl -fsSL https://agentworth.dev/install.sh | sh` | Installs the pre-built native binary directly to `~/.local/bin`. |
| **NPX (Instant)** | `npx -y agentworth@latest` | Zero-install runner that detects or downloads the native binary. |

---

## Environment Variables

| Variable | Description |
| :--- | :--- |
| `AGENTWORTH_BIN` | Path to a specific `agentworth` binary executable. |
| `CARGO_TARGET_DIR` | Custom Cargo target directory to search for build outputs. |
| `AGENTWORTH_RELEASE_BASE_URL` | Overrides the GitHub Releases base URL the on-demand downloader fetches archives from. Defaults to `https://github.com/unfoundbox-crew/agentworth/releases/download`; only for testing or an internal release mirror. |

---

## License

Apache-2.0
