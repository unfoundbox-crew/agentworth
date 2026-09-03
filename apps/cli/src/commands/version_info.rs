//! Version reporting and update checking for AgentWorth.
//!
//! Subcommands: `archie version [--offline] [--json]` and
//! `archie update [--offline] [--json]`.
//!
//! AgentWorth ships two ways: the native `agentworth`/`agwt` binaries (built from this
//! source, installed via `cargo install --path apps/cli`, or downloaded as a GitHub
//! Release tarball), and the `agentworth` npm package, whose JS launcher
//! (`packages/agentworth/bin/agentworth.js` -> `lib/resolver.js`) resolves and execs one
//! of those same native binaries as a child process. This binary has no filesystem link
//! back to any npm package.json at runtime -- the only signal it has that it was launched
//! by the npm wrapper is the `AGENTWORTH_LAUNCHER_ACTIVE`/`AGENTWORTH_NPM_VERSION`
//! environment variables the wrapper sets on its child (see `resolver.js`'s `run()`).
//!
//! `update` deliberately never rewrites its own install: an npm-managed binary is npm's
//! to replace (`npm install -g agentworth@latest`), and a binary installed some other way
//! (raw GitHub Release download, `cargo install`, a source build) has no single "correct"
//! self-replace strategy this binary could safely automate from inside itself. So this
//! command only checks and advises, reading the same GitHub Releases source of truth
//! `resolver.js` already downloads real release binaries from -- not the npm registry,
//! since resolver.js never queries that for "latest" either (it only reads its own local
//! package.json), and GitHub Releases is the one place both distribution paths agree on.
//!
//! Which install commands this screen may print is a checked question, not a guessed one.
//! `npm install -g agentworth@latest` and the GitHub Releases page were confirmed live
//! 2026-09-01 against the registry and the releases API. `curl -fsSL
//! https://agentworth.dev/install.sh | sh` was re-checked 2026-09-03 and now works: the
//! URL returns the real POSIX script committed at `apps/web/public/install.sh` (HTTP 200,
//! `text/plain`, ~11 KB). It did not work on 2026-09-01, when the site served its SPA
//! `index.html` for that route, and this module's comment said so; the script shipping is
//! what changed. Two install methods README.md has advertised still do NOT work and must
//! never be suggested here: `brew install unfoundbox-crew/tap/agentworth` 404s (no such
//! tap repo), and `cargo install agentworth-cli` has no matching crate on crates.io.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use serde_json::json;

use crate::ui::views::{self, UpdateChannelView, UpdateView, VersionView};
use crate::ui::{Role, Ui};

/// GitHub `owner/repo` slug this binary's releases are published under. Matches the
/// literal string `packages/agentworth/lib/resolver.js` already hardcodes for its own
/// release-download URLs.
const REPO_SLUG: &str = "unfoundbox-crew/agentworth";

/// Network timeout for the live update check. Short and non-configurable on purpose: this
/// runs by default on every `version`/`update` invocation (unless `--offline`), so a slow
/// or unreachable network must fail fast rather than make an otherwise-instant command
/// hang or feel broken.
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// How this binary was invoked, as far as it can tell from its environment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LauncherInfo {
    /// Set when `AGENTWORTH_LAUNCHER_ACTIVE=1` -- the npm JS wrapper sets this on every
    /// child it spawns (see `resolver.js`'s `run()`).
    pub npm_launcher_active: bool,
    /// The npm package's own `version` field, threaded through by the wrapper via
    /// `AGENTWORTH_NPM_VERSION`. Can differ from this binary's own `CARGO_PKG_VERSION` if
    /// a stale cached binary sits under a newer npm package, or vice versa.
    pub npm_package_version: Option<String>,
}

/// Branching logic kept separate from environment access so it is unit-testable without
/// touching real process-global state.
fn detect_launcher_from(active: Option<&str>, npm_version: Option<&str>) -> LauncherInfo {
    LauncherInfo {
        // resolver.js only ever writes the literal string "1"; match that exactly rather
        // than any truthy-looking value.
        npm_launcher_active: active == Some("1"),
        npm_package_version: npm_version.map(|s| s.to_string()),
    }
}

/// Reads the real environment.
pub fn detect_launcher() -> LauncherInfo {
    let active = std::env::var("AGENTWORTH_LAUNCHER_ACTIVE").ok();
    let npm_version = std::env::var("AGENTWORTH_NPM_VERSION").ok();
    detect_launcher_from(active.as_deref(), npm_version.as_deref())
}

