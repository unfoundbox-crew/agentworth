//! The governor's decisions: pure functions over what the session has already done.
//!
//! Nothing here reads a file, opens a socket or knows the time. The caller brings the ledger of
//! edits and verifications, the spend from `crate::meter`, the loop count, and whether the
//! session is suspended; this module says what to do about it and writes the ground truth the
//! model will read. `crate::gate` turns a decision into what the harness understands.
//!
//! The rule under the thrash counter: a passing verification clears every path, not one. A green
//! run is proof for the tree, and a run that passes after four files were edited says nothing
//! about which of the four the model should stop touching.

use crate::meter::SessionSpend;
use crate::policy::Policy;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What a tripped rule does. `Note` speaks to the model; `Halt` stops the next model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Note,
    Halt,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Note => "note",
            Action::Halt => "halt",
        }
    }

    pub(crate) fn halt() -> Self {
        Action::Halt
    }

    pub(crate) fn note() -> Self {
        Action::Note
    }
}

/// Which rule tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    Thrash,
    Spend,
    Loop,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Thrash => "thrash",
            Rule::Spend => "spend",
            Rule::Loop => "loop",
        }
    }
}

/// One rule tripping, with the sentence for the person and the sentence for the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub rule: Rule,
    pub action: Action,
    /// For the person and the row: what happened, in one line.
    pub reason: String,
    /// For the model: the receipt, files and sequence numbers included.
    pub ground_truth: String,
    pub evidence: serde_json::Value,
}

/// The last verification that failed, kept so the halt can quote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub command: String,
    /// The last line of that command's output: the part that says what broke.
    pub line: String,
    pub seq: u64,
}

/// A session whose spend cap has tripped, until `archie policy lift` clears it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suspension {
    pub session_id: String,
    pub reason: String,
    pub since: DateTime<Utc>,
}

/// Edits per file since the last passing verification, and the failure that stands.
#[derive(Debug, Clone, Default)]
pub struct EditLedger {
    edits: HashMap<String, usize>,
    first_edit_seq: HashMap<String, u64>,
    failure_by_path: HashMap<String, Failure>,
    last_failure: Option<Failure>,
    last_pass_seq: Option<u64>,
}

impl EditLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_edit(&mut self, path: &str, seq: u64) {
        *self.edits.entry(path.to_string()).or_insert(0) += 1;
        self.first_edit_seq.entry(path.to_string()).or_insert(seq);
    }

    /// A verification run and its result. `path_hint` attaches a failure to one file when the
    /// caller knows which; without it the failure stands for every path.
    pub fn record_verification(
        &mut self,
        path_hint: Option<&str>,
        passed: bool,
        seq: u64,
        command: &str,
        last_output_line: &str,
    ) {
        if passed {
            self.edits.clear();
            self.first_edit_seq.clear();
            self.failure_by_path.clear();
            self.last_failure = None;
            self.last_pass_seq = Some(seq);
            return;
        }
        let failure = Failure {
            command: command.to_string(),
            line: last_output_line.to_string(),
            seq,
        };
        if let Some(path) = path_hint {
            self.failure_by_path.insert(path.to_string(), failure.clone());
        }
        self.last_failure = Some(failure);
    }

    pub fn edits_since_pass(&self, path: &str) -> usize {
        self.edits.get(path).copied().unwrap_or(0)
    }

    pub fn last_failure(&self, path: &str) -> Option<&Failure> {
        self.failure_by_path.get(path).or(self.last_failure.as_ref())
    }

    /// The sequence of the last verification that passed, if one ever did.
    pub fn last_pass_seq(&self) -> Option<u64> {
        self.last_pass_seq
    }

    /// Every path with at least one edit since the last pass, in a stable order.
    pub fn paths(&self) -> Vec<&str> {
        let mut paths: Vec<&str> = self.edits.keys().map(String::as_str).collect();
        paths.sort_unstable();
        paths
    }
}

/// The decisions themselves. A unit type so the entry point has a name to hang on.
pub struct Governor;

