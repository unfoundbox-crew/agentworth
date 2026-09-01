//! Persisted CLI configuration defaults for AgentWorth.
//!
//! Subcommand: `agentworth config get <key>` / `set <key> <value>` / `list`.
//!
//! Stores user-level defaults (output format, result limit, `usage` lookback period) in
//! `config.toml` next to the SQLite index (`~/.agentworth/agentworth.db`, see
//! `agentworth_storage::default_db_dir`) so the two files share one base-directory
//! convention. An explicit CLI flag always overrides a persisted default; the config only
//! fills in values the caller didn't pass on the command line.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Persisted user-level CLI defaults. Every field is optional: `None` means "not set,
/// fall back to the command's own built-in default."
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CliConfig {
    /// Default for every subcommand's `--json` flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<bool>,
    /// Default for the `--limit` flag shared by `traces`, `search`, `usage`,
    /// `blind-spots`, `threat-digest`, and `recall`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Default lookback window (`day` | `week` | `month`) for `usage --period`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
}

const CONFIG_KEYS: &[&str] = &["json", "limit", "period"];
const VALID_PERIODS: &[&str] = &["day", "week", "month"];

/// Path to the persisted config file: `<default_db_dir>/config.toml`, i.e. right beside
/// `agentworth.db`. Honors `AGENTWORTH_CONFIG_PATH` first, purely as an escape hatch for
/// tests (and anyone who wants their config elsewhere) — the default always resolves
/// through the same base-directory logic `agentworth-storage` uses for the index db.
pub fn config_file_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("AGENTWORTH_CONFIG_PATH") {
        return Ok(PathBuf::from(path));
    }
    Ok(agentworth_storage::default_db_dir()?.join("config.toml"))
}

/// Load persisted config from the default location. A missing file is not an error —
/// it just means no defaults have been set yet.
pub fn load_config() -> Result<CliConfig> {
    load_config_from(&config_file_path()?)
}

fn load_config_from(path: &Path) -> Result<CliConfig> {
    if !path.exists() {
        return Ok(CliConfig::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {:?}", path))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config file at {:?}", path))
}

fn save_config_to(path: &Path, cfg: &CliConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory at {:?}", parent))?;
    }
    let raw = toml::to_string_pretty(cfg).context("failed to serialize config")?;
    std::fs::write(path, raw)
        .with_context(|| format!("failed to write config file at {:?}", path))
}

/// Resolve the effective `--json` flag. An explicit `--json` always wins; otherwise fall
/// back to the persisted default; `no_json` is the CLI-wide escape hatch for a persisted
/// default of `true` (there is no bare "--json false", since `--json` is a plain switch).
pub fn resolve_json(explicit: bool, no_json: bool, config_default: Option<bool>) -> bool {
    explicit || (!no_json && config_default.unwrap_or(false))
}

/// Resolve an effective `--limit`: explicit flag wins, else the persisted default, else
/// the command's own built-in default.
pub fn resolve_limit(
    explicit: Option<usize>,
    config_default: Option<usize>,
    builtin_default: usize,
) -> usize {
    explicit.unwrap_or_else(|| config_default.unwrap_or(builtin_default))
}

/// Resolve an effective `--period` for `usage`, re-validating a config-sourced value
/// against the same allowed set clap enforces on the flag itself (a hand-edited config
/// file can't smuggle in a bogus period).
pub fn resolve_period(
    explicit: Option<String>,
    config_default: Option<String>,
    builtin_default: &str,
) -> Result<String> {
    let value = explicit.unwrap_or_else(|| config_default.unwrap_or_else(|| builtin_default.to_string()));
    if !VALID_PERIODS.contains(&value.as_str()) {
        return Err(anyhow!(
            "invalid persisted config value for 'period': {:?} (expected one of: {})",
            value,
            VALID_PERIODS.join(", ")
        ));
    }
    Ok(value)
}

fn parse_bool_value(raw: &str) -> Result<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(anyhow!(
            "invalid value {:?} for 'json': expected 'true' or 'false'",
            raw
        )),
    }
}

fn parse_limit_value(raw: &str) -> Result<usize> {
    raw.parse::<usize>()
        .map_err(|_| anyhow!("invalid value {:?} for 'limit': expected a non-negative integer", raw))
}

fn parse_period_value(raw: &str) -> Result<String> {
    if VALID_PERIODS.contains(&raw) {
        Ok(raw.to_string())
    } else {
        Err(anyhow!(
            "invalid value {:?} for 'period': expected one of: {}",
            raw,
            VALID_PERIODS.join(", ")
        ))
    }
}

fn unknown_key_error(key: &str) -> anyhow::Error {
    anyhow!("unknown config key {:?}. Valid keys: {}", key, CONFIG_KEYS.join(", "))
}

/// Execute `agentworth config list`.
pub fn run_config_list(json: bool) -> Result<()> {
    run_config_list_at(&config_file_path()?, json)
}

fn run_config_list_at(path: &Path, json: bool) -> Result<()> {
    let cfg = load_config_from(path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "config_path": path.to_string_lossy(),
                "json": cfg.json,
                "limit": cfg.limit,
                "period": cfg.period,
            }))?
        );
        return Ok(());
    }

    println!("Config file: {}", path.display());
    println!();
    print_kv("json", cfg.json.map(|v| v.to_string()));
    print_kv("limit", cfg.limit.map(|v| v.to_string()));
    print_kv("period", cfg.period.clone());
    Ok(())
}

