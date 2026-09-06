//! The meter: what a live session has spent, read from its own transcript.
//!
//! Hook payloads carry no token usage. The transcript does: every assistant record has
//! `message.usage` with the four counts the pricing table prices. [`TranscriptTail`] reads
//! forward from where it stopped last time, so a session can be metered while it runs.
//!
//! Two flavours of transcript, one meter. Claude Code writes an `assistant` record per turn with
//! `message.usage`; Codex writes an `event_msg` record whose `payload.type` is `token_count`,
//! carrying `last_token_usage` and, unlike Claude Code, the provider's own rate-limit percentages.
//!
//! Two traps this module is built around. Claude Code writes one record per content block and
//! repeats the same `message.id` with the same usage on each, so an undeduped sum counts a turn
//! two or three times. And the last line of a file being appended to is often half-written, so
//! the offset stays at the start of it until the rest arrives.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Per-million-token prices for one model. The caller supplies these; this crate holds no
/// pricing table of its own and asserts no number.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

/// One assistant turn's usage, priced at the rates the caller gave for its model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnUsage {
    /// Running count of assistant records seen for this session, starting at 1.
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub usd: f64,
}

impl TurnUsage {
    /// Every token the turn touched, cache included: the unit a spend cap is written in.
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }
}

/// The provider's own accounting, when the harness writes it down. Codex does; Claude Code does
/// not. This is a percentage of a window the provider defines, and it is never AgentWorth's
/// token count -- the two answer different questions and must not be mixed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderLimit {
    pub used_percent: f64,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub plan_type: Option<String>,
    pub spend_control_reached: Option<bool>,
}

/// A transcript read forward from wherever the last read stopped.
#[derive(Debug, Clone)]
pub struct TranscriptTail {
    pub path: PathBuf,
    /// Byte offset of the next unread line. A partial trailing line leaves it at that line's
    /// start, so the line is parsed once, whole.
    pub offset: u64,
    pub turns_seen: u64,
    seen_ids: HashSet<String>,
    model: Option<String>,
    primary_limit: Option<ProviderLimit>,
    secondary_limit: Option<ProviderLimit>,
}

