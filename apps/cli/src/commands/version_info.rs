//! Version reporting and update checking for AgentWorth.
//!
//! Subcommands: `agentworth version [--offline] [--json]` and
//! `agentworth update [--offline] [--json]`.
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
//! Verified live against the real repo (2026-09-01) before choosing which install
//! commands to print: `npm install -g agentworth@latest` and the GitHub Releases page
//! both work (confirmed via the registry and the releases API directly). Three other
//! install methods README.md advertises do NOT actually work and must never be suggested
//! here: `curl -fsSL https://agentworth.dev/install.sh` serves the marketing site's own
//! `index.html` (a plain SPA fallback route, not a shell script -- piping it to `sh` would
//! fail immediately), `brew install unfoundbox-crew/tap/agentworth` 404s (no such tap repo
//! exists), and `cargo install agentworth-cli` has no matching crate on crates.io. None of
//! the three are wired into `.github/workflows/release.yml` either, which only ever
//! publishes to GitHub Releases and npm.

use std::time::Duration;

use anyhow::Result;
use console::style;
use serde::Serialize;
use serde_json::json;

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

/// Renders `agentworth version`'s text-mode report. Pure formatting -- no I/O, no
/// printing -- so every branch (each `UpdateStatus` variant, launcher active/inactive) is
/// directly unit-testable against the exact returned string.
fn format_version_report(current: &str, launcher: &LauncherInfo, status: &UpdateStatus) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("agentworth {}\n", style(current).bold().cyan()));
    out.push('\n');

    match (launcher.npm_launcher_active, &launcher.npm_package_version) {
        (true, Some(v)) => out.push_str(&format!("Installed via: npm (agentworth@{v})\n")),
        (true, None) => out.push_str("Installed via: npm\n"),
        (false, _) => out.push_str(
            "Installed via: unknown (no npm launcher detected -- likely a direct binary, `cargo install`, or a build from source)\n",
        ),
    }

    out.push_str(&format_update_status_line(current, status));
    out
}

fn format_update_status_line(current: &str, status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::UpToDate => {
            format!("Update check:  {}\n", style(format!("up to date ({current})")).green())
        }
        UpdateStatus::UpdateAvailable { latest_version, .. } => format!(
            "Update check:  {}\n",
            style(format!("{latest_version} available (you have {current})"))
                .yellow()
                .bold()
        ),
        UpdateStatus::CheckFailed { reason } => {
            format!("Update check:  {}\n", style(format!("couldn't check ({reason})")).dim())
        }
        UpdateStatus::Skipped => {
            format!("Update check:  {}\n", style("skipped (--offline)").dim())
        }
    }
}

/// Renders `agentworth update`'s text-mode report, including the actionable "how to
/// update" guidance when a newer version exists. Pure formatting, same testing rationale
/// as `format_version_report`.
fn format_update_report(current: &str, launcher: &LauncherInfo, status: &UpdateStatus) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("agentworth {}\n", style(current).bold().cyan()));
    out.push('\n');

    match status {
        UpdateStatus::UpToDate => {
            out.push_str(&format!(
                "{}\n",
                style(format!("You're on the latest version ({current}).")).green().bold()
            ));
        }
        UpdateStatus::UpdateAvailable { latest_version, release_url } => {
            out.push_str(&format!(
                "{}\n",
                style(format!(
                    "A newer version is available: {latest_version} (you have {current})"
                ))
                .yellow()
                .bold()
            ));
            out.push('\n');
            out.push_str(&format_update_advice(launcher, release_url));
        }
        UpdateStatus::CheckFailed { reason } => {
            out.push_str("Couldn't check for updates.\n");
            out.push_str(&format!("  ({reason})\n"));
        }
        UpdateStatus::Skipped => {
            out.push_str("Update check skipped (--offline).\n");
        }
    }
    out.push('\n');
    out
}

fn format_update_advice(launcher: &LauncherInfo, release_url: &str) -> String {
    let mut out = String::new();
    if launcher.npm_launcher_active {
        out.push_str("This binary was launched via the npm package. To update, run:\n\n");
        out.push_str(&format!(
            "  {}\n",
            style("npm install -g agentworth@latest").bold().cyan()
        ));
    } else {
        out.push_str(
            "AgentWorth doesn't self-update. Get the new version with whichever of\nthese matches how you installed it:\n\n",
        );
        out.push_str(&format!(
            "  {} {}\n",
            style("npm:").bold(),
            style("npm install -g agentworth@latest").cyan()
        ));
        out.push_str(&format!(
            "  {} {}\n",
            style("Download:").bold(),
            style(release_url).cyan()
        ));
        out.push_str(&format!(
            "  {} {}\n",
            style("From source:").bold(),
            style("cargo install --path apps/cli").cyan()
        ));
    }
    out
}

