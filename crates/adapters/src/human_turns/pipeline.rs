//! One JSON line from a turn source → one normalized [`HumanTurn`]. This is the step-1
//! normalization the Python builder (`tools/behavioral_insights.py`) did in
//! `clean_human_prompt` + `parse_iso_or_epoch` + the per-record field picks, ported here with
//! the same degeneracy rules: a record with no clean human text, no timestamp, or unparseable
//! lines degrade to `None`, never to a file failure.

use super::taxonomy;
use chrono::{DateTime, FixedOffset, Offset, TimeZone, Utc};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// One normalized human turn, ready to store. Local hour/date are computed at parse time
/// against the ingestor's offset (system local for real scans, a fixed offset in tests so
/// goldens are timezone-independent).
#[derive(Debug, Clone, PartialEq)]
pub struct HumanTurn {
    pub source: &'static str,
    pub session_id: Option<String>,
    pub turn_index: u64,
    /// UTC epoch milliseconds, rounded to 100 ms (the dedup grain the builder used).
    pub timestamp_ms: i64,
    pub epoch_secs: f64,
    pub local_hour: u32,
    pub local_date: String,
    pub word_count: u64,
    pub char_count: u64,
    pub friction_type: &'static str,
    /// `(term, occurrences)` for every taxonomy vocabulary term the text mentions, lowercased.
    pub vocab: Vec<(String, u64)>,
    /// Deterministic turn identity across sources: sha256 of
    /// `<timestamp_ms>|<first 60 chars>`, so the same turn in both `history.jsonl` and a
    /// transcript deduplicates in storage instead of double-counting.
    pub dedup_sig: String,
}

/// Extract the clean human text from a record's raw payload, then pass the schema's shared
/// injected-marker list over the remainder for anything this source and the shared list both
/// care about (`<system-reminder>`, task/telemetry envelopes, wake payloads).
///
/// Encoding the `?`-free parts of the Python `is_synthetic` check that the schema list does
/// not already cover — a turn shorter than two characters, and `<local-command-caveat>`
/// envelopes.
pub(crate) fn clean_turn_text(raw: &str) -> Option<String> {
    let without_settings = strip_block(raw, "<USER_SETTINGS_CHANGE>", "</USER_SETTINGS_CHANGE>");
    let cleaned = agentworth_schema::human_prompt_text(&without_settings)?;
    let text = cleaned.trim().to_string();
    if text.chars().count() < 2 {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("<local-command-caveat>") {
        return None;
    }
    if lower.starts_with("wait, you are agent w") {
        return None;
    }
    Some(text)
}

/// Epoch seconds from an ISO string, epoch seconds, or epoch milliseconds — the builder's
/// `parse_iso_or_epoch` (values over 1e11 read as milliseconds).
fn parse_timestamp(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => {
            let s = if let Some(i) = n.as_i64() {
                if i > 1_000_000_000_000 {
                    i as f64 / 1000.0
                } else {
                    i as f64
                }
            } else {
                n.as_f64()?
            };
            Some(s)
        }
        Value::String(s) => {
            let cleaned = s.trim().replace('Z', "+00:00");
            let dt = DateTime::parse_from_rfc3339(&cleaned).ok()?;
            let utc = dt.with_timezone(&Utc);
            Some(utc.timestamp() as f64 + utc.timestamp_subsec_millis() as f64 / 1000.0)
        }
        _ => None,
    }
}

/// Pull `(raw_text, timestamp, inline session id)` from a record of the given source family.
fn fields<'v>(
    source: &'static str,
    value: &'v Value,
) -> Option<(std::borrow::Cow<'v, str>, &'v Value, Option<String>)> {
    match source {
        "claude" => claude_record_fields(value),
        "antigravity" => agy_record_fields(value),
        _ => None,
    }
}

/// Claude sources carry two record shapes: the standalone history's
/// `{display, timestamp, sessionId}` and transcript user-turns (`type == "user"` with
/// `message.content` — a string, or a list whose text parts join and whose tool_results
/// degrade the record under the same rule the Python builder applied).
fn claude_record_fields<'v>(
    value: &'v Value,
) -> Option<(std::borrow::Cow<'v, str>, &'v Value, Option<String>)> {
    if value.get("type").and_then(|t| t.as_str()) == Some("user") {
        let message = value.get("message")?;
        let content = message.get("content")?;
        let text = match content {
            Value::String(s) => std::borrow::Cow::Borrowed(s.as_str()),
            Value::Array(parts) => {
                let mut joined = String::new();
                for part in parts {
                    match part.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if !joined.is_empty() {
                                joined.push(' ');
                            }
                            joined.push_str(part.get("text")?.as_str()?);
                        }
                        // A tool_result carried on a user record is a harness delivery, not
                        // typing — degrade under the same rule as the builder.
                        _ => return None,
                    }
                }
                std::borrow::Cow::Owned(joined)
            }
            _ => return None,
        };
        Some((text, value.get("timestamp")?, None))
    } else {
        Some((
            std::borrow::Cow::Borrowed(value.get("display")?.as_str()?),
            value.get("timestamp")?,
            value
                .get("sessionId")
                .and_then(|s| s.as_str())
                .map(str::to_string),
        ))
    }
}

