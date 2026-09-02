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
    /// Default lookback window (`day` | `week` | `month` | `year` | `all`) for `usage --period`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    /// How Archie is drawn wherever the product draws him. A TOML table, so it has to
    /// stay last in this struct -- `toml` refuses to emit a table before a scalar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archie: Option<ArchieConfig>,
}

/// Archie's kit and colourway. See `packages/ui/brand/archie/README.md`; the SVG poses
/// read `data-accessory` and the colourway is a `.archie-cN` class.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchieConfig {
    /// `lamp` (default) | `goggles` | `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessory: Option<String>,
    /// `C1` mono | `C2` dark | `C3` AgentWorth, the default | `C4` quiet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colourway: Option<String>,
}

const CONFIG_KEYS: &[&str] = &[
    "json",
    "limit",
    "period",
    "archie.accessory",
    "archie.colourway",
];
const VALID_PERIODS: &[&str] = &["day", "week", "month", "year", "all"];
const VALID_ACCESSORIES: &[&str] = &["lamp", "goggles", "none"];
const VALID_COLOURWAYS: &[&str] = &["C1", "C2", "C3", "C4"];

// No default constants here on purpose. The CLI stores these two keys but never draws
// the SVG -- the terminal short form's lamp glyph is the state, not an accessory, and it
// is fixed. Unset reads back as null and the surface that renders him applies its own
// default (packages/ui/Archie.tsx). A second copy of "lamp"/"C3" in Rust would be one
// more thing to keep in step for no reader.

/// Canonicalize a `--period` value, accepting the single-letter aliases `d`/`w`/`m`/`y`
/// alongside the full words. `None` means the input matches none of them -- the caller
/// decides how to phrase the error (clap's own message for the flag, a config-specific one
/// for `agentworth config set period`).
pub fn normalize_period(raw: &str) -> Option<&'static str> {
    match raw {
        "day" | "d" => Some("day"),
        "week" | "w" => Some("week"),
        "month" | "m" => Some("month"),
        "year" | "y" => Some("year"),
        "all" => Some("all"),
        _ => None,
    }
}

/// Canonicalize an `archie.accessory` value. Case-insensitive; `None` means no match.
pub fn normalize_accessory(raw: &str) -> Option<&'static str> {
    match raw.to_ascii_lowercase().as_str() {
        "lamp" => Some("lamp"),
        "goggles" => Some("goggles"),
        "none" => Some("none"),
        _ => None,
    }
}

/// Canonicalize an `archie.colourway` value. `c3` and `C3` both mean C3.
pub fn normalize_colourway(raw: &str) -> Option<&'static str> {
    match raw.to_ascii_uppercase().as_str() {
        "C1" => Some("C1"),
        "C2" => Some("C2"),
        "C3" => Some("C3"),
        "C4" => Some("C4"),
        _ => None,
    }
}

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

/// Persist config to the default location. The local API's `POST /api/config` writes
/// through here, so it lands in exactly the file `agentworth config set` writes.
pub fn save_config(cfg: &CliConfig) -> Result<()> {
    save_config_to(&config_file_path()?, cfg)
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
    match normalize_period(&value) {
        Some(canonical) => Ok(canonical.to_string()),
        None => Err(anyhow!(
            "invalid persisted config value for 'period': {:?} (expected one of: {}, or d/w/m/y)",
            value,
            VALID_PERIODS.join(", ")
        )),
    }
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
    normalize_period(raw).map(str::to_string).ok_or_else(|| {
        anyhow!(
            "invalid value {:?} for 'period': expected one of: {}, or d/w/m/y",
            raw,
            VALID_PERIODS.join(", ")
        )
    })
}

fn parse_accessory_value(raw: &str) -> Result<String> {
    normalize_accessory(raw).map(str::to_string).ok_or_else(|| {
        anyhow!(
            "invalid value {:?} for 'archie.accessory': expected one of: {}",
            raw,
            VALID_ACCESSORIES.join(", ")
        )
    })
}

fn parse_colourway_value(raw: &str) -> Result<String> {
    normalize_colourway(raw).map(str::to_string).ok_or_else(|| {
        anyhow!(
            "invalid value {:?} for 'archie.colourway': expected one of: {}",
            raw,
            VALID_COLOURWAYS.join(", ")
        )
    })
}

fn unknown_key_error(key: &str) -> anyhow::Error {
    anyhow!("unknown config key {:?}. Valid keys: {}", key, CONFIG_KEYS.join(", "))
}

/// Every key a caller may set, for a 400 that names the whole vocabulary.
pub fn config_keys() -> &'static [&'static str] {
    CONFIG_KEYS
}

/// Validate `value` and write it into `cfg`. The single validation path: `config set`
/// and `POST /api/config` both come through here, so neither can drift from the other.
pub fn apply_config_value(cfg: &mut CliConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "json" => cfg.json = Some(parse_bool_value(value)?),
        "limit" => cfg.limit = Some(parse_limit_value(value)?),
        "period" => cfg.period = Some(parse_period_value(value)?),
        "archie.accessory" => {
            cfg.archie.get_or_insert_default().accessory =
                Some(parse_accessory_value(value)?)
        }
        "archie.colourway" => {
            cfg.archie.get_or_insert_default().colourway =
                Some(parse_colourway_value(value)?)
        }
        other => return Err(unknown_key_error(other)),
    }
    Ok(())
}

