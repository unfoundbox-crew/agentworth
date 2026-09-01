# Desktop App — feasibility spec

Status: proposal, not started. Written 2026-09-01.

Question this answers: would a Tauri wrapper around AgentWorth produce a working `.dmg`, what would that actually cost, and what has to be decided first.

## What it is, and why anyone would want it over `agentworth serve`

`agentworth serve` already works: it starts a local Axum server on `127.0.0.1`, prints a URL, and can open it in the default browser. The desktop app doesn't change what runs — same server, same SQLite index, same offline-only scanning. It changes how the running tool shows up on the machine.

What a browser tab doesn't give you:

- A dock icon and an app that feels installed, not launched from a terminal.
- Survival past an errant `Cmd+W` or the browser being closed for something else.
- A quit action that actually stops the server, instead of a tab going stale while the process keeps running in a terminal you forgot about.
- Window state (size, position) that persists across launches.
- The menubar presence covered below.

This is a packaging and presence upgrade, not a new product.

## Feasibility: what exists, what is config, what is new code

**Already in place:**

- A Rust backend. `apps/cli` is a lib crate (`agentworth_cli`) plus a bin — `start_server()` in `apps/cli/src/server/mod.rs` builds the Axum app and binds it. Tauri wants exactly this shape: a Rust backend a native shell can drive.
- A web frontend already compiled into the binary. `apps/cli/src/server/static_files.rs` embeds `apps/dashboard/dist` via `rust-embed` (`DashboardAssets`) and serves it with SPA history-fallback. This is the same trick Tauri itself would otherwise want to do.
- A local-only HTTP server. `start_server` binds `SocketAddr::from(([127, 0, 0, 1], port))` — never `0.0.0.0`. Nothing to change here for "local-only" to hold in a desktop shell.

**Two ways to wire Tauri to this, and which one fits:**

1. **Native-embed (recommended).** Add a new workspace crate, `apps/desktop`, that depends on `agentworth_cli` and `tauri`. In Tauri's `setup()` hook, spawn `start_server()` on a background tokio task inside the same process, then open a `WebviewWindow` at `http://127.0.0.1:<port>`. This reuses the server and dashboard as-is. New code is just the Tauri shell: a `main.rs` of maybe 40-60 lines, `tauri.conf.json`, and an icon set.
2. **Sidecar.** Bundle the existing `agentworth` CLI binary as a Tauri `externalBin`, have the app spawn it as a subprocess, point a webview at its port. More process isolation, but two binaries to build, sign, and version instead of one — and since the code is already Rust in the same workspace, there's no serialization boundary sidecar isolation would actually be buying you.

Path 1 is the better fit here: no IPC boundary to invent, no second artifact to sign per platform, and the desktop crate can call `start_server` directly.

**Net effect:** this is closer to "a new thin crate plus CI wiring" than a rewrite. `agentworth-core`, `agentworth-storage`, every adapter, the CLI, and the dashboard React app are untouched. The honest framing: mostly configuration and packaging, with one small new crate. Not a rewrite, and not "just a settings file" either — someone still has to write and test the Tauri shell, the tray/menubar logic if that's built, and the CI job.

## The index-ownership decision

This has to be settled before any of the above gets built, because it changes what `apps/desktop` is allowed to assume.

**The concurrency problem already exists today, independent of this spec.** `Storage::open_path` (`crates/storage/src/lib.rs`) is called independently by every CLI invocation — `agentworth scan`, `agentworth serve`, `agentworth stats`, etc. each open their own `Arc<Mutex<Connection>>` to the same `~/.agentworth/agentworth.db`, already in `PRAGMA journal_mode = WAL` (set in `initialize_schema`). Running `agentworth serve` in one terminal and `agentworth scan` in another already hits this, today, with zero desktop app involved. A desktop app just makes the "two processes touching one DB" case the common path instead of a rare one, because a persistent app sitting in the dock is far more likely to overlap with a terminal `scan` than two manually-run terminal commands are.