impl TranscriptTail {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            turns_seen: 0,
            seen_ids: HashSet::new(),
            model: None,
            primary_limit: None,
            secondary_limit: None,
        }
    }

    /// The newest primary rate-limit window the transcript has reported, if its harness reports
    /// one at all.
    pub fn latest_limit(&self) -> Option<&ProviderLimit> {
        self.primary_limit.as_ref()
    }

    /// The newest secondary window (Codex's weekly cap, next to the five-hour one).
    pub fn secondary_limit(&self) -> Option<&ProviderLimit> {
        self.secondary_limit.as_ref()
    }

    /// Reads every complete line appended since the last call and prices the assistant turns
    /// among them. A file that has not grown yields an empty vector, which is the common case.
    pub fn read_new(&mut self, rates: &dyn Fn(&str) -> Rates) -> Result<Vec<TurnUsage>> {
        let mut file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e).with_context(|| format!("reading {}", self.path.display()));
            }
        };
        let len = file.metadata()?.len();
        // A shorter file is a different file: a rotated or rewritten transcript restarts.
        if len < self.offset {
            self.offset = 0;
        }
        if len == self.offset {
            return Ok(Vec::new());
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let text = String::from_utf8_lossy(&buf).into_owned();

        let mut consumed = 0usize;
        let mut turns = Vec::new();
        for line in text.split_inclusive('\n') {
            if !line.ends_with('\n') {
                break; // partial: leave the offset here and read it whole next time
            }
            consumed += line.len();
            if let Some(turn) = self.turn_from_line(line.trim_end(), rates) {
                turns.push(turn);
            }
        }
        self.offset += consumed as u64;
        Ok(turns)
    }

    fn turn_from_line(&mut self, line: &str, rates: &dyn Fn(&str) -> Rates) -> Option<TurnUsage> {
        if line.is_empty() {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        match value.get("type").and_then(|t| t.as_str()) {
            Some("assistant") => self.claude_turn(&value, rates),
            Some("event_msg") | Some("turn_context") => self.codex_turn(&value, rates),
            _ => None,
        }
    }

    /// Codex's rollout log: a `token_count` event per turn, a `turn_context` event whenever the
    /// model changes, and the provider's rate limits riding along on the former.
    fn codex_turn(
        &mut self,
        value: &serde_json::Value,
        rates: &dyn Fn(&str) -> Rates,
    ) -> Option<TurnUsage> {
        if let Some(model) = value
            .get("payload")
            .and_then(|p| p.get("model"))
            .or_else(|| value.get("model"))
            .and_then(|m| m.as_str())
        {
            self.model = Some(model.to_string());
        }
        let payload = value.get("payload")?;
        if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
            return None;
        }
        if let Some(limits) = payload.get("rate_limits") {
            let plan = limits.get("plan_type").and_then(|p| p.as_str());
            let reached = limits
                .get("spend_control_reached")
                .and_then(serde_json::Value::as_bool);
            if let Some(window) = limits.get("primary").and_then(|w| provider_limit(w, plan, reached))
            {
                self.primary_limit = Some(window);
            }
            if let Some(window) = limits
                .get("secondary")
                .and_then(|w| provider_limit(w, plan, reached))
            {
                self.secondary_limit = Some(window);
            }
        }
        let last = payload.get("info")?.get("last_token_usage")?;
        let count = |key: &str| last.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0);
        let cache_read = count("cached_input_tokens");
        let input = count("input_tokens").saturating_sub(cache_read);
        let output = count("output_tokens") + count("reasoning_output_tokens");
        let model = self.model.clone().unwrap_or_else(|| "unknown".to_string());
        Some(self.finish(value, model, input, output, cache_read, 0, rates))
    }

    fn claude_turn(
        &mut self,
        value: &serde_json::Value,
        rates: &dyn Fn(&str) -> Rates,
    ) -> Option<TurnUsage> {
        let message = value.get("message")?;
        let usage = message.get("usage")?;
        if let Some(id) = message.get("id").and_then(|i| i.as_str()) {
            if !self.seen_ids.insert(id.to_string()) {
                return None;
            }
        }
        let count = |key: &str| usage.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0);
        let input = count("input_tokens");
        let output = count("output_tokens");
        let cache_read = count("cache_read_input_tokens");
        let cache_creation = count("cache_creation_input_tokens");
        let model = message
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();
        Some(self.finish(value, model, input, output, cache_read, cache_creation, rates))
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        value: &serde_json::Value,
        model: String,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_creation: u64,
        rates: &dyn Fn(&str) -> Rates,
    ) -> TurnUsage {
        let r = rates(&model);
        let usd = (input as f64 * r.input_per_mtok
            + output as f64 * r.output_per_mtok
            + cache_read as f64 * r.cache_read_per_mtok
            + cache_creation as f64 * r.cache_write_per_mtok)
            / 1_000_000.0;
        let at = value
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map_or_else(Utc::now, |t| t.with_timezone(&Utc));
        self.turns_seen += 1;
        TurnUsage {
            seq: self.turns_seen,
            at,
            model,
            input,
            output,
            cache_read,
            cache_creation,
            usd,
        }
    }
}

fn provider_limit(
    window: &serde_json::Value,
    plan_type: Option<&str>,
    spend_control_reached: Option<bool>,
) -> Option<ProviderLimit> {
    let used_percent = window.get("used_percent")?.as_f64()?;
    Some(ProviderLimit {
        used_percent,
        window_minutes: window.get("window_minutes").and_then(serde_json::Value::as_i64),
        resets_at: window
            .get("resets_at")
            .and_then(serde_json::Value::as_i64)
            .and_then(|s| DateTime::from_timestamp(s, 0)),
        plan_type: plan_type.map(str::to_string),
        spend_control_reached,
    })
}