/// Clear one key. `POST /api/config` uses this for an explicit JSON `null`, which is how
/// a caller says "go back to the built-in default" without a second route.
pub fn clear_config_value(cfg: &mut CliConfig, key: &str) -> Result<()> {
    match key {
        "json" => cfg.json = None,
        "limit" => cfg.limit = None,
        "period" => cfg.period = None,
        "archie.accessory" => {
            if let Some(a) = cfg.archie.as_mut() {
                a.accessory = None;
            }
        }
        "archie.colourway" => {
            if let Some(a) = cfg.archie.as_mut() {
                a.colourway = None;
            }
        }
        other => return Err(unknown_key_error(other)),
    }
    Ok(())
}

/// Read one key back as the string form `config get` prints. `None` is "not set".
pub fn config_value(cfg: &CliConfig, key: &str) -> Result<Option<String>> {
    let value = match key {
        "json" => cfg.json.map(|v| v.to_string()),
        "limit" => cfg.limit.map(|v| v.to_string()),
        "period" => cfg.period.clone(),
        "archie.accessory" => cfg.archie.as_ref().and_then(|a| a.accessory.clone()),
        "archie.colourway" => cfg.archie.as_ref().and_then(|a| a.colourway.clone()),
        other => return Err(unknown_key_error(other)),
    };
    Ok(value)
}

/// The whole config as JSON, unset values as `null` rather than absent -- a client
/// reading this should not have to tell "not set" from "key I have never heard of".
///
/// The file's own path is deliberately NOT in here. `GET /api/config` serves this over
/// HTTP, and that path is an absolute home directory: it names the user and it is of no
/// use to a browser. `config list --json` adds it back, where the caller already has the
/// filesystem in front of them.
pub fn config_as_json(cfg: &CliConfig) -> serde_json::Value {
    json!({
        "json": cfg.json,
        "limit": cfg.limit,
        "period": cfg.period,
        "archie.accessory": cfg.archie.as_ref().and_then(|a| a.accessory.clone()),
        "archie.colourway": cfg.archie.as_ref().and_then(|a| a.colourway.clone()),
    })
}

/// Execute `agentworth config list`.
pub fn run_config_list(json: bool) -> Result<()> {
    run_config_list_at(&config_file_path()?, json)
}

fn run_config_list_at(path: &Path, json: bool) -> Result<()> {
    let cfg = load_config_from(path)?;

    if json {
        let mut payload = config_as_json(&cfg);
        payload["config_path"] = json!(path.to_string_lossy());
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("Config file: {}", path.display());
    println!();
    for key in CONFIG_KEYS {
        print_kv(key, config_value(&cfg, key)?);
    }
    Ok(())
}

fn print_kv(key: &str, value: Option<String>) {
    match value {
        Some(v) => println!("  {:<16} = {}", key, v),
        None => println!("  {:<16} = (not set)", key),
    }
}

/// Execute `agentworth config get <key>`.
pub fn run_config_get(key: &str, json: bool) -> Result<()> {
    run_config_get_at(&config_file_path()?, key, json)
}

fn run_config_get_at(path: &Path, key: &str, json: bool) -> Result<()> {
    let cfg = load_config_from(path)?;
    let value = config_value(&cfg, key)?;

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
    apply_config_value(&mut cfg, key, value)?;
    save_config_to(path, &cfg)?;

    // Report what was stored, not what was typed: `set archie.colourway c3` saves "C3".
    let stored = config_value(&cfg, key)?.unwrap_or_else(|| value.to_string());

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "key": key, "value": stored, "status": "saved" }))?
        );
    } else {
        println!("Saved {} = {}", key, stored);
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
            archie: Some(ArchieConfig {
                accessory: Some("goggles".to_string()),
                colourway: Some("C2".to_string()),
            }),
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
    fn test_archie_keys_round_trip_and_normalize_case() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        run_config_set_at(&path, "archie.accessory", "GOGGLES", false).unwrap();
        run_config_set_at(&path, "archie.colourway", "c4", false).unwrap();

        let cfg = load_config_from(&path).unwrap();
        assert_eq!(config_value(&cfg, "archie.accessory").unwrap().as_deref(), Some("goggles"));
        assert_eq!(config_value(&cfg, "archie.colourway").unwrap().as_deref(), Some("C4"));

        // Setting one archie key must not wipe the other.
        run_config_set_at(&path, "archie.accessory", "none", false).unwrap();
        let cfg = load_config_from(&path).unwrap();
        assert_eq!(config_value(&cfg, "archie.colourway").unwrap().as_deref(), Some("C4"));
    }

    #[test]
    fn test_archie_keys_reject_invalid_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(run_config_set_at(&path, "archie.accessory", "monocle", false).is_err());
        assert!(run_config_set_at(&path, "archie.colourway", "C9", false).is_err());
    }

    #[test]
    fn test_config_as_json_names_every_key_even_unset_and_never_the_path() {
        let value = config_as_json(&CliConfig::default());
        for key in CONFIG_KEYS {
            assert!(value.get(*key).is_some_and(|v| v.is_null()), "{} missing or not null", key);
        }
        assert!(
            value.get("config_path").is_none(),
            "the config file's absolute path must not reach the HTTP payload"
        );
    }

    #[test]
    fn test_clear_config_value_unsets_without_touching_its_sibling() {
        let mut cfg = CliConfig::default();
        apply_config_value(&mut cfg, "archie.accessory", "goggles").unwrap();
        apply_config_value(&mut cfg, "archie.colourway", "C2").unwrap();
        clear_config_value(&mut cfg, "archie.accessory").unwrap();
        assert_eq!(config_value(&cfg, "archie.accessory").unwrap(), None);
        assert_eq!(config_value(&cfg, "archie.colourway").unwrap().as_deref(), Some("C2"));
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