fn print_kv(key: &str, value: Option<String>) {
    match value {
        Some(v) => println!("  {:<8} = {}", key, v),
        None => println!("  {:<8} = (not set)", key),
    }
}

/// Execute `agentworth config get <key>`.
pub fn run_config_get(key: &str, json: bool) -> Result<()> {
    run_config_get_at(&config_file_path()?, key, json)
}

fn run_config_get_at(path: &Path, key: &str, json: bool) -> Result<()> {
    let cfg = load_config_from(path)?;
    let value: Option<String> = match key {
        "json" => cfg.json.map(|v| v.to_string()),
        "limit" => cfg.limit.map(|v| v.to_string()),
        "period" => cfg.period.clone(),
        other => return Err(unknown_key_error(other)),
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "key": key, "value": value }))?
        );
    } else {
        match value {
            Some(v) => println!("{}", v),
            None => println!("(not set)"),
        }
    }
    Ok(())
}

/// Execute `agentworth config set <key> <value>`.
pub fn run_config_set(key: &str, value: &str, json: bool) -> Result<()> {
    run_config_set_at(&config_file_path()?, key, value, json)
}

fn run_config_set_at(path: &Path, key: &str, value: &str, json: bool) -> Result<()> {
    let mut cfg = load_config_from(path)?;
    match key {
        "json" => cfg.json = Some(parse_bool_value(value)?),
        "limit" => cfg.limit = Some(parse_limit_value(value)?),
        "period" => cfg.period = Some(parse_period_value(value)?),
        other => return Err(unknown_key_error(other)),
    }
    save_config_to(path, &cfg)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "key": key, "value": value, "status": "saved" }))?
        );
    } else {
        println!("Saved {} = {}", key, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_round_trip_save_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let cfg = CliConfig {
            json: Some(true),
            limit: Some(42),
            period: Some("week".to_string()),
        };

        save_config_to(&path, &cfg).unwrap();
        let loaded = load_config_from(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does_not_exist.toml");
        let loaded = load_config_from(&path).unwrap();
        assert_eq!(loaded, CliConfig::default());
    }

    #[test]
    fn test_partial_config_leaves_other_keys_unset() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "limit = 7\n").unwrap();

        let loaded = load_config_from(&path).unwrap();
        assert_eq!(loaded.limit, Some(7));
        assert_eq!(loaded.json, None);
        assert_eq!(loaded.period, None);
    }

    #[test]
    fn test_resolve_json_explicit_flag_overrides_config_default() {
        // Config says "always JSON" but the caller explicitly asked for text via
        // --no-json: the explicit flag must win.
        assert!(!resolve_json(false, true, Some(true)));
        // No explicit flag, no opt-out: the persisted default applies.
        assert!(resolve_json(false, false, Some(true)));
        // Explicit --json always wins regardless of config.
        assert!(resolve_json(true, false, Some(false)));
        // Nothing set anywhere: falls back to the historical default (text).
        assert!(!resolve_json(false, false, None));
    }

    #[test]
    fn test_resolve_limit_explicit_flag_overrides_config_default() {
        // Explicit flag (7) wins over both the persisted default (3) and the
        // command's own built-in default (20).
        assert_eq!(resolve_limit(Some(7), Some(3), 20), 7);
        // No explicit flag: the persisted default applies.
        assert_eq!(resolve_limit(None, Some(3), 20), 3);
        // Nothing persisted either: falls back to the command's built-in default.
        assert_eq!(resolve_limit(None, None, 20), 20);
    }

    #[test]
    fn test_resolve_period_explicit_flag_overrides_config_default_and_validates() {
        assert_eq!(
            resolve_period(Some("month".into()), Some("week".into()), "day").unwrap(),
            "month"
        );
        assert_eq!(
            resolve_period(None, Some("week".into()), "day").unwrap(),
            "week"
        );
        assert_eq!(resolve_period(None, None, "day").unwrap(), "day");
        assert!(resolve_period(None, Some("fortnight".into()), "day").is_err());
    }

    #[test]
    fn test_set_get_list_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        run_config_set_at(&path, "limit", "42", false).unwrap();
        run_config_set_at(&path, "json", "true", false).unwrap();
        run_config_set_at(&path, "period", "week", false).unwrap();

        let cfg = load_config_from(&path).unwrap();
        assert_eq!(cfg.limit, Some(42));
        assert_eq!(cfg.json, Some(true));
        assert_eq!(cfg.period, Some("week".to_string()));

        run_config_get_at(&path, "limit", false).unwrap();
        run_config_list_at(&path, false).unwrap();
        run_config_list_at(&path, true).unwrap();
    }

    #[test]
    fn test_set_rejects_unknown_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let err = run_config_set_at(&path, "bogus", "x", false).unwrap_err();
        assert!(err.to_string().contains("unknown config key"));
    }

    #[test]
    fn test_set_rejects_invalid_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(run_config_set_at(&path, "json", "maybe", false).is_err());
        assert!(run_config_set_at(&path, "limit", "not-a-number", false).is_err());
        assert!(run_config_set_at(&path, "period", "fortnight", false).is_err());
    }

    #[test]
    fn test_get_unset_key_reports_not_set() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // No file written at all yet: every key should read back as unset, not error.
        run_config_get_at(&path, "json", false).unwrap();
        run_config_get_at(&path, "limit", false).unwrap();
        run_config_get_at(&path, "period", false).unwrap();
    }
}
