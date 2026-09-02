//! Recovering a shell command's real exit status from an agent transcript.
//!
//! A `ShellCommand` whose `exit_code` is `None` says "somebody typed this command". Only
//! `Some(0)` says "it ran and succeeded". The outcome engine leans on that distinction, so
//! every adapter that *can* see the result of a Bash call should record it instead of
//! dropping it on the floor.
//!
//! Transcripts almost never carry a numeric exit code. What they carry is the harness's own
//! pass/fail envelope: a `tool_result` block with `is_error`, sometimes with an "Exit code N"
//! line inside the error text. Measured over 20 real Claude Code sessions (17,981 Bash tool
//! results): every result carried an explicit `is_error` boolean, 659 were errors, and 519 of
//! those spelled out a numeric code. Nothing carried a `returnCode`/`exitCode` field.

use agentworth_schema::{EventPayload, NormalizedEvent};
use std::collections::{HashMap, HashSet};

/// Pull a numeric exit code out of a harness error envelope such as "Error: Exit code 1" or
/// "Command failed with exit code 137".
///
/// Only meaningful on text the harness itself marked as an error. On a *successful* result the
/// same phrase routinely appears inside ordinary stdout (a grep hit, a pasted log), so calling
/// this on passing output would invent failures out of a command's own words.
pub fn parse_exit_code_phrase(text: &str) -> Option<i32> {
    let lower = text.to_lowercase();
    let idx = lower.find("exit code")?;
    let rest = &lower[idx + "exit code".len()..];
    let digits: String = rest
        .trim_start_matches([':', ' ', '='])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i32>().ok()
}

/// The exit code implied by a tool result's pass/fail envelope.
///
/// - not an error -> `Some(0)`. The harness only clears `is_error` when the command returned 0.
/// - an error with a code in the text -> that code.
/// - an error with no code -> `Some(1)`. `ShellCommand::exit_code` is an `Option<i32>` with no
///   room for "failed, code unknown", and widening it to an enum would churn the on-disk trace
///   format and ~40 call sites for no gain: every consumer asks either `== Some(0)` (really
///   succeeded) or `!= Some(0)` (did not). `Some(1)` lands on the right side of that line, and
///   `None` stays reserved for "we genuinely never saw the result". A command the harness
///   refused to run also arrives here; it did not succeed either, so the same answer holds.
pub fn exit_code_from_result(is_error: bool, output_text: &str) -> Option<i32> {
    if !is_error {
        return Some(0);
    }
    Some(parse_exit_code_phrase(output_text).unwrap_or(1))
}

/// Flatten a `tool_result` content value (string, or a list of text blocks) into plain text.
pub fn result_text(output: &serde_json::Value) -> String {
    match output {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(o) => o
            .get("output")
            .or_else(|| o.get("stdout"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

/// Fill in `ShellCommand::exit_code` from the `ToolResult` that answered the same tool call.
///
/// Adapters emit a `ShellCommand` when they see the *request* — at which point the result is
/// still several records away — so the exit status has to be stitched back on afterwards. The
/// `ShellCommand` is always emitted directly behind its own `ToolCall`, which is what carries
/// the id the result is keyed by; requiring that adjacency is what keeps this from attaching
/// one command's result to another command.
///
/// Only fills gaps: an adapter whose source format states the exit code outright keeps it.
pub fn backfill_shell_exit_codes(events: &mut [NormalizedEvent]) {
    backfill_shell_exit_codes_except(events, &HashSet::new())
}

/// `backfill_shell_exit_codes`, minus the tool calls whose result does not describe a finished
/// command — a backgrounded task that only reports its *launch*, or a run the user interrupted.
/// Those keep `exit_code: None`, because "still running" is not "passed".
pub fn backfill_shell_exit_codes_except(events: &mut [NormalizedEvent], unknown: &HashSet<String>) {
    let mut results: HashMap<String, Option<i32>> = HashMap::new();
    for event in events.iter() {
        if let EventPayload::ToolResult(res) = &event.payload {
            if let Some(id) = &res.call_id {
                if unknown.contains(id) {
                    continue;
                }
                let text = result_text(&res.output);
                results.insert(id.clone(), exit_code_from_result(res.is_error, &text));
            }
        }
    }
    if results.is_empty() {
        return;
    }

    let owning_call_ids: Vec<Option<String>> = events
        .iter()
        .enumerate()
        .map(|(i, event)| {
            if !matches!(&event.payload, EventPayload::ShellCommand(c) if c.exit_code.is_none()) {
                return None;
            }
            match i
                .checked_sub(1)
                .and_then(|prev| events.get(prev))
                .map(|e| &e.payload)
            {
                Some(EventPayload::ToolCall(call)) => call.id.clone(),
                _ => None,
            }
        })
        .collect();

    for (event, call_id) in events.iter_mut().zip(owning_call_ids) {
        let Some(call_id) = call_id else { continue };
        let Some(code) = results.get(&call_id).copied().flatten() else {
            continue;
        };
        if let EventPayload::ShellCommand(cmd) = &mut event.payload {
            cmd.exit_code = Some(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_envelope_means_exit_zero() {
        assert_eq!(exit_code_from_result(false, "test result: ok."), Some(0));
    }

    #[test]
    fn error_envelope_uses_the_stated_code() {
        assert_eq!(exit_code_from_result(true, "Error: Exit code 101"), Some(101));
        assert_eq!(
            exit_code_from_result(true, "Command failed with exit code 137"),
            Some(137)
        );
    }

    #[test]
    fn error_envelope_without_a_code_is_still_a_failure() {
        assert_eq!(
            exit_code_from_result(true, "Permission for this action was denied"),
            Some(1)
        );
    }

    #[test]
    fn exit_code_phrase_is_not_read_off_passing_output() {
        // The phrase shows up inside ordinary stdout often enough that trusting it on a
        // success envelope would invent failures. `exit_code_from_result` never looks.
        assert_eq!(
            exit_code_from_result(false, "the script returns exit code 1 on bad input"),
            Some(0)
        );
    }
}
