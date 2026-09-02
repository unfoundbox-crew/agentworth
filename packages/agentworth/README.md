# AgentWorth (npm package)

Official npm launcher for **AgentWorth** — discover, normalize, and understand AI-agent histories locally.

```bash
npx agentworth
```

When run with no arguments, `npx agentworth` defaults to launching the local web UI (`serve --open`). All native subcommands and flags are forwarded transparently to the native binary.

---

## Usage

### Quick Start

Launch the local interactive UI:

```bash
npx agentworth
```

### CLI Subcommands

Scan and index local agent histories across 11 agent adapters:

```bash
npx agentworth scan
```

View summary statistics across all indexed traces:

```bash
npx agentworth stats
```

Inspect token rollups, costs, and rolling pacing:

```bash
npx agentworth usage --period day
npx agentworth usage --pacing
```

Trace file modifications back to the AI agent session and prompt:

```bash
npx agentworth blame src/main.rs
```

List indexed sessions with filtering by adapter or model:

```bash
npx agentworth traces --limit 20
npx agentworth traces --adapter claude_code --json
```

Inspect a specific session with timeline and outcome analysis:

```bash
npx agentworth inspect <session-id>
```

Export traces safely with automatic secret and path redaction:

```bash
npx agentworth export <session-id> --redact --format atif --output session.atif.json
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

---

## Alternative Installation Methods

You can also install the native AgentWorth binary directly:

| Method | Command | Description |
| :--- | :--- | :--- |
| **Standalone Script** | `curl -fsSL https://agentworth.dev/install.sh | sh` | Installs the pre-built native binary directly to `~/.local/bin`. |
| **Homebrew** | `brew install unfoundbox-crew/tap/agentworth` | Installs via official Homebrew tap. |
| **Cargo (Native)** | `cargo install agentworth-cli` | Compiles and installs `agentworth` and its short alias `archie` to `~/.cargo/bin`. |
| **NPX (Instant)** | `npx agentworth` | Zero-install runner that detects or downloads the native binary. |

---

## Environment Variables

| Variable | Description |
| :--- | :--- |
| `AGENTWORTH_BIN` | Path to a specific `agentworth` binary executable. |
| `CARGO_TARGET_DIR` | Custom Cargo target directory to search for build outputs. |

---

## License

Apache-2.0