/// Which install channel this binary came from -- the one `update` puts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Npm,
    InstallSh,
    Unknown,
}

impl Channel {
    /// The name this channel is listed under on the `update` screen. `None` for a channel
    /// with nothing to point at.
    fn name(self) -> Option<&'static str> {
        match self {
            Channel::Npm => Some("npm"),
            Channel::InstallSh => Some("install.sh"),
            Channel::Unknown => None,
        }
    }
}

/// Branching kept out of the environment so both signals are injectable.
///
/// The npm marker wins: the wrapper set it on this very process, which is a stronger claim
/// than any path prefix. Otherwise the install script's own destination is the signal --
/// `apps/web/public/install.sh` writes to `${AGENTWORTH_INSTALL_DIR:-$HOME/.local/bin}`.
/// Both roots are checked rather than just the env var, because the var is read at install
/// time and need not still be set in the shell that later runs `update`.
fn detect_channel_from(
    launcher: &LauncherInfo,
    exe: Option<&Path>,
    install_dir: Option<&str>,
    home: Option<&str>,
) -> Channel {
    if launcher.npm_launcher_active {
        return Channel::Npm;
    }
    let Some(exe) = exe else {
        return Channel::Unknown;
    };
    let roots = [
        install_dir.filter(|d| !d.is_empty()).map(PathBuf::from),
        home.filter(|h| !h.is_empty())
            .map(|h| Path::new(h).join(".local").join("bin")),
    ];
    if roots.iter().flatten().any(|root| exe.starts_with(root)) {
        Channel::InstallSh
    } else {
        Channel::Unknown
    }
}

/// Reads the real environment. A `current_exe()` this process cannot resolve is not an
/// error -- it just leaves the channel unknown, the same as any unrecognised location.
pub fn detect_channel(launcher: &LauncherInfo) -> Channel {
    let exe = std::env::current_exe().ok();
    let install_dir = std::env::var("AGENTWORTH_INSTALL_DIR").ok();
    let home = std::env::var("HOME").ok();
    detect_channel_from(
        launcher,
        exe.as_deref(),
        install_dir.as_deref(),
        home.as_deref(),
    )
}

/// Strips a single leading 'v'/'V' (GitHub tag names look like "v0.1.5";
/// `CARGO_PKG_VERSION` does not).
fn strip_v_prefix(v: &str) -> &str {
    v.strip_prefix('v').or_else(|| v.strip_prefix('V')).unwrap_or(v)
}

/// Parses up to a (major, minor, patch) triplet, ignoring any non-numeric
/// prerelease/build suffix on the segment it trails (e.g. "1.2.3-beta.1" -> (1, 2, 3)).
/// Missing trailing segments default to 0 ("0.1" -> (0, 1, 0)). `None` if the string has
/// no leading digits at all, i.e. isn't version-shaped.
fn parse_version_triplet(v: &str) -> Option<(u64, u64, u64)> {
    let v = strip_v_prefix(v.trim());
    let mut parts = v.split('.');

    let take_numeric_prefix = |s: &str| -> Option<u64> {
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            None
        } else {
            digits.parse().ok()
        }
    };

    let major = take_numeric_prefix(parts.next()?)?;
    let minor = parts.next().and_then(take_numeric_prefix).unwrap_or(0);
    let patch = parts.next().and_then(take_numeric_prefix).unwrap_or(0);
    Some((major, minor, patch))
}

/// True only when `candidate` parses as a strictly newer version than `current`.
/// Malformed input on either side is treated as "not newer" -- this must never claim an
/// update is available on the strength of a string it couldn't actually parse.
fn is_newer_version(current: &str, candidate: &str) -> bool {
    match (parse_version_triplet(current), parse_version_triplet(candidate)) {
        (Some(c), Some(n)) => n > c,
        _ => false,
    }
}

/// Outcome of checking for a newer release. Adjacently tagged so JSON output is a single
/// flat object with a `status` discriminant, matching the rest of this CLI's JSON shapes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable {
        latest_version: String,
        release_url: String,
    },
    CheckFailed {
        reason: String,
    },
    Skipped,
}

