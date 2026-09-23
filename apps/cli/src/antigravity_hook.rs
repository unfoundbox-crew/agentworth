//! The Antigravity CLI session-start wake injection.
//!
//! Antigravity CLI (`agy`) exposes no `SessionStart` hook. Its hook surface is
//! `~/.gemini/config/hooks.json`, with `PreInvocation` / `PostInvocation` entries that run a
//! command and read one JSON object from its stdout. `PreInvocation` fires before every model
//! call, not once per session, so the closest thing to a session-start hook is `PreInvocation`
//! plus a once-per-conversation marker: the first turn of a conversation gets the wake
//! document, later turns get `{}` and nothing is injected.
//!
//! `archie hook print antigravity` prints the hooks.json entry that points here. The injected
//! text is `crate::wake`'s rendered document, which is thirty lines by design -- the token cap
//! is the document's own line budget, not a second truncation. That document already carries
//! the prior sessions for the repo ("Before that"), which is the carry-forward half of the
//! packet; nothing here re-renders a handoff.
//!
//! Local injection, like the shell hook it replaces, does not redact: the agent is already
//! standing in the checkout and the paths are its own. `--redact` is available for a caller
//! that wants the masked form.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::repo_key_for_dir;
use agentworth_storage::Storage;
use anyhow::Result;
use serde_json::{json, Value};

use crate::wake::{load_wake, render_markdown, WakeOptions};

/// The hooks.json entry that registers the wake injection. The key is the same one the
/// hand-rolled `archie-wake.sh` used, so a user replacing the script in place keeps their
/// existing `~/.gemini/config/hooks.json` readable.
pub fn antigravity_hooks_value() -> Value {
    json!({
        "archie_wake": {
            "PreInvocation": [{
                "command": "archie session wake --inject antigravity",
                "timeout": 5,
                "type": "command",
            }]
        }
    })
}

/// Print the snippet for a person to merge into `~/.gemini/config/hooks.json`. Never writes:
/// the file is the user's, and printing lets them see what they are pasting, the same contract
/// `archie hook print claude` holds.
pub fn print_antigravity_snippet() -> Result<()> {
    println!("// merge into ~/.gemini/config/hooks.json (replaces the archie_wake entry)");
    println!("{}", serde_json::to_string_pretty(&antigravity_hooks_value())?);
    Ok(())
}