/// What the session has spent so far. Built by folding [`TurnUsage`] in; start from
/// `SessionSpend::default()`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSpend {
    pub tokens: u64,
    pub usd: f64,
    pub turns: u64,
    /// Cache reads as a fraction of all tokens. The number the cache economics report reads.
    pub cache_read_share: f64,
    /// The provider's own window, when the harness writes one down. Never derived from the
    /// counts above: AgentWorth reports burn against a budget the person set, and quota against
    /// the provider's own number or not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_limit: Option<ProviderLimit>,
    #[serde(default, skip_serializing)]
    cache_read: u64,
}

impl SessionSpend {
    pub fn add(&mut self, turn: &TurnUsage) {
        self.tokens += turn.tokens();
        self.usd += turn.usd;
        self.turns += 1;
        self.cache_read += turn.cache_read;
        self.cache_read_share = if self.tokens == 0 {
            0.0
        } else {
            self.cache_read as f64 / self.tokens as f64
        };
    }

    /// Every path a transcript names, folded in one call.
    pub fn from_turns(turns: &[TurnUsage]) -> Self {
        let mut spend = Self::default();
        for turn in turns {
            spend.add(turn);
        }
        spend
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn flat_rates(_model: &str) -> Rates {
        Rates {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
            cache_write_per_mtok: 3.75,
        }
    }

    fn assistant(id: &str, input: u64, output: u64, cache_read: u64, cache_creation: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "sessionId": "7f3c9a2e-1b4d-4c8e-9a0f-2d6e8b1c3a5f",
            "timestamp": "2026-09-04T05:33:01.000Z",
            "message": {
                "model": "claude-fable-5-1",
                "id": id,
                "role": "assistant",
                "content": [{"type": "text", "text": "…"}],
                "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": cache_read,
                    "cache_creation_input_tokens": cache_creation
                }
            }
        })
        .to_string()
    }

    fn append(path: &std::path::Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open");
        file.write_all(text.as_bytes()).expect("write");
    }

    #[test]
    fn a_second_read_sees_only_what_was_appended() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        append(&path, &format!("{}\n", assistant("msg_1", 12, 240, 31000, 4800)));
        let mut tail = TranscriptTail::new(&path);
        let first = tail.read_new(&flat_rates).expect("read");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].seq, 1);
        assert_eq!(first[0].model, "claude-fable-5-1");

        assert!(tail.read_new(&flat_rates).expect("read").is_empty());

        append(&path, &format!("{}\n", assistant("msg_2", 20, 100, 1000, 0)));
        let second = tail.read_new(&flat_rates).expect("read");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].seq, 2);
        assert_eq!(second[0].input, 20);
        assert_eq!(tail.turns_seen, 2);
    }

    #[test]
    fn a_half_written_last_line_is_read_once_it_is_whole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let whole = assistant("msg_1", 10, 10, 0, 0);
        let (head, tail_text) = whole.split_at(whole.len() / 2);
        append(&path, &format!("{}\n{}", assistant("msg_0", 1, 1, 0, 0), head));
        let mut tail = TranscriptTail::new(&path);
        let first = tail.read_new(&flat_rates).expect("read");
        assert_eq!(first.len(), 1, "the partial line is not parsed yet");

        append(&path, &format!("{tail_text}\n"));
        let second = tail.read_new(&flat_rates).expect("read");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].input, 10);
    }

    #[test]
    fn one_message_id_repeated_per_content_block_counts_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let line = assistant("msg_dup", 100, 200, 0, 0);
        append(&path, &format!("{line}\n{line}\n{line}\n"));
        let mut tail = TranscriptTail::new(&path);
        let turns = tail.read_new(&flat_rates).expect("read");
        assert_eq!(turns.len(), 1);
        assert_eq!(tail.turns_seen, 1);
    }

    #[test]
    fn a_turn_is_priced_at_the_rates_the_caller_gave() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        append(
            &path,
            &format!("{}\n", assistant("msg_1", 1_000_000, 1_000_000, 1_000_000, 1_000_000)),
        );
        let mut tail = TranscriptTail::new(&path);
        let turns = tail.read_new(&flat_rates).expect("read");
        assert!((turns[0].usd - (3.0 + 15.0 + 0.30 + 3.75)).abs() < 1e-9);
        assert_eq!(turns[0].tokens(), 4_000_000);

        let spend = SessionSpend::from_turns(&turns);
        assert_eq!(spend.turns, 1);
        assert_eq!(spend.tokens, 4_000_000);
        assert!((spend.cache_read_share - 0.25).abs() < 1e-9);
    }

    #[test]
    fn records_that_are_not_assistant_turns_are_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        append(
            &path,
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\nnot json\n",
        );
        let mut tail = TranscriptTail::new(&path);
        assert!(tail.read_new(&flat_rates).expect("read").is_empty());
    }

    fn codex_token_count(input: u64, cached: u64, output: u64, reasoning: u64, limits: bool) -> String {
        let mut payload = serde_json::json!({
            "type": "token_count",
            "info": {
                "total_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output,
                    "reasoning_output_tokens": reasoning,
                    "total_tokens": input + output
                },
                "last_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output,
                    "reasoning_output_tokens": reasoning,
                    "total_tokens": input + output
                },
                "model_context_window": 272_000
            }
        });
        if limits {
            payload["rate_limits"] = serde_json::json!({
                "primary": {"used_percent": 41.5, "window_minutes": 300, "resets_at": 1_788_000_000_i64},
                "secondary": {"used_percent": 88.0, "window_minutes": 10080, "resets_at": null},
                "plan_type": "pro",
                "spend_control_reached": false
            });
        }
        serde_json::json!({
            "type": "event_msg",
            "timestamp": "2026-09-04T05:33:01.000Z",
            "payload": payload
        })
        .to_string()
    }

    #[test]
    fn a_codex_rollout_is_metered_with_the_model_from_its_turn_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rollout.jsonl");
        let turn_context = serde_json::json!({
            "type": "turn_context",
            "timestamp": "2026-09-04T05:32:00.000Z",
            "payload": {"cwd": "/repo", "model": "gpt-5.3-codex", "approval_policy": "on-request"}
        })
        .to_string();
        append(
            &path,
            &format!(
                "{turn_context}\n{}\n{}\n",
                codex_token_count(9_000, 8_000, 300, 700, false),
                codex_token_count(12_000, 11_000, 100, 0, true)
            ),
        );

        let mut tail = TranscriptTail::new(&path);
        let turns = tail.read_new(&flat_rates).expect("read");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].model, "gpt-5.3-codex");
        assert_eq!(turns[0].cache_read, 8_000);
        assert_eq!(turns[0].input, 1_000, "cached input is not counted twice");
        assert_eq!(turns[0].output, 1_000, "reasoning tokens are output");
        assert_eq!(turns[0].cache_creation, 0);
        assert_eq!(turns[1].seq, 2);

        let primary = tail.latest_limit().expect("the provider's own window");
        assert!((primary.used_percent - 41.5).abs() < 1e-9);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(
            primary.resets_at,
            DateTime::from_timestamp(1_788_000_000, 0),
            "resets_at is a unix timestamp"
        );
        assert_eq!(primary.plan_type.as_deref(), Some("pro"));
        assert_eq!(primary.spend_control_reached, Some(false));
        let secondary = tail.secondary_limit().expect("the weekly window");
        assert!((secondary.used_percent - 88.0).abs() < 1e-9);
        assert_eq!(secondary.resets_at, None);
    }

    #[test]
    fn a_claude_transcript_reports_no_provider_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        append(&path, &format!("{}\n", assistant("msg_1", 12, 240, 31000, 4800)));
        let mut tail = TranscriptTail::new(&path);
        assert_eq!(tail.read_new(&flat_rates).expect("read").len(), 1);
        assert!(tail.latest_limit().is_none());
        assert!(tail.secondary_limit().is_none());
        assert!(SessionSpend::default().provider_limit.is_none());
    }
}