/// Pure decision logic: given the current version and an already-resolved fetch outcome,
/// decide the update status. No I/O -- the entire live-check code path is unit-testable
/// through this function by injecting a canned `Ok`/`Err`, without ever making a real
/// network call.
fn evaluate_update_status(
    current_version: &str,
    fetch_result: Result<(String, String), String>,
) -> UpdateStatus {
    match fetch_result {
        Ok((latest, url)) => {
            if is_newer_version(current_version, &latest) {
                UpdateStatus::UpdateAvailable {
                    latest_version: latest,
                    release_url: url,
                }
            } else {
                UpdateStatus::UpToDate
            }
        }
        Err(reason) => UpdateStatus::CheckFailed { reason },
    }
}

/// Fetches the latest published GitHub release's version and URL. Real network I/O --
/// deliberately not exercised by the automated test suite (see the module doc comment for
/// the manual verification already run against the live repo). Every failure mode
/// (unreachable, timed out, rate-limited, malformed response) becomes a plain
/// `Err(String)` for `evaluate_update_status` to turn into a graceful `CheckFailed` --
/// never a panic, never a fatal error for the calling command.
fn fetch_latest_release() -> Result<(String, String), String> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start async runtime: {e}"))?;

    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(UPDATE_CHECK_TIMEOUT)
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;

        let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases/latest");
        let response = client
            .get(&url)
            .header("User-Agent", "agentworth-cli-update-check")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| format!("network request failed: {e}"))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("failed to read GitHub API response: {e}"))?;

        if !status.is_success() {
            let message = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(format!("GitHub API error: {message}"));
        }

        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("failed to parse GitHub API response: {e}"))?;

        let tag = body
            .get("tag_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "GitHub API response missing tag_name".to_string())?;
        let html_url = body
            .get("html_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("https://github.com/{REPO_SLUG}/releases/latest"));

        Ok((strip_v_prefix(tag).to_string(), html_url))
    })
}

fn update_status_for(current_version: &str, offline: bool) -> UpdateStatus {
    if offline {
        UpdateStatus::Skipped
    } else {
        evaluate_update_status(current_version, fetch_latest_release())
    }
}

/// How this binary was installed, in one phrase. Pure formatting, so both the version and
/// the update screens read the same sentence off the same branch.
fn installed_via(launcher: &LauncherInfo, channel: Channel) -> String {
    match channel {
        Channel::Npm => match &launcher.npm_package_version {
            Some(v) => format!("npm (agentworth@{v})"),
            None => "npm".to_string(),
        },
        Channel::InstallSh => "install.sh (agentworth.dev/install.sh)".to_string(),
        Channel::Unknown => {
            "unknown (a direct binary, cargo install, or a source build)".to_string()
        }
    }
}

/// The update check as one line plus the role it should be painted in.
fn update_status_line(current: &str, status: &UpdateStatus) -> (String, Role) {
    match status {
        UpdateStatus::UpToDate => (format!("up to date ({current})"), Role::Verified),
        UpdateStatus::UpdateAvailable { latest_version, .. } => (
            format!("{latest_version} available (you have {current})"),
            Role::Warn,
        ),
        UpdateStatus::CheckFailed { reason } => {
            (format!("couldn't check ({reason})"), Role::Unverified)
        }
        UpdateStatus::Skipped => ("skipped (--offline)".to_string(), Role::Unverified),
    }
}

/// The parser version each registered adapter writes into `sessions.parser_version`. A row
/// indexed below one of these numbers gets reparsed on the next scan, so this is the number
/// that explains why a scan did work when nothing on disk changed.
fn parser_versions() -> Vec<(&'static str, i64)> {
    agentworth_adapters::all_adapters()
        .iter()
        .map(|a| (a.name(), a.parser_version()))
        .collect()
}

/// Where this invocation would read its index from: the explicit `--db-path` if one was
/// given, otherwise the same default `Storage::open_default` resolves.
fn index_path(db_path: Option<PathBuf>) -> String {
    match db_path {
        Some(p) => p.to_string_lossy().to_string(),
        None => agentworth_storage::default_db_dir()
            .map(|d| d.join("agentworth.db").to_string_lossy().to_string())
            .unwrap_or_else(|_| "unresolved".to_string()),
    }
}

