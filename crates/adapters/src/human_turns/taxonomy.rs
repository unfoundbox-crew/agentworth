//! The deterministic turn taxonomy, ported verbatim from the reviewed Python pass
//! (`tools/behavioral_insights.py` FRICTION_RULES + the vocabulary clusters that
//! `tools/insights/insights_query.py` v1.4 stepped over). One rule order, one list, so this
//! Rust classification and the prototype numbers it replaces cannot disagree. Bump
//! [`INGESTION_VERSION`] when anything here changes what a turn's derived features would be.

use regex::Regex;
use std::sync::OnceLock;

/// No friction detected on this turn. Stored on every row, never implicit: a NULL would read
/// as "not looked at", and every stored row has been looked at.
pub const NONE: &str = "none";

/// Ship the whole classification pipeline together; a change to these rules rewrites every
/// turn's derived features, so file fingerprints alone cannot decide staleness. The
/// orchestrator wipes and re-ingests everything on a bump.
pub const INGESTION_VERSION: i64 = 1;

/// The dashboard's friction vocabulary, first match wins — definitions and order quoted
/// character-for-character from the builder's `FRICTION_RULES`.
pub const FRICTION_RULES: &[(&str, &str)] = &[
    (
        "loop_interruption",
        r"\b(stop|stuck|looping|again|same thing|infinite loop|repeating|repeats|cancel|kill)\b",
    ),
    (
        "hallucination_pushback",
        r"\b(fake|hallucin|wrong|check path|check \$path|does not exist|fabricated|invented|not real|404|liar|lying)\b",
    ),
    (
        "autonomy_nudge",
        r"\b(just do it|don'?t ask|run it|proceed|go ahead|stop asking|don'?t wait|execute|stop planning|do it)\b",
    ),
    (
        "context_amnesia",
        r"\b(you forgot|read the summary|as i said|already told you|remember|already said|forgotten|amnesia)\b",
    ),
    (
        "sandbox_error",
        r"\b(permission denied|command failed|exit code|syntax error|segfault|cannot find|enoent)\b",
    ),
    (
        "sarcasm_cynicism",
        r"\b(code is dead|agi next week|magic|genius|unbelievable|lol|surely|classic)\b",
    ),
];

/// The vocabulary clusters the insights_query.py v1.4 pass counts over every cached turn —
/// case-insensitive whole-word matches over the product's own standing vocabulary.
pub const VOCAB_CLUSTERS: &[(&str, &[&str])] = &[
    (
        "Fleet & agentic",
        &["agentworth", "spacepilot", "fleet", "herdr", "headless"],
    ),
    (
        "Architecture",
        &[
            "mcp", "archie", "docir", "receipts", "rust", "doppler", "cargo", "webgpu",
        ],
    ),
    (
        "Philosophy",
        &[
            "truth",
            "compaction",
            "sovereign",
            "probabilistic",
            "narrow waist",
            "unflown",
        ],
    ),
    (
        "Market / models",
        &["claude", "fable", "gemini", "deepseek", "anthropic"],
    ),
];

/// One compiled classifier per friction rule, in FRICTION_RULES order so the first match
/// maps 1:1 back to the rule's own `&'static str`.
fn friction_matchers() -> &'static [Regex] {
    static MATCHERS: OnceLock<Vec<Regex>> = OnceLock::new();
    MATCHERS.get_or_init(|| {
        FRICTION_RULES
            .iter()
            .map(|(_, pattern)| compile(pattern))
            .collect()
    })
}

/// One compiled matcher per vocabulary term, in VOCAB_CLUSTERS order; multi-word terms join
/// on any whitespace, word-bounded both ends.
fn vocab_matchers() -> &'static Vec<(String, Regex)> {
    static MATCHERS: OnceLock<Vec<(String, Regex)>> = OnceLock::new();
    MATCHERS.get_or_init(|| {
        VOCAB_CLUSTERS
            .iter()
            .flat_map(|(_, terms)| terms.iter())
            .map(|term| {
                let joined = term
                    .split_whitespace()
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join(r"\s+");
                (term.to_string(), compile(&format!(r"\b{joined}\b")))
            })
            .collect()
    })
}

fn compile(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}"))
        .unwrap_or_else(|e| panic!("taxonomy regex failed to compile: {pattern}: {e}"))
}

/// The first friction rule the turn's text matches, or [`NONE`].
pub fn classify_friction(text: &str) -> &'static str {
    for (idx, matcher) in friction_matchers().iter().enumerate() {
        if matcher.is_match(text) {
            let (name, _) = FRICTION_RULES[idx];
            return name;
        }
    }
    NONE
}

/// `(term, occurrences)` for every vocabulary term the text mentions, lowercased.
pub fn vocab_matches(text: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for (term, matcher) in vocab_matchers() {
        let uses = matcher.find_iter(text).count() as u64;
        if uses > 0 {
            out.push((term.clone(), uses));
        }
    }
    out
}