```
today:  [ agentworth serve ]──┐
                               ├──▶ ~/.agentworth/agentworth.db  (WAL)
        [ agentworth scan  ]──┘

with desktop app:  [ AgentWorth.app (embeds server) ]──┐
                                                          ├──▶ same file, same WAL
                    [ agentworth scan, run from a shell ]┘
```

Options, as laid out in the brief:

| Option | What it requires | Verdict |
|---|---|---|
| App owns the index; CLI becomes a client | A new IPC/RPC surface so the CLI can talk to the app instead of SQLite directly. Only works while the app is running — CLI stops working standalone. | Rejected: breaks "CLI works with no app running," which is the whole product today. |
| CLI owns it; app attaches | Same problem in reverse — app can't function without a CLI-managed daemon. | Rejected for the same reason, mirrored. |
| Both use SQLite WAL, accept the concurrency | Nothing new to build except a busy-timeout (see below). | **Recommended.** |
| App spawns and supervises the existing server as a subprocess | This is a process-supervision choice, not an index-ownership one — and it's subsumed by the native-embed path above, which puts the server *inside* the app process rather than supervising a separate one. | Not a separate option once native-embed is chosen; there's no second process to supervise. |

**Recommendation: WAL, accepted, plus a one-line fix.** WAL already allows many concurrent readers and one writer, across processes, at the file level — that's the entire point of the mode, and it's already turned on. The two ownership options both invent a lock-negotiation protocol to solve a problem SQLite already solves. The actual gap isn't ownership, it's that **no connection sets a busy timeout** (checked: no `busy_timeout` call anywhere in `crates/storage` or `apps/cli`). Without one, a write that lands mid-transaction from the other process returns `SQLITE_BUSY` immediately instead of waiting. Setting `PRAGMA busy_timeout = 5000;` (or `Connection::busy_timeout`) in `Storage::open_path` and `open_default` turns "second writer gets an error" into "second writer waits a few hundred ms and proceeds" — which is the actual desired behavior for a single-user local tool, and costs one line plus a test.