/// `archie update`'s headline, and the role it is painted in. Pure formatting; every branch
/// is directly unit-testable against the exact returned string.
fn update_headline(current: &str, status: &UpdateStatus) -> (String, Role) {
    match status {
        UpdateStatus::UpToDate => (
            format!("You're on the latest version ({current})."),
            Role::Verified,
        ),
        UpdateStatus::UpdateAvailable { latest_version, .. } => (
            format!("A newer version is available: {latest_version} (you have {current})."),
            Role::Warn,
        ),
        UpdateStatus::CheckFailed { reason } => (
            format!("Couldn't check for updates ({reason})."),
            Role::Unverified,
        ),
        UpdateStatus::Skipped => (
            "Update check skipped (--offline).".to_string(),
            Role::Unverified,
        ),
    }
}

/// Why `npx -y agentworth@latest` is printed next to the global install, rather than the
/// bare `npx agentworth` every doc used to show.
const NPX_NOTE: &str = "A bare `npx agentworth` reuses whatever npx already cached, which \
    can be several releases behind; the `@latest` form asks the registry every run.";

/// Every real install channel, the one this binary came from first. `update` prints them
/// and stops; it never runs any of them. An npm-managed binary is npm's to replace, and a
/// binary installed any other way has no single correct self-replace strategy this process
/// could safely pick from inside itself.
///
/// The other channels are listed rather than hidden: a person on one channel routinely
/// wants off it, and a screen that shows only the channel it detected cannot say how.
fn update_advice(channel: Channel, status: &UpdateStatus) -> Vec<UpdateChannelView> {
    let UpdateStatus::UpdateAvailable { release_url, .. } = status else {
        return Vec::new();
    };

    let mut all = vec![
        UpdateChannelView {
            name: "install.sh",
            tag: "",
            lines: vec!["curl -fsSL https://agentworth.dev/install.sh | sh".to_string()],
            note: None,
        },
        UpdateChannelView {
            name: "npm",
            tag: "",
            lines: vec![
                "npm install -g agentworth@latest".to_string(),
                "npx -y agentworth@latest".to_string(),
            ],
            note: Some(NPX_NOTE),
        },
        UpdateChannelView {
            name: "download",
            tag: "",
            lines: vec![release_url.clone()],
            note: None,
        },
        UpdateChannelView {
            name: "from source",
            tag: "",
            lines: vec!["cargo install --path apps/cli".to_string()],
            note: None,
        },
    ];

    let Some(detected) = channel.name() else {
        // Nothing was detected, so nothing is "other" -- the list stays untagged.
        return all;
    };
    if let Some(i) = all.iter().position(|c| c.name == detected) {
        let mine = all.remove(i);
        all.insert(0, mine);
        all[0].tag = "this install";
        if let Some(next) = all.get_mut(1) {
            next.tag = "other ways";
        }
    }
    all
}

/// The same channels as the printed screen, off the same list, so the two cannot drift.
fn update_advice_json(advice: &[UpdateChannelView]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for channel in advice {
        map.insert(channel.name.replace([' ', '.'], "_"), json!(channel.lines));
    }
    serde_json::Value::Object(map)
}

fn launcher_json(launcher: &LauncherInfo) -> serde_json::Value {
    json!({
        "npm_launcher_active": launcher.npm_launcher_active,
        "npm_package_version": launcher.npm_package_version,
    })
}

/// Execute `archie version`: what this build is, the name it answers to, where its index
/// lives, and which parser version each adapter is on -- one block, no network unless the
/// update check is allowed to run.
pub fn run_version_command(
    json_output: bool,
    offline: bool,
    db_path: Option<PathBuf>,
    invoked_as: &str,
    ui: &Ui,
) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let launcher = detect_launcher();
    let channel = detect_channel(&launcher);
    let status = update_status_for(current, offline);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "version": current,
                "channel": channel,
                "launcher": launcher_json(&launcher),
                "update_check": status,
            }))?
        );
        return Ok(());
    }

    let (update_line, update_role) = update_status_line(current, &status);
    let index = index_path(db_path);
    let via = installed_via(&launcher, channel);
    print!(
        "{}",
        views::version(
            ui,
            &VersionView {
                version: current,
                invoked_as,
                index_path: &index,
                installed_via: &via,
                update_line: &update_line,
                update_role,
                parsers: parser_versions(),
            }
        )
    );
    Ok(())
}