impl Governor {
    /// Every rule that trips, in rule order. An empty vector is the common case and means the
    /// turn proceeds untouched.
    pub fn evaluate(
        policy: &Policy,
        ledger: &EditLedger,
        spend: &SessionSpend,
        repeats: usize,
        suspended: Option<&Suspension>,
    ) -> Vec<Decision> {
        let mut decisions = Vec::new();

        if let Some(rule) = policy.thrash {
            for path in ledger.paths() {
                let edits = ledger.edits_since_pass(path);
                if rule.edits == 0 || edits < rule.edits {
                    continue;
                }
                decisions.push(thrash_decision(path, edits, rule.action, ledger));
            }
        }

        if let Some(suspension) = suspended {
            decisions.push(Decision {
                rule: Rule::Spend,
                action: Action::Halt,
                reason: suspension.reason.clone(),
                ground_truth: format!(
                    "This session is suspended: {}. It stays suspended until `archie policy lift {}` or a higher cap in policy.toml.",
                    suspension.reason, suspension.session_id
                ),
                evidence: serde_json::json!({
                    "session_id": suspension.session_id,
                    "since": suspension.since,
                    "suspended": true,
                }),
            });
        } else if let Some(rule) = policy.spend {
            if let Some(decision) = spend_decision(&rule, spend) {
                decisions.push(decision);
            }
        }

        if let Some(rule) = policy.loop_rule {
            if rule.repeats > 0 && repeats >= rule.repeats {
                let reason = format!(
                    "the same tool call has run {repeats} times in a row, at or over the limit of {}",
                    rule.repeats
                );
                decisions.push(Decision {
                    rule: Rule::Loop,
                    action: rule.action,
                    ground_truth: format!(
                        "{reason}. The call has not changed and neither has its result; change the call or say what you are stuck on."
                    ),
                    reason,
                    evidence: serde_json::json!({"repeats": repeats, "limit": rule.repeats}),
                });
            }
        }

        decisions
    }
}

fn thrash_decision(path: &str, edits: usize, action: Action, ledger: &EditLedger) -> Decision {
    let mut ground_truth = format!(
        "{path} edited {edits} times since the last passing verification."
    );
    if let Some(failure) = ledger.last_failure(path) {
        ground_truth.push_str(&format!(
            " Last failure: `{}` -> {} [seq {}].",
            failure.command, failure.line, failure.seq
        ));
    }
    match ledger.last_pass_seq() {
        Some(seq) => ground_truth.push_str(&format!(" No run has passed since seq {seq}.")),
        None => ground_truth.push_str(" No run has passed in this session."),
    }
    Decision {
        rule: Rule::Thrash,
        action,
        reason: format!("{path} has been edited {edits} times with nothing passing in between"),
        ground_truth,
        evidence: serde_json::json!({
            "path": path,
            "edits": edits,
            "last_failure": ledger.last_failure(path),
            "last_pass_seq": ledger.last_pass_seq(),
        }),
    }
}

fn spend_decision(rule: &crate::policy::SpendRule, spend: &SessionSpend) -> Option<Decision> {
    let over_tokens = rule.tokens.is_some_and(|cap| spend.tokens > cap);
    let over_usd = rule.usd.is_some_and(|cap| spend.usd > cap);
    if !over_tokens && !over_usd {
        return None;
    }
    let mut parts = Vec::new();
    if over_tokens {
        parts.push(format!(
            "{} tokens against a cap of {}",
            spend.tokens,
            rule.tokens.unwrap_or_default()
        ));
    }
    if over_usd {
        parts.push(format!(
            "${:.2} at table prices against a cap of ${:.2}",
            spend.usd,
            rule.usd.unwrap_or_default()
        ));
    }
    let reason = format!("this session has spent {}", parts.join(", and "));
    Some(Decision {
        rule: Rule::Spend,
        action: rule.action,
        ground_truth: format!(
            "{reason}. This is AgentWorth's own count at the pricing table's rates over {} turns, not the provider's accounting of what is left.",
            spend.turns
        ),
        reason,
        evidence: serde_json::json!({
            "tokens": spend.tokens,
            "usd": spend.usd,
            "turns": spend.turns,
            "cap_tokens": rule.tokens,
            "cap_usd": rule.usd,
        }),
    })
}