fn update_advice_json(launcher: &LauncherInfo, release_url: &str) -> serde_json::Value {
    if launcher.npm_launcher_active {
        json!({ "npm": "npm install -g agentworth@latest" })
    } else {
        json!({
            "npm": "npm install -g agentworth@latest",
            "download": release_url,
            "from_source": "cargo install --path apps/cli",
        })
    }
}

fn launcher_json(launcher: &LauncherInfo) -> serde_json::Value {
    json!({
        "npm_launcher_active": launcher.npm_launcher_active,
        "npm_package_version": launcher.npm_package_version,
    })
}

/// Execute `agentworth version`.
pub fn run_version_command(json_output: bool, offline: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let launcher = detect_launcher();
    let status = update_status_for(current, offline);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "version": current,
                "launcher": launcher_json(&launcher),
                "update_check": status,
            }))?
        );
        return Ok(());
    }

    print!("{}", format_version_report(current, &launcher, &status));
    Ok(())
}

/// Execute `agentworth update`.
pub fn run_update_command(json_output: bool, offline: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let launcher = detect_launcher();
    let status = update_status_for(current, offline);

    if json_output {
        let mut payload = json!({
            "version": current,
            "launcher": launcher_json(&launcher),
            "update_check": status,
        });
        if let UpdateStatus::UpdateAvailable { release_url, .. } = &status {
            payload["advice"] = update_advice_json(&launcher, release_url);
        }
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    print!("{}", format_update_report(current, &launcher, &status));
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

    // ---- text rendering (every branch, no I/O) ----

    #[test]
    fn test_format_version_report_up_to_date() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let report = format_version_report("0.1.5", &launcher, &UpdateStatus::UpToDate);
        assert!(report.contains("agentworth 0.1.5"));
        assert!(report.contains("up to date"));
        assert!(report.contains("no npm launcher detected"));
    }

    #[test]
    fn test_format_version_report_npm_launcher_active() {
        let launcher =
            LauncherInfo { npm_launcher_active: true, npm_package_version: Some("0.1.9".to_string()) };
        let report = format_version_report("0.1.5", &launcher, &UpdateStatus::Skipped);
        assert!(report.contains("npm (agentworth@0.1.9)"));
        assert!(report.contains("skipped"));
    }

    #[test]
    fn test_format_version_report_check_failed_is_graceful() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let report = format_version_report(
            "0.1.5",
            &launcher,
            &UpdateStatus::CheckFailed { reason: "network request failed: timed out".to_string() },
        );
        assert!(report.contains("couldn't check"));
        assert!(report.contains("timed out"));
    }

    #[test]
    fn test_format_update_report_up_to_date_has_no_advice() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let report = format_update_report("0.1.9", &launcher, &UpdateStatus::UpToDate);
        assert!(report.contains("You're on the latest version"));
        assert!(!report.contains("npm install -g"));
    }

    #[test]
    fn test_format_update_report_npm_launcher_gets_npm_command() {
        let launcher =
            LauncherInfo { npm_launcher_active: true, npm_package_version: Some("0.1.5".to_string()) };
        let status = UpdateStatus::UpdateAvailable {
            latest_version: "0.1.9".to_string(),
            release_url: "https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.9".to_string(),
        };
        let report = format_update_report("0.1.5", &launcher, &status);
        assert!(report.contains("npm install -g agentworth@latest"));
        assert!(report.contains("launched via the npm package"));
        // Must never suggest the fabricated install methods (verified live 2026-09-01:
        // curl serves HTML not a script, the brew tap 404s, and no `agentworth-cli` crate
        // exists on crates.io -- see the module doc comment).
        assert!(!report.contains("brew"));
        assert!(!report.contains("install.sh"));
    }

    #[test]
    fn test_format_update_report_non_npm_lists_all_real_channels() {
        let launcher = LauncherInfo { npm_launcher_active: false, npm_package_version: None };
        let status = UpdateStatus::UpdateAvailable {
            latest_version: "0.1.9".to_string(),
            release_url: "https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.9".to_string(),
        };
        let report = format_update_report("0.1.5", &launcher, &status);
        assert!(report.contains("npm install -g agentworth@latest"));
        assert!(report.contains("https://github.com/unfoundbox-crew/agentworth/releases/tag/v0.1.9"));
        assert!(report.contains("cargo install --path apps/cli"));
        assert!(!report.contains("brew"));
        assert!(!report.contains("install.sh"));
        assert!(!report.contains("cargo install agentworth-cli"));
    }

    #[test]
    fn test_update_status_for_offline_never_touches_network() {
        // No mocking needed: offline=true short-circuits before fetch_latest_release is
        // ever called, so this is deterministic and instant with no network access.
        assert_eq!(update_status_for("0.1.5", true), UpdateStatus::Skipped);
    }
}