/// The working directory the hook payload is about.
///
/// Preference order mirrors the old shell hook: `workspacePaths` first, then a top-level
/// `workspace` or `cwd`. The user's home directory itself and anything under `~/.gemini` are
/// rejected -- those are the harness's own directories, not the project the agent is in.
pub fn resolve_antigravity_workspace(payload: &Value) -> Option<PathBuf> {
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    let acceptable = |candidate: &str| -> bool {
        if candidate.trim().is_empty() {
            return false;
        }
        let path = Path::new(candidate);
        if let Some(home) = &home {
            if path == home || path.starts_with(home.join(".gemini")) {
                return false;
            }
        }
        true
    };

    if let Some(paths) = payload.get("workspacePaths").and_then(Value::as_array) {
        for item in paths {
            if let Some(path) = item.as_str().filter(|p| acceptable(p)) {
                return Some(PathBuf::from(path));
            }
        }
    }
    for key in ["workspace", "cwd"] {
        if let Some(path) = payload.get(key).and_then(Value::as_str).filter(|p| acceptable(p)) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// The Antigravity injection envelope: one ephemeral message carrying the wake document.
pub fn antigravity_injection(markdown: &str) -> Value {
    let body = format!("[ARCHIE WAKE]\n{}\n[/ARCHIE WAKE]", markdown.trim_end());
    json!({ "injectSteps": [{ "ephemeralMessage": body }] })
}

/// Resolve one hook payload to an injection, or `{}` when there is nothing to inject.
///
/// Kept separate from stdin/storage so a test can drive it with an in-memory index and a temp
/// marker directory. The marker is what makes a per-invocation hook behave like a
/// session-start one: one wake per `conversationId`.
pub fn inject_for_payload(
    storage: &Storage,
    scanner: &Scanner,
    payload: &Value,
    state_dir: &Path,
    redact: bool,
) -> Result<Value> {
    let Some(workspace) = resolve_antigravity_workspace(payload) else {
        return Ok(json!({}));
    };
    let conversation = payload
        .get("conversationId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let marker = marker_path(state_dir, &conversation);
    if let Some(marker) = &marker {
        if marker.exists() {
            return Ok(json!({}));
        }
    }

    // A directory, resolved from the hook payload's `workspacePaths`/`cwd`, so it keys through
    // the checkout root like every other directory caller. `extract_repository_or_workspace`
    // is written for a session's transcript path and keys a repo checked out directly under
    // `code/` one component short of what the index holds.
    let repo = repo_key_for_dir(&workspace);
    let report = load_wake(
        storage,
        scanner,
        &repo,
        &workspace,
        WakeOptions { include_raw: !redact },
    )?;
    let markdown = render_markdown(&report);

    if let Some(marker) = &marker {
        if let Some(parent) = marker.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(marker, b"")?;
    }

    Ok(antigravity_injection(&markdown))
}

/// The command `archie session wake --inject antigravity` runs. Reads the hook payload from
/// stdin, prints one JSON object on stdout (and nothing else -- stdout is the hook channel).
pub fn run_antigravity_inject(redact: bool, db_path: Option<PathBuf>) -> Result<()> {
    let mut input = String::new();
    let _ = std::io::stdin().lock().read_to_string(&mut input);
    let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);

    let storage = match db_path {
        Some(path) => Storage::open_path(&path)?,
        None => Storage::open_default()?,
    };
    let storage = Arc::new(storage);
    let scanner = Scanner::new(Arc::clone(&storage));
    let state_dir = agentworth_storage::default_db_dir()?.join("antigravity-hooks");

    let value = inject_for_payload(&storage, &scanner, &payload, &state_dir, redact)?;
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

/// `<state_dir>/<sanitized conversation id>.wake`, or `None` when the payload named no
/// conversation (then every invocation injects, which is the honest failure -- better a
/// repeated wake than a session that never gets one).
fn marker_path(state_dir: &Path, conversation: &str) -> Option<PathBuf> {
    if conversation.is_empty() {
        return None;
    }
    let safe: String = conversation
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Some(state_dir.join(format!("{safe}.wake")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(workspace: &str, conversation: &str) -> Value {
        json!({ "conversationId": conversation, "workspacePaths": [workspace] })
    }

    #[test]
    fn resolves_the_first_project_workspace_and_skips_home() {
        let home = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| "/Users/nobody".to_string());
        let payload = json!({
            "conversationId": "c1",
            "workspacePaths": [home, format!("{home}/.gemini/antigravity-cli"), "/tmp/wt-a"]
        });
        assert_eq!(
            resolve_antigravity_workspace(&payload),
            Some(PathBuf::from("/tmp/wt-a")),
            "home and the harness dir are not the project"
        );
    }

    #[test]
    fn falls_back_to_a_top_level_cwd_when_workspace_paths_are_absent() {
        let payload = json!({ "cwd": "/tmp/wt-b" });
        assert_eq!(resolve_antigravity_workspace(&payload), Some(PathBuf::from("/tmp/wt-b")));
        assert_eq!(resolve_antigravity_workspace(&json!({})), None);
    }

    #[test]
    fn the_injection_envelope_wraps_the_wake_document() {
        let value = antigravity_injection("# Wake\nhello");
        let message = value["injectSteps"][0]["ephemeralMessage"]
            .as_str()
            .expect("an ephemeral message");
        assert!(message.starts_with("[ARCHIE WAKE]\n"));
        assert!(message.contains("# Wake\nhello"));
        assert!(message.ends_with("[/ARCHIE WAKE]"));
    }

    #[test]
    fn the_hooks_entry_points_at_the_inject_command() {
        let value = antigravity_hooks_value();
        let command = value["archie_wake"]["PreInvocation"][0]["command"]
            .as_str()
            .expect("a command");
        assert_eq!(command, "archie session wake --inject antigravity");
    }

    /// `PreInvocation` fires before every turn. Without the marker, wake would be re-injected
    /// on every one; with it, exactly the first turn of a conversation gets the document.
    #[test]
    fn injects_once_per_conversation_then_nothing() {
        use std::sync::Arc;

        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner = Scanner::new(Arc::clone(&storage));
        let dir = tempfile::tempdir().expect("tempdir");
        let hook = payload("/tmp/agwake-wt-a", "conv-1");

        let first = inject_for_payload(&storage, &scanner, &hook, dir.path(), true)
            .expect("first injection");
        let message = first["injectSteps"][0]["ephemeralMessage"]
            .as_str()
            .expect("first injection has a message");
        assert!(message.contains("[ARCHIE WAKE]"), "{message}");

        let second = inject_for_payload(&storage, &scanner, &hook, dir.path(), true)
            .expect("second injection");
        assert_eq!(
            second,
            json!({}),
            "a repeat invocation of the same conversation must inject nothing"
        );

        // A different conversation gets its own wake.
        let other = payload("/tmp/agwake-wt-a", "conv-2");
        let third = inject_for_payload(&storage, &scanner, &other, dir.path(), true)
            .expect("other conversation");
        assert!(third.get("injectSteps").is_some(), "{third}");
    }
}