Residual risk, called out rather than solved: two writers *simultaneously* running a scan (CLI `scan` and the app's own rescan button, mid-write) can still race on the same fingerprint rows. That risk exists today between two terminal windows and isn't introduced by this spec. Busy-timeout turns the hard failure into a short wait; it doesn't add write-write conflict resolution, and nothing here needs it for a first version.

## Distribution and signing

This is the real cost center, not the code.

### macOS

| Path | What happens | Cost |
|---|---|---|
| Unsigned `.dmg` | Gatekeeper blocks first launch with "Apple could not verify... unidentified developer." Most people bounce rather than right-click → Open. | $0 |
| Signed + notarized | Needs an Apple Developer Program membership, a Developer ID Application certificate, and a notarization step (`tauri build` can drive this via the App Store Connect API or an Apple ID + app-specific password). **A free Apple account cannot notarize at all** — confirmed against Tauri's own v2 docs. | $99/yr (Apple Developer Program) + one-time CI secret setup |

Universal binary: `cargo tauri build --target universal-apple-darwin` builds `aarch64-apple-darwin` and `x86_64-apple-darwin`, then `lipo`s them into one `.app` — both Rust targets need to be installed first. This is a separate build target from the plain CLI's two existing macOS jobs in `release.yml`; it doesn't replace them, since the desktop `.dmg` and the CLI `.tar.gz` are different artifacts serving different install paths.

### Windows

| Path | What happens | Cost |
|---|---|---|
| Unsigned | SmartScreen shows "Windows protected your PC" — user must click "More info" → "Run anyway." | $0, same bounce risk as macOS |
| Signed | An OV code-signing certificate (roughly $65-100+/yr) or cloud signing through something like Azure Key Vault (~$10/mo) reduces but doesn't eliminate SmartScreen friction until the certificate has built reputation; EV certificates (~$400+/yr, hardware token required) clear that faster. | $65-500+/yr depending on tier, plus setup |

**Correction to the brief's premise:** `release.yml`'s current matrix is `macos-latest` (arm64 + x64) and `ubuntu-latest`/`ubuntu-24.04-arm` (x64 + arm64) — **there is no Windows job today.** A Windows build, desktop or CLI, is new CI surface to add, not an existing job to extend.

### Linux

| Format | Notes |
|---|---|
| AppImage | Bundles its own dependencies, no install step, runs on most distros unmodified. No Gatekeeper/SmartScreen-equivalent blocking. |
| `.deb` / `.rpm` | Smaller artifact, relies on system-installed libraries, standard for package-manager users. |

Linux has no notarization-equivalent gate. `release.yml` already builds Linux x86_64 and aarch64 CLI binaries, so a Linux desktop bundle mainly needs Tauri's Linux build dependencies (`webkit2gtk` etc.) added to that existing job rather than a new one.

## The menubar variant

Kept short on purpose — the honest answer is that a menubar icon has room for almost nothing.

What it can realistically hold: an icon reflecting idle / scanning / error state, and a small popover on click showing something like "are any agents running right now, and what did the last one score." That live-fleet content is being specced separately in `docs/specs/fleet-view.md` — this doc doesn't duplicate it, it just names the icon's data source. Tauri v2 supports a tray icon and a window in the same binary, so the menubar variant doesn't need a second app; it's a tray-icon addition to the same `apps/desktop` crate from the feasibility section above.

## Explicitly out of scope

- **Accounts, telemetry, auto-update phoning home, or cloud sync.** None of this changes because the product now has an installer. Local-only is a privacy line rather than a scaling decision — a `.dmg` is a distribution format, and it does not reopen that.
- Tauri ships an official updater plugin that checks a remote URL for new versions. That is a network call. So are `agwt search`'s model download and `agwt blunder --submit`, both of which already ship — see AGENTS.md. If auto-update is wanted later, it needs its own explicit decision — it does not ship by default just because Tauri makes it easy.
- A signed Windows/Linux desktop build, unless real demand shows up — see sequencing below.

## Open questions — needs a human decision

- Auto-update: does Saurabh want the Tauri updater plugin at all, given what already goes out, and that it sends nothing about the user? Answer changes the "zero telemetry" framing even if scanning itself stays untouched.
- Which Apple Developer account does this enroll under — personal or `unfoundbox`? Ties into the multi-account conventions already in `AGENTS.md`.
- Is a Windows desktop build wanted at all, given nobody has asked for one and it is genuinely new CI surface with its own signing cost?
- Same binary with a tray icon that toggles a window, or two separate apps (main window app + menubar-only app)? Tauri supports either; worth deciding before writing the shell.
- Where does this sit against Phase 1 adapter work that HANDOFF.md says likely isn't finished yet — is this actually next, or is it competing with adapter coverage for the same attention?

## Sequencing

1. **Native-embed proof of concept.** `apps/desktop` crate, Tauri window pointed at the in-process server, unsigned local build only, no CI yet. Proves the wrapper works technically at near-zero packaging cost.
2. **Busy-timeout fix.** `PRAGMA busy_timeout` in `Storage::open_path`/`open_default`, plus a test that runs a scan and a read concurrently. Cheap, and closes the one real gap in the recommended index-ownership approach before it can bite anyone.
3. **Unsigned `.dmg` in CI.** Ship it before spending money on signing. This step is the actual value test: if people bounce off the unsigned-app warning anyway, signing spend is premature.
4. **Signing and notarization**, only after step 3 shows the app is worth having: Apple Developer enrollment, notarization in CI, universal binary target.
5. **Windows and Linux desktop builds**, only on real demand — neither has an existing Tauri CI job to extend today, so both are new cost, not incremental.
6. **Menubar variant**, after `docs/specs/fleet-view.md`'s data shape is settled, since the tray content depends on it.

What proves this was worth doing: whether people actually keep the app open and come back to it, versus closing the `agentworth serve` browser tab once and forgetting the tool exists. That's a presence-retention bet, not a technical one — no amount of Tauri correctness answers it.
