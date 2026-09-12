//! Which `UserMessage` events a person actually typed.
//!
//! Harnesses put a lot on the user role that no human wrote: cross-session relays, task
//! notifications, compaction continuations, expanded slash-command and skill bodies, wake
//! payloads for autonomous loops. Everything that asks "what did the person say" -- the wake
//! document, the correction autopsy, the `user_turn` chunk kind -- has to agree on one list, so
//! the list lives here and nowhere else.

/// Prefixes of user-role text the harness generated, matched after leading whitespace.
/// Case-sensitive on purpose: these are literal envelope strings, not prose.
const INJECTED_PREFIXES: &[&str] = &[
    "Another Claude session sent a message",
    "<cross-session-message",
    "[SYSTEM NOTIFICATION",
    "<task-notification",
    "<system-reminder",
    "<command-name>",
    "<command-message>",
    "<local-command",
    "<telem-notice>",
    "<command-status>",
    "<context_summary>",
    "<ci-monitor-event",
    "[archie grounding]",
    "This session is being continued",
    "Base directory for this skill",
    "Paperclip wake payload",
    "You are Paperclip",
    "You are paperclip",
];

/// Substrings that mark an autonomous-loop wake payload wherever they fall in the text.
const INJECTED_MARKERS: &[&str] = &[
    "treat this wake payload as the highest-priority change",
    "Continue your Paperclip work",
];

/// False for a user-role message the harness injected rather than the person typed.
pub fn is_human_prompt(content: &str) -> bool {
    let text = content.trim_start();
    if text.is_empty() {
        return false;
    }
    if INJECTED_PREFIXES.iter().any(|p| text.starts_with(p)) {
        return false;
    }
    !INJECTED_MARKERS.iter().any(|m| text.contains(m))
}

/// The text a person typed, with harness envelopes stripped: `<system-reminder>` blocks the
/// harness appends to a real prompt, Antigravity's `<USER_REQUEST>` wrapper and its
/// `<ADDITIONAL_METADATA>` tail. `None` when nothing human is left.
pub fn human_prompt_text(content: &str) -> Option<String> {
    let text = unwrap_block(content, "<USER_REQUEST>", "</USER_REQUEST>");
    let text = strip_blocks(text, "<system-reminder>", "</system-reminder>");
    let text = strip_blocks(&text, "<ADDITIONAL_METADATA>", "</ADDITIONAL_METADATA>");
    let text = text.trim();
    is_human_prompt(text).then(|| text.to_string())
}

/// The inside of the first `open`..`close` pair, or the whole string when there is none.
#[allow(
    clippy::string_slice,
    reason = "offsets come from find() of ASCII tags, always char boundaries"
)]
fn unwrap_block<'a>(s: &'a str, open: &str, close: &str) -> &'a str {
    match (s.find(open), s.find(close)) {
        (Some(a), Some(b)) if a < b => &s[a + open.len()..b],
        _ => s,
    }
}

/// `s` with every `open`..`close` block removed. An unterminated block drops the tail.
#[allow(
    clippy::string_slice,
    reason = "offsets come from find() of ASCII tags, always char boundaries"
)]
fn strip_blocks(s: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find(open) {
        out.push_str(&rest[..a]);
        match rest[a..].find(close) {
            Some(b) => rest = &rest[a + b + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_prompts_pass() {
        assert!(is_human_prompt("fix the failing test in billing.js"));
        assert!(is_human_prompt("  ok"));
        assert!(is_human_prompt("<!-- attach --> > quoted comment\n\nmy reply"));
    }

    #[test]
    fn harness_envelopes_fail() {
        assert!(!is_human_prompt(""));
        assert!(!is_human_prompt("   "));
        assert!(!is_human_prompt("<task-notification>done</task-notification>"));
        assert!(!is_human_prompt("Another Claude session sent a message: hi"));
        assert!(!is_human_prompt("<command-name>/compact</command-name>"));
        assert!(!is_human_prompt("Base directory for this skill: /tmp/x\n# Skill"));
        assert!(!is_human_prompt(
            "You are agent fa470b9f (CEO). Continue your Paperclip work."
        ));
        assert!(!is_human_prompt("This session is being continued from a previous one"));
    }

    #[test]
    fn strips_appended_system_reminder() {
        let raw = "make it blue<system-reminder>\nsome harness text\n</system-reminder>";
        assert_eq!(human_prompt_text(raw).as_deref(), Some("make it blue"));
        let two = "a<system-reminder>x</system-reminder> b <system-reminder>y</system-reminder>";
        assert_eq!(human_prompt_text(two).as_deref(), Some("a b"));
    }

    #[test]
    fn unwraps_antigravity_request() {
        let raw = "<USER_REQUEST>\nrun the suite\n</USER_REQUEST><ADDITIONAL_METADATA>cwd=/x</ADDITIONAL_METADATA>";
        assert_eq!(human_prompt_text(raw).as_deref(), Some("run the suite"));
    }

    #[test]
    fn nothing_human_left_is_none() {
        assert_eq!(human_prompt_text("<system-reminder>only this</system-reminder>"), None);
        assert_eq!(human_prompt_text("<task-notification>x</task-notification>"), None);
    }
}
