//! `policy.toml`: the file that decides whether anything is governed at all.
//!
//! A missing file governs nothing, and so does an empty one. A section that is absent turns its
//! rule off rather than defaulting it on -- the brake exists because a person wrote a number
//! down, not because AgentWorth guessed one. The repo's file overrides the home file section by
//! section: a repo can add the thrash rule without inheriting a spend cap it never set.
//!
//! ```toml
//! [thrash]   edits = 3            action = "halt"
//! [loop]     repeats = 3          action = "note"
//! [cache]    block_config_change = false         block_model_switch = false
//! # [spend]  tokens = ...         usd = ...      action = "halt"
//! # set this only after `archie policy init` shows you your own sessions' sizes
//! ```

use crate::governor::Action;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThrashRule {
    /// Edits to one file with no verification passing between them.
    pub edits: usize,
    #[serde(default = "Action::halt")]
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpendRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usd: Option<f64>,
    #[serde(default = "Action::halt")]
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoopRule {
    /// Identical tool calls in a row before the rule trips.
    pub repeats: usize,
    #[serde(default = "Action::note")]
    pub action: Action,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CacheRule {
    #[serde(default)]
    pub block_config_change: bool,
    #[serde(default)]
    pub block_model_switch: bool,
}

/// Every rule, each off until a section says otherwise.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thrash: Option<ThrashRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend: Option<SpendRule>,
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub loop_rule: Option<LoopRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheRule>,
}

impl Policy {
    /// True when nothing governs: the state a machine with no `policy.toml` is in.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Home first, repo over it. A file that is not there is not an error; a file that is there
    /// and malformed is, named with the line the parser stopped on.
    pub fn load(home_file: &Path, repo_file: &Path) -> Result<Self> {
        let mut policy = read_one(home_file)?.unwrap_or_default();
        if let Some(repo) = read_one(repo_file)? {
            if repo.thrash.is_some() {
                policy.thrash = repo.thrash;
            }
            if repo.spend.is_some() {
                policy.spend = repo.spend;
            }
            if repo.loop_rule.is_some() {
                policy.loop_rule = repo.loop_rule;
            }
            if repo.cache.is_some() {
                policy.cache = repo.cache;
            }
        }
        Ok(policy)
    }

    /// The lines `archie policy show` prints, in file order.
    pub fn describe(&self) -> Vec<String> {
        if self.is_empty() {
            return vec!["nothing is governed: no policy file, or no rule in it".to_string()];
        }
        let mut lines = Vec::new();
        if let Some(t) = self.thrash {
            lines.push(format!(
                "thrash: {} edits to one file with no passing verification -> {}",
                t.edits,
                t.action.as_str()
            ));
        }
        if let Some(s) = self.spend {
            let mut caps = Vec::new();
            if let Some(tokens) = s.tokens {
                caps.push(format!("{tokens} tokens"));
            }
            if let Some(usd) = s.usd {
                caps.push(format!("${usd:.2}"));
            }
            let caps = if caps.is_empty() {
                "no cap set".to_string()
            } else {
                caps.join(" or ")
            };
            lines.push(format!(
                "spend: a session over {caps} -> {}",
                s.action.as_str()
            ));
        }
        if let Some(l) = self.loop_rule {
            lines.push(format!(
                "loop: {} identical tool calls in a row -> {}",
                l.repeats,
                l.action.as_str()
            ));
        }
        if let Some(c) = self.cache {
            lines.push(format!(
                "cache: config change {}, model switch {}",
                blocked(c.block_config_change),
                blocked(c.block_model_switch)
            ));
        }
        lines
    }
}

fn blocked(on: bool) -> &'static str {
    if on {
        "blocked"
    } else {
        "allowed"
    }
}

fn read_one(path: &Path) -> Result<Option<Policy>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let policy: Policy = toml::from_str(&text)
        .with_context(|| format!("policy file {} is malformed", path.display()))?;
    Ok(Some(policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write");
        path
    }

    #[test]
    fn no_file_means_nothing_is_governed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = Policy::load(&dir.path().join("home.toml"), &dir.path().join("repo.toml"))
            .expect("load");
        assert!(policy.is_empty());
        assert_eq!(policy, Policy::default());
        assert_eq!(policy.describe().len(), 1);
    }

    #[test]
    fn the_repo_file_overrides_one_section_and_leaves_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = write(
            dir.path(),
            "home.toml",
            "[thrash]\nedits = 3\naction = \"halt\"\n[spend]\ntokens = 20000000\nusd = 40.0\n",
        );
        let repo = write(dir.path(), "repo.toml", "[thrash]\nedits = 5\naction = \"note\"\n");
        let policy = Policy::load(&home, &repo).expect("load");
        let thrash = policy.thrash.expect("thrash set");
        assert_eq!(thrash.edits, 5);
        assert_eq!(thrash.action, Action::Note);
        let spend = policy.spend.expect("spend survives");
        assert_eq!(spend.tokens, Some(20_000_000));
        assert_eq!(spend.action, Action::Halt, "the default action is halt");
        assert!(policy.loop_rule.is_none(), "an absent section is off");
        assert_eq!(policy.describe().len(), 2);
    }

    #[test]
    fn a_malformed_file_names_itself_and_the_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = write(dir.path(), "home.toml", "[thrash]\nedits = \"three\"\n");
        let err = Policy::load(&home, &dir.path().join("repo.toml")).expect_err("malformed");
        let text = format!("{err:#}");
        assert!(text.contains("home.toml"), "{text}");
        assert!(text.contains("line 2"), "{text}");
    }

    #[test]
    fn every_rule_round_trips_from_the_documented_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = write(
            dir.path(),
            "home.toml",
            "[thrash]\nedits = 3\naction = \"halt\"\n\n[spend]\ntokens = 20000000\nusd = 40.0\naction = \"halt\"\n\n[loop]\nrepeats = 3\naction = \"note\"\n\n[cache]\nblock_config_change = false\nblock_model_switch = true\n",
        );
        let policy = Policy::load(&home, &dir.path().join("repo.toml")).expect("load");
        assert!(!policy.is_empty());
        assert_eq!(policy.loop_rule.expect("loop").repeats, 3);
        assert!(policy.cache.expect("cache").block_model_switch);
        assert_eq!(policy.describe().len(), 4);
    }
}
