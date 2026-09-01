//! CLI command implementations for AgentWorth.

pub mod audit;
pub mod blunder;
pub mod search;

pub use audit::run_audit_command;
pub use blunder::run_blunder_command;
pub use search::run_search_command;

/// Canonical shared list of assistant apology and remorse patterns across all commands.
pub const APOLOGY_PATTERNS: &[&str] = &[
    "my mistake",
    "i apologize",
    "my apologies",
    "i am sorry",
    "i'm sorry",
    "turned my safety mechanism into a weapon",
    "lost track",
    "accidentally deleted",
    "sincerely apologize",
];

/// Checks if a shell command executes recursive deletion on top-level root/system paths,
/// avoiding false positives on nested subdirectories like `~/code/...` or `/Users/.../code/...`.
pub fn is_high_risk_rm_path(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if lower.contains(" / ")
        || lower.ends_with(" /")
        || lower.contains(" /*")
        || lower.contains(" ~")
        || lower.contains(" $home")
        || lower.contains(" ${home}")
    {
        return true;
    }

    for part in lower.split_whitespace() {
        let clean = part.trim_matches(|c| c == '\'' || c == '"');
        if clean == "/"
            || clean == "/*"
            || clean == "~"
            || clean == "~/"
            || clean == "/code"
            || clean == "/code/"
            || clean == "/projects"
            || clean == "/projects/"
            || clean == "/users"
            || clean == "/users/"
            || clean == "/home"
            || clean == "/home/"
            || clean == "/root"
            || clean == "/root/"
            || clean == "/bin"
            || clean == "/usr"
            || clean == "/etc"
            || clean == "/var"
            || clean == "/system"
            || clean == "/library"
        {
            return true;
        }
    }
    false
}

/// Checks if a destructive command uses a bare/unscoped loop variable (the Katana disaster signature).
pub fn is_leaked_katana_var(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if !lower.contains("rm -rf") && !lower.contains("rm -fr") && !lower.contains("rm -r -f") {
        return false;
    }
    for word in lower.split_whitespace() {
        let clean = word.trim_matches(|c| c == '"' || c == '\'' || c == ';');
        if clean == "$d"
            || clean.starts_with("$d/")
            || clean == "${d}"
            || clean.starts_with("${d}/")
            || clean == "$target"
            || clean == "${target}"
        {
            return true;
        }
    }
    false
}