/// Execute `archie update`. Prints the install line for the channel this binary came from
/// and exits -- it never runs npm, cargo, or any other package manager itself.
pub fn run_update_command(json_output: bool, offline: bool, ui: &Ui) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let launcher = detect_launcher();
    let channel = detect_channel(&launcher);
    let status = update_status_for(current, offline);
    let advice = update_advice(channel, &status);

    if json_output {
        let mut payload = json!({
            "version": current,
            "channel": channel,
            "launcher": launcher_json(&launcher),
            "update_check": status,
        });
        if !advice.is_empty() {
            payload["advice"] = update_advice_json(&advice);
        }
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let (headline, headline_role) = update_headline(current, &status);
    print!(
        "{}",
        views::update(
            ui,
            &UpdateView {
                version: current,
                headline: &headline,
                headline_role,
                advice,
            }
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_version_triplet / is_newer_version ----

    #[test]
    fn test_parse_version_triplet_basic() {
        assert_eq!(parse_version_triplet("0.1.5"), Some((0, 1, 5)));
        assert_eq!(parse_version_triplet("1.2.3"), Some((1, 2, 3)));
    }

    #[test]
    fn test_parse_version_triplet_strips_v_prefix() {
        assert_eq!(parse_version_triplet("v0.1.9"), Some((0, 1, 9)));
        assert_eq!(parse_version_triplet("V2.0.0"), Some((2, 0, 0)));
    }

    #[test]
    fn test_parse_version_triplet_pads_missing_segments() {
        assert_eq!(parse_version_triplet("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version_triplet("2"), Some((2, 0, 0)));
    }

    #[test]
    fn test_parse_version_triplet_ignores_prerelease_suffix() {
        assert_eq!(parse_version_triplet("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version_triplet("1.2.3+build.5"), Some((1, 2, 3)));
    }

    #[test]
    fn test_parse_version_triplet_rejects_non_version_strings() {
        assert_eq!(parse_version_triplet("not-a-version"), None);
        assert_eq!(parse_version_triplet(""), None);
        assert_eq!(parse_version_triplet("v"), None);
    }

    #[test]
    fn test_is_newer_version_real_world_case() {
        // The actual live state confirmed 2026-09-01: npm/GitHub both already serve
        // 0.1.9 while this checkout's Cargo.toml still says 0.1.5 (the final version
        // bump for this session's work hasn't been cut yet). Pins down the exact
        // real-world comparison this feature has to get right on day one.
        assert!(is_newer_version("0.1.5", "v0.1.9"));
    }

    #[test]
    fn test_is_newer_version_equal_is_not_newer() {
        assert!(!is_newer_version("0.1.5", "0.1.5"));
        assert!(!is_newer_version("0.1.5", "v0.1.5"));
    }

    #[test]
    fn test_is_newer_version_older_is_not_newer() {
        assert!(!is_newer_version("0.1.5", "0.1.4"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
    }

    #[test]
    fn test_is_newer_version_minor_and_major_bumps() {
        assert!(is_newer_version("0.1.5", "0.2.0"));
        assert!(is_newer_version("0.9.9", "1.0.0"));
    }

    #[test]
    fn test_is_newer_version_never_claims_update_on_malformed_input() {
        assert!(!is_newer_version("0.1.5", "not-a-version"));
        assert!(!is_newer_version("garbage", "1.0.0"));
        assert!(!is_newer_version("garbage", "also-garbage"));
    }

    // ---- evaluate_update_status (the injectable live-check seam) ----

    #[test]
    fn test_evaluate_update_status_reports_update_available() {
        let result = evaluate_update_status(
            "0.1.5",
            Ok((
                "0.1.9".to_string(),
                "https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.9".to_string(),
            )),
        );
        assert_eq!(
            result,
            UpdateStatus::UpdateAvailable {
                latest_version: "0.1.9".to_string(),
                release_url: "https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.9"
                    .to_string(),
            }
        );
    }

    #[test]
    fn test_evaluate_update_status_reports_up_to_date() {
        let result =
            evaluate_update_status("0.1.5", Ok(("0.1.5".to_string(), "https://example.com".to_string())));
        assert_eq!(result, UpdateStatus::UpToDate);
    }

    #[test]
    fn test_evaluate_update_status_up_to_date_when_remote_is_older() {
        // A dev build ahead of the last tag must never be told to "update" backwards.
        let result =
            evaluate_update_status("0.2.0", Ok(("0.1.9".to_string(), "https://example.com".to_string())));
        assert_eq!(result, UpdateStatus::UpToDate);
    }

    #[test]
    fn test_evaluate_update_status_reports_check_failed_without_touching_network() {
        let result = evaluate_update_status("0.1.5", Err("simulated timeout".to_string()));
        assert_eq!(
            result,
            UpdateStatus::CheckFailed { reason: "simulated timeout".to_string() }
        );
    }

    // ---- launcher detection ----

    #[test]
    fn test_detect_launcher_active_with_npm_version() {
        let info = detect_launcher_from(Some("1"), Some("0.1.9"));
        assert!(info.npm_launcher_active);
        assert_eq!(info.npm_package_version, Some("0.1.9".to_string()));
    }

    #[test]
    fn test_detect_launcher_inactive_when_unset() {
        let info = detect_launcher_from(None, None);
        assert!(!info.npm_launcher_active);
        assert_eq!(info.npm_package_version, None);
    }

    #[test]
    fn test_detect_launcher_requires_exact_literal_one() {
        // resolver.js only ever writes the literal string "1" -- anything else (including
        // "true" or "0") must not be treated as an active launcher.
        assert!(!detect_launcher_from(Some("true"), None).npm_launcher_active);
        assert!(!detect_launcher_from(Some("0"), None).npm_launcher_active);
    }

    // ---- channel detection ----

    fn direct() -> LauncherInfo {
        LauncherInfo { npm_launcher_active: false, npm_package_version: None }
    }

    #[test]
    fn test_channel_is_install_sh_under_the_install_dir_env() {
        let channel = detect_channel_from(
            &direct(),
            Some(Path::new("/opt/aw/bin/archie")),
            Some("/opt/aw/bin"),
            Some("/home/dev"),
        );
        assert_eq!(channel, Channel::InstallSh);
    }

    /// `AGENTWORTH_INSTALL_DIR` is read by install.sh at install time and is usually gone
    /// from the shell that later runs `update`, so the default destination has to be
    /// recognised on its own.
    #[test]
    fn test_channel_is_install_sh_under_the_default_local_bin() {
        let channel = detect_channel_from(
            &direct(),
            Some(Path::new("/home/dev/.local/bin/archie")),
            None,
            Some("/home/dev"),
        );
        assert_eq!(channel, Channel::InstallSh);
    }

    #[test]
    fn test_channel_is_unknown_for_a_binary_somewhere_else() {
        let channel = detect_channel_from(
            &direct(),
            Some(Path::new("/home/dev/code/agentworth/target/debug/archie")),
            None,
            Some("/home/dev"),
        );
        assert_eq!(channel, Channel::Unknown);
        // A path that merely starts with the same characters is not under the directory.
        assert_eq!(
            detect_channel_from(
                &direct(),
                Some(Path::new("/home/dev/.local/binaries/archie")),
                None,
                Some("/home/dev"),
            ),
            Channel::Unknown
        );
    }

    #[test]
    fn test_channel_is_unknown_when_the_exe_path_cannot_be_resolved() {
        assert_eq!(
            detect_channel_from(&direct(), None, Some("/opt/aw/bin"), Some("/home/dev")),
            Channel::Unknown
        );
    }

    /// The wrapper set its marker on this very process; a path prefix is circumstantial
    /// next to that, so npm wins even for a binary sitting under the install dir.
    #[test]
    fn test_npm_marker_beats_the_install_dir_path() {
        let launcher =
            LauncherInfo { npm_launcher_active: true, npm_package_version: Some("0.1.17".into()) };
        let channel = detect_channel_from(
            &launcher,
            Some(Path::new("/home/dev/.local/bin/archie")),
            None,
            Some("/home/dev"),
        );
        assert_eq!(channel, Channel::Npm);
    }

    // ---- text rendering (every branch, no I/O) ----

    fn test_ui() -> Ui {
        Ui::new(80, crate::ui::ColorMode::None, true)
    }

    fn version_screen(current: &str, launcher: &LauncherInfo, status: &UpdateStatus) -> String {
        let (update_line, update_role) = update_status_line(current, status);
        let channel = if launcher.npm_launcher_active { Channel::Npm } else { Channel::Unknown };
        let via = installed_via(launcher, channel);
        views::version(
            &test_ui(),
            &VersionView {
                version: current,
                invoked_as: "archie",
                index_path: "/tmp/index.db",
                installed_via: &via,
                update_line: &update_line,
                update_role,
                parsers: parser_versions(),
            },
        )
    }

    fn update_screen(current: &str, channel: Channel, status: &UpdateStatus) -> String {
        let (headline, headline_role) = update_headline(current, status);
        views::update(
            &test_ui(),
            &UpdateView {
                version: current,
                headline: &headline,
                headline_role,
                advice: update_advice(channel, status),
            },
        )
    }

    fn available() -> UpdateStatus {
        UpdateStatus::UpdateAvailable {
            latest_version: "0.1.9".to_string(),
            release_url: "https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.9"
                .to_string(),
        }
    }

    #[test]
    fn test_version_report_up_to_date() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let report = version_screen("0.1.5", &launcher, &UpdateStatus::UpToDate);
        assert!(report.contains("0.1.5"));
        assert!(report.contains("up to date"));
        assert!(report.contains("a direct binary, cargo install, or a source build"));
    }

    /// The one block `archie version` exists to print: what this build is, the name it was
    /// invoked as, where the index lives, and every adapter's parser version.
    #[test]
    fn test_version_report_names_the_binary_the_index_and_every_parser() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let report = version_screen("0.1.5", &launcher, &UpdateStatus::Skipped);
        assert!(report.contains("invoked as"), "{report}");
        assert!(report.contains("archie"), "{report}");
        assert!(report.contains("index"), "{report}");
        assert!(report.contains("index.db"), "{report}");
        let parsers = parser_versions();
        assert!(!parsers.is_empty(), "no adapter registered");
        for (name, version) in parsers {
            assert!(report.contains(name), "parser {name} missing from:\n{report}");
            assert!(
                report.contains(&format!("v{version}")),
                "parser version v{version} missing from:\n{report}"
            );
        }
    }

    #[test]
    fn test_version_report_npm_launcher_active() {
        let launcher =
            LauncherInfo { npm_launcher_active: true, npm_package_version: Some("0.1.9".to_string()) };
        let report = version_screen("0.1.5", &launcher, &UpdateStatus::Skipped);
        assert!(report.contains("npm (agentworth@0.1.9)"));
        assert!(report.contains("skipped"));
    }

    #[test]
    fn test_version_report_check_failed_is_graceful() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let report = version_screen(
            "0.1.5",
            &launcher,
            &UpdateStatus::CheckFailed { reason: "network request failed: timed out".to_string() },
        );
        assert!(report.contains("couldn't check"));
        assert!(report.contains("timed out"));
    }

    #[test]
    fn test_update_report_up_to_date_has_no_advice() {
        let report = update_screen("0.1.9", Channel::Unknown, &UpdateStatus::UpToDate);
        assert!(report.contains("You're on the latest version"));
        assert!(!report.contains("npm install -g"));
    }

    /// The detected channel leads and says so; the rest follow under one "other ways" tag.
    #[test]
    fn test_update_report_puts_the_detected_channel_first() {
        let report = update_screen("0.1.5", Channel::InstallSh, &available());
        let install_at = report.find("install.sh").expect("install.sh is listed");
        let npm_at = report.find("npm install -g").expect("npm is listed");
        assert!(install_at < npm_at, "the detected channel must lead:\n{report}");
        assert!(report.contains("this install"), "{report}");
        assert!(report.contains("other ways"), "{report}");

        let npm_first = update_screen("0.1.5", Channel::Npm, &available());
        assert!(
            npm_first.find("npm install -g").unwrap()
                < npm_first.find("curl -fsSL").unwrap(),
            "an npm-launched binary must be told about npm first:\n{npm_first}"
        );
    }

    /// Nothing detected means nothing is "other", so the list ships untagged rather than
    /// claiming a channel it did not observe.
    #[test]
    fn test_update_report_tags_nothing_when_the_channel_is_unknown() {
        let report = update_screen("0.1.5", Channel::Unknown, &available());
        assert!(!report.contains("this install"), "{report}");
        assert!(!report.contains("other ways"), "{report}");
        assert!(report.contains("curl -fsSL https://agentworth.dev/install.sh | sh"));
    }

    /// The npx line and the sentence that explains why it carries `@latest`: a bare
    /// `npx agentworth` runs whatever npx already cached, which is how a months-old
    /// binary answers for a fresh install.
    #[test]
    fn test_update_report_prints_both_npm_lines_and_the_npx_caveat() {
        let report = update_screen("0.1.5", Channel::Npm, &available());
        assert!(report.contains("npm install -g agentworth@latest"), "{report}");
        assert!(report.contains("npx -y agentworth@latest"), "{report}");
        assert!(report.contains("reuses whatever npx already cached"), "{report}");
    }

    /// Every listed channel has been checked to work. The two that never did must not
    /// come back -- see the module doc comment.
    #[test]
    fn test_update_report_never_suggests_a_channel_that_does_not_exist() {
        for channel in [Channel::Npm, Channel::InstallSh, Channel::Unknown] {
            let report = update_screen("0.1.5", channel, &available());
            assert!(!report.contains("brew"), "{report}");
            assert!(!report.contains("cargo install agentworth-cli"), "{report}");
        }
    }

    /// The JSON is built off the same channel list the screen prints, so a channel can
    /// never appear on one and not the other.
    #[test]
    fn test_update_advice_json_carries_the_same_channels_as_the_screen() {
        let advice = update_advice(Channel::InstallSh, &available());
        let json = update_advice_json(&advice);
        let map = json.as_object().expect("an object");
        assert_eq!(map.len(), advice.len());
        assert_eq!(map["install_sh"][0], "curl -fsSL https://agentworth.dev/install.sh | sh");
        assert_eq!(map["npm"][1], "npx -y agentworth@latest");
        assert_eq!(map["from_source"][0], "cargo install --path apps/cli");
        assert!(map.contains_key("download"));
    }

    /// The release URL must reach the terminal whole. It used to render through
    /// `Ui::leaders`, whose value budget under "  download" is 65 columns at MAX_CONTENT --
    /// and `https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.17` is 66. So
    /// from the first two-digit patch release, the only actionable line on this screen would
    /// have printed as a truncated, dead link. Pinned verbatim, at both a wide and a narrow
    /// terminal, with the digits that trip the boundary.
    #[test]
    fn test_update_prints_the_release_url_whole_not_truncated() {
        const URL: &str = "https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.17";
        assert_eq!(URL.len(), 66, "the boundary this test exists for has moved");

        let status = UpdateStatus::UpdateAvailable {
            latest_version: "0.1.17".to_string(),
            release_url: URL.to_string(),
        };
        let (headline, headline_role) = update_headline("0.1.16", &status);
        let view = UpdateView {
            version: "0.1.16",
            headline: &headline,
            headline_role,
            advice: update_advice(Channel::InstallSh, &status),
        };

        // Wide enough for one line: the URL appears verbatim, on a line of its own.
        let wide = views::update(&Ui::new(80, crate::ui::ColorMode::None, true), &view);
        assert!(wide.contains(URL), "the release URL was cut:\n{wide}");
        assert!(!wide.contains(".."), "something on this screen was truncated:\n{wide}");

        // Narrow enough to force a wrap: no character may be lost, so the URL with the
        // line breaks taken back out is still exactly the URL.
        let narrow = views::update(&Ui::new(40, crate::ui::ColorMode::None, true), &view);
        let rejoined: String = narrow.lines().map(str::trim).collect::<Vec<_>>().join("");
        assert!(
            rejoined.contains(URL),
            "the URL did not survive wrapping at 40 columns:\n{narrow}"
        );
        for line in narrow.lines() {
            assert!(
                crate::ui::display_width(line) <= 38,
                "a line outgrew the narrow terminal:\n{line}"
            );
        }
    }

    /// `update` advises; it never executes. Nothing on this path spawns a process.
    #[test]
    fn test_update_advice_is_text_only_and_empty_when_there_is_nothing_to_do() {
        assert!(update_advice(Channel::Unknown, &UpdateStatus::UpToDate).is_empty());
        assert!(update_advice(Channel::Npm, &UpdateStatus::Skipped).is_empty());
        assert!(update_advice(
            Channel::InstallSh,
            &UpdateStatus::CheckFailed { reason: "offline".to_string() }
        )
        .is_empty());
    }

    #[test]
    fn test_update_status_for_offline_never_touches_network() {
        // No mocking needed: offline=true short-circuits before fetch_latest_release is
        // ever called, so this is deterministic and instant with no network access.
        assert_eq!(update_status_for("0.1.5", true), UpdateStatus::Skipped);
    }
}