/// Antigravity brain transcripts' `USER_INPUT` records carry `content` + `created_at`; its
/// history carries `display` + `timestamp` + `conversationId`.
fn agy_record_fields<'v>(
    value: &'v Value,
) -> Option<(std::borrow::Cow<'v, str>, &'v Value, Option<String>)> {
    if value.get("type").and_then(|t| t.as_str()) == Some("USER_INPUT") {
        Some((
            std::borrow::Cow::Borrowed(value.get("content")?.as_str()?),
            value.get("created_at")?,
            None,
        ))
    } else {
        Some((
            std::borrow::Cow::Borrowed(value.get("display")?.as_str()?),
            value.get("timestamp")?,
            value
                .get("conversationId")
                .and_then(|s| s.as_str())
                .map(str::to_string),
        ))
    }
}

/// Local hour and local date from epoch seconds against a fixed offset (or the system's
/// current one).
fn local_parts(epoch_secs: f64, offset: Option<FixedOffset>) -> (u32, String) {
    let secs_floor = epoch_secs.floor() as i64;
    let millis_subsec = ((epoch_secs - epoch_secs.floor()) * 1000.0).round() as u32;
    let utc = Utc
        .timestamp_millis_opt(secs_floor * 1000 + millis_subsec.min(999) as i64)
        .single()
        .unwrap_or_else(|| Utc.timestamp_millis_opt(0).single().unwrap());
    let offset = offset.unwrap_or_else(|| {
        // The system's current local offset — the same semantics the Python builder's
        // `dt.astimezone()` reached without carrying a TZ argument.
        chrono::Local::now().offset().fix()
    });
    let now = utc.with_timezone(&offset);
    (
        now.format("%H").to_string().parse().unwrap_or(0),
        now.format("%Y-%m-%d").to_string(),
    )
}

/// Word count over the builder's token pattern (`\b\w+\b`).
pub(crate) fn count_words(text: &str) -> usize {
    static WORDS: OnceLock<Regex> = OnceLock::new();
    let words = WORDS.get_or_init(|| compile(r"\b\w+\b"));
    words.find_iter(text).count()
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("pipeline regex failed to compile")
}

fn strip_block(s: &str, open: &str, close: &str) -> String {
    // Mirrors agentworth_schema::human::strip_blocks's structure for one more harness
    // envelope this source produces (`<USER_SETTINGS_CHANGE>` in Antigravity prompts). The
    // offsets come from find() of ASCII tags, always char boundaries.
    #[allow(
        clippy::string_slice,
        reason = "offsets come from find() of ASCII tags, always char boundaries"
    )]
    fn inner(s: &str, open: &str, close: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(start) = rest.find(open) {
            out.push_str(&rest[..start]);
            let after = &rest[start + open.len()..];
            match after.find(close) {
                Some(end) => {
                    rest = &after[end + close.len()..];
                }
                None => {
                    return out;
                }
            }
        }
        out.push_str(rest);
        out
    }
    inner(s, open, close)
}

/// The per-line normalizer: fields per source family, cleaning, timestamp, local-hour/date,
/// classification, dedup signature.
pub fn parse_line(
    src: &super::TurnFileSource,
    value: &Value,
    offset: Option<FixedOffset>,
) -> Option<HumanTurn> {
    let (raw, timestamp, inline_session) = fields(src.source, value)?;
    let cleaned = clean_turn_text(raw.as_ref())?;
    let epoch_secs = parse_timestamp(timestamp)?;
    let timestamp_ms = (epoch_secs * 1000.0).round() as i64;
    let (local_hour, local_date) = local_parts(epoch_secs, offset);
    let prefix: String = cleaned.chars().take(60).collect();
    Some(HumanTurn {
        source: src.source,
        session_id: inline_session,
        turn_index: 0,
        timestamp_ms,
        epoch_secs,
        local_hour,
        local_date,
        word_count: count_words(&cleaned) as u64,
        char_count: cleaned.chars().count() as u64,
        friction_type: taxonomy::classify_friction(&cleaned),
        vocab: taxonomy::vocab_matches(&cleaned),
        dedup_sig: dedup_sig(timestamp_ms, &prefix),
    })
}

fn dedup_sig(timestamp_ms: i64, prefix: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(timestamp_ms.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(prefix.as_bytes());
    hex::encode(hasher.finalize())
}