/// Test-, build- or release-shaped commands: the ones whose exit code is evidence rather than
/// trivia. Matches the program and its first verb rather than substring-matching the whole
/// command line -- a `grep -n "test"` or `cd repo && ls` is not a passing test just because the
/// word "test" appears in it. Deliberately a small table and not a shell parser: it splits on
/// the usual separators, strips the wrappers a real invocation carries (`cd`, env assignments,
/// `sudo`, `time`, `nice`, `flock <file>`, a leading path), and checks only the program name and
/// its first argument.
pub fn is_verification_command(command: &str) -> bool {
    command
        .split(['\n'])
        .flat_map(|line| line.split("&&"))
        .flat_map(|s| s.split("||"))
        .flat_map(|s| s.split(['|', ';']))
        .any(|segment| segment_is_verification(segment.trim()))
}

/// One `&&`/`;`/`|`-separated segment of a shell command, after stripping the wrappers a real
/// invocation carries around the program that actually runs.
fn segment_is_verification(segment: &str) -> bool {
    let mut words: Vec<&str> = segment.split_whitespace().collect();

    loop {
        match words.first().copied() {
            Some("cd") => {
                words.remove(0);
                if !words.is_empty() {
                    words.remove(0);
                }
            }
            Some(w) if w.contains('=') && !w.starts_with('-') => {
                words.remove(0);
            }
            Some("sudo") | Some("time") | Some("nice") => {
                words.remove(0);
            }
            Some("flock") => {
                words.remove(0);
                if !words.is_empty() {
                    words.remove(0);
                }
            }
            _ => break,
        }
    }

    let Some(program_path) = words.first() else {
        return false;
    };
    let program = program_path.rsplit('/').next().unwrap_or(program_path);
    let verb = words.get(1).copied();

    const VERB_PROGRAMS: &[(&str, &[&str])] = &[
        (
            "cargo",
            &["test", "build", "check", "clippy", "fmt", "nextest", "run"],
        ),
        ("npm", &["test", "run", "build", "ci"]),
        ("pnpm", &["test", "run", "build", "ci"]),
        ("yarn", &["test", "run", "build", "ci"]),
        ("bun", &["test", "run", "build", "ci"]),
        ("go", &["build", "vet", "test"]),
        ("git", &["commit", "push"]),
        ("gh", &["pr", "run", "workflow"]),
        ("gradle", &[]),
        ("gradlew", &[]),
    ];
    const BARE_PROGRAMS: &[&str] = &[
        "pytest", "tsc", "eslint", "vitest", "jest", "make", "mvn", "ruff", "mypy",
    ];

    if program == "docker" {
        return verb == Some("build");
    }
    if BARE_PROGRAMS.contains(&program) {
        return true;
    }
    for (name, verbs) in VERB_PROGRAMS {
        if program != *name {
            continue;
        }
        return verbs.is_empty() || verb.is_some_and(|v| verbs.contains(&v));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::{SessionSpend, TurnUsage};
    use crate::policy::{LoopRule, SpendRule, ThrashRule};

    fn thrash_policy(edits: usize, action: Action) -> Policy {
        Policy {
            thrash: Some(ThrashRule { edits, action }),
            ..Policy::default()
        }
    }

    fn spend_of(tokens: u64, usd: f64) -> SessionSpend {
        let mut spend = SessionSpend::default();
        spend.add(&TurnUsage {
            seq: 1,
            at: Utc::now(),
            model: "claude-fable-5-1".to_string(),
            input: tokens,
            output: 0,
            cache_read: 0,
            cache_creation: 0,
            usd,
        });
        spend
    }

    #[test]
    fn three_edits_with_nothing_passing_halts_and_quotes_the_failure() {
        let mut ledger = EditLedger::new();
        ledger.record_edit("crates/loop/src/lib.rs", 1);
        ledger.record_verification(None, false, 2, "cargo test -p agentworth-loop", "error[E0425]");
        ledger.record_edit("crates/loop/src/lib.rs", 3);
        ledger.record_edit("crates/loop/src/lib.rs", 4);

        let decisions = Governor::evaluate(
            &thrash_policy(3, Action::Halt),
            &ledger,
            &SessionSpend::default(),
            0,
            None,
        );
        assert_eq!(decisions.len(), 1);
        let decision = &decisions[0];
        assert_eq!(decision.rule, Rule::Thrash);
        assert_eq!(decision.action, Action::Halt);
        assert_eq!(
            decision.ground_truth,
            "crates/loop/src/lib.rs edited 3 times since the last passing verification. \
             Last failure: `cargo test -p agentworth-loop` -> error[E0425] [seq 2]. \
             No run has passed in this session."
        );
        assert_eq!(decision.evidence["edits"], 3);
    }

    #[test]
    fn a_passing_run_clears_every_path_not_just_its_own() {
        let mut ledger = EditLedger::new();
        ledger.record_edit("a.rs", 1);
        ledger.record_edit("a.rs", 2);
        ledger.record_edit("b.rs", 3);
        ledger.record_verification(Some("a.rs"), true, 4, "cargo test", "");
        assert_eq!(ledger.edits_since_pass("a.rs"), 0);
        assert_eq!(ledger.edits_since_pass("b.rs"), 0);
        assert!(ledger.last_failure("a.rs").is_none());
        assert_eq!(ledger.last_pass_seq(), Some(4));

        ledger.record_edit("a.rs", 5);
        ledger.record_edit("a.rs", 6);
        ledger.record_edit("a.rs", 7);
        let decisions = Governor::evaluate(
            &thrash_policy(3, Action::Note),
            &ledger,
            &SessionSpend::default(),
            0,
            None,
        );
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, Action::Note);
        assert!(decisions[0].ground_truth.ends_with("No run has passed since seq 4."));
    }

    #[test]
    fn the_spend_cap_names_the_number_and_the_cap() {
        let policy = Policy {
            spend: Some(SpendRule {
                tokens: Some(20_000_000),
                usd: Some(40.0),
                action: Action::Halt,
            }),
            ..Policy::default()
        };
        let under = Governor::evaluate(
            &policy,
            &EditLedger::new(),
            &spend_of(19_000_000, 39.0),
            0,
            None,
        );
        assert!(under.is_empty());

        let over = Governor::evaluate(
            &policy,
            &EditLedger::new(),
            &spend_of(21_000_000, 41.5),
            0,
            None,
        );
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].rule, Rule::Spend);
        assert_eq!(
            over[0].reason,
            "this session has spent 21000000 tokens against a cap of 20000000, \
             and $41.50 at table prices against a cap of $40.00"
        );
    }

    #[test]
    fn a_suspended_session_halts_whatever_the_meter_says() {
        let suspension = Suspension {
            session_id: "7f3c9a2e".to_string(),
            reason: "this session has spent $41.50 against a cap of $40.00".to_string(),
            since: Utc::now(),
        };
        let decisions = Governor::evaluate(
            &Policy::default(),
            &EditLedger::new(),
            &SessionSpend::default(),
            0,
            Some(&suspension),
        );
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].rule, Rule::Spend);
        assert_eq!(decisions[0].action, Action::Halt);
        assert!(decisions[0].ground_truth.contains("archie policy lift 7f3c9a2e"));
    }

    #[test]
    fn the_loop_rule_notes_by_default_and_an_empty_policy_governs_nothing() {
        let policy = Policy {
            loop_rule: Some(LoopRule {
                repeats: 3,
                action: Action::Note,
            }),
            ..Policy::default()
        };
        assert!(Governor::evaluate(&policy, &EditLedger::new(), &SessionSpend::default(), 2, None)
            .is_empty());
        let tripped =
            Governor::evaluate(&policy, &EditLedger::new(), &SessionSpend::default(), 3, None);
        assert_eq!(tripped[0].rule, Rule::Loop);
        assert_eq!(tripped[0].action, Action::Note);

        let mut ledger = EditLedger::new();
        for seq in 1..9 {
            ledger.record_edit("a.rs", seq);
        }
        assert!(Governor::evaluate(
            &Policy::default(),
            &ledger,
            &spend_of(99_000_000, 900.0),
            9,
            None
        )
        .is_empty());
    }

    #[test]
    fn is_verification_command_matches_the_program_not_the_word_test() {
        assert!(!is_verification_command(
            r#"cd x; /usr/bin/grep -n -A4 "…test…""#
        ));
        assert!(is_verification_command("cd x && cargo test -p foo"));
        assert!(is_verification_command("FOO=1 pytest tests/"));
        assert!(!is_verification_command(r#"grep -rn "test" src"#));
        assert!(!is_verification_command("ls crates"));
        assert!(is_verification_command("git commit -m x"));
    }
}
