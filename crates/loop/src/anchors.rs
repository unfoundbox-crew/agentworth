//! The shared field: the ids that let one product's record find another's, without either
//! product learning about the other.
//!
//! SpacePilot already mints a `run_id` per served call and already calls it the join key.
//! MotionVector already prints a receipt with `canonical_sha256` and `out_sha256`. Both appear in
//! the agent's own tool result, so the transcript is the carrier and AgentWorth only has to
//! notice. Nothing here calls a network, and nothing here is invented: an anchor is a token that
//! was literally in the text.

use crate::predict::Prediction;
use crate::support::{hash_file, MAX_HASH_BYTES};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// How far back from a uuid a `run_id` label may sit and still be the label for it.
const RUN_ID_WINDOW: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    RunId,
    Sha256,
    PaneId,
}

impl AnchorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AnchorKind::RunId => "run_id",
            AnchorKind::Sha256 => "sha256",
            AnchorKind::PaneId => "pane_id",
        }
    }
}

/// One id seen at one point in one session. The primary key of `trace_anchors`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Anchor {
    pub session_id: String,
    pub seq: u64,
    pub kind: AnchorKind,
    pub value: String,
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b",
        )
        .expect("the uuid pattern compiles")
    })
}

fn run_id_label_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)run[_-]?id").expect("the run_id pattern compiles"))
}

fn sha256_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b[0-9a-f]{64}\b").expect("the sha256 pattern compiles"))
}

/// Every anchor in one tool result, deduplicated within that result.
///
/// A uuid counts as a `run_id` only when a `run_id` / `run-id` / `runId` label sits within
/// `RUN_ID_WINDOW` characters before it. Without that, a uuid is just a uuid -- transcripts are
/// full of them, and an anchor that joins the wrong two records is worse than no anchor.
pub fn extract_anchors(session_id: &str, seq: u64, tool_response_text: &str) -> Vec<Anchor> {
    let text = tool_response_text;
    let mut seen: BTreeSet<(AnchorKind, String)> = BTreeSet::new();
    let mut anchors = Vec::new();

    for found in uuid_re().find_iter(text) {
        let mut start = found.start().saturating_sub(RUN_ID_WINDOW);
        while start > 0 && !text.is_char_boundary(start) {
            start -= 1;
        }
        let labelled = text
            .get(start..found.start())
            .is_some_and(|window| run_id_label_re().is_match(window));
        if !labelled {
            continue;
        }
        let value = found.as_str().to_lowercase();
        if seen.insert((AnchorKind::RunId, value.clone())) {
            anchors.push(Anchor {
                session_id: session_id.to_string(),
                seq,
                kind: AnchorKind::RunId,
                value,
            });
        }
    }

    for found in sha256_re().find_iter(text) {
        let value = found.as_str().to_lowercase();
        if seen.insert((AnchorKind::Sha256, value.clone())) {
            anchors.push(Anchor {
                session_id: session_id.to_string(),
                seq,
                kind: AnchorKind::Sha256,
                value,
            });
        }
    }

    anchors
}

/// Hashes the paths a `Write` or `Edit` predicted, after it ran. A path that no longer exists, or
/// one over the cap, contributes nothing rather than a placeholder.
pub fn hash_paths_for_anchors(session_id: &str, seq: u64, prediction: &Prediction) -> Vec<Anchor> {
    prediction
        .paths
        .iter()
        .filter_map(|path| hash_file(path, MAX_HASH_BYTES).ok().flatten())
        .map(|value| Anchor {
            session_id: session_id.to_string(),
            seq,
            kind: AnchorKind::Sha256,
            value,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_spacepilot_response_yields_one_run_id() {
        let response = r#"{"run_id":"018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f","metric":"tokens_per_second","value":41.2}"#;
        let anchors = extract_anchors("s1", 12, response);
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].kind, AnchorKind::RunId);
        assert_eq!(anchors[0].value, "018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f");
        assert_eq!(anchors[0].seq, 12);
        assert_eq!(anchors[0].session_id, "s1");
    }

    #[test]
    fn a_motionvector_receipt_yields_both_of_its_hashes() {
        let receipt = "Receipt v1\n  document.canonical_sha256 \
             = 3b1f9a0c2d4e6f8a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a\n  \
             render.out_sha256 = a1b2c3d4e5f60718293a4b5c6d7e8f901a2b3c4d5e6f708192a3b4c5d6e7f809\n";
        let anchors = extract_anchors("s1", 3, receipt);
        assert_eq!(anchors.len(), 2);
        assert!(anchors.iter().all(|a| a.kind == AnchorKind::Sha256));
        assert_eq!(
            anchors[0].value,
            "3b1f9a0c2d4e6f8a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a"
        );
    }

    #[test]
    fn a_response_with_neither_yields_nothing() {
        assert!(extract_anchors("s1", 1, "wrote 3 files, 0 errors").is_empty());
    }

    #[test]
    fn an_unlabelled_uuid_is_not_a_run_id() {
        let response = r#"{"id":"018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f"}"#;
        assert!(extract_anchors("s1", 1, response).is_empty());
    }

    #[test]
    fn a_uuid_too_far_from_its_label_is_not_a_run_id() {
        let far = format!(
            "run_id{}018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f",
            " ".repeat(60)
        );
        assert!(extract_anchors("s1", 1, &far).is_empty());
    }

    #[test]
    fn the_three_spellings_of_the_label_all_count() {
        for label in ["run_id", "run-id", "runId", "RUN_ID"] {
            let text = format!("{label}: 018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f");
            assert_eq!(extract_anchors("s1", 1, &text).len(), 1, "{label}");
        }
    }

    #[test]
    fn the_same_id_twice_in_one_response_is_one_anchor() {
        let text = "run_id=018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f run_id=018f3a2c-7b41-7c9d-9f2e-1a2b3c4d5e6f";
        assert_eq!(extract_anchors("s1", 1, text).len(), 1);
    }

    #[test]
    fn a_written_file_anchors_on_its_hash_and_a_missing_one_anchors_on_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.mp4");
        std::fs::write(&path, "hello\n").expect("write");
        let prediction = Prediction {
            paths: vec![path, dir.path().join("never-written.mp4")],
            command: None,
            known: true,
        };
        let anchors = hash_paths_for_anchors("s1", 4, &prediction);
        assert_eq!(anchors.len(), 1);
        assert_eq!(
            anchors[0].value,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn a_bash_prediction_has_no_paths_to_anchor() {
        let prediction = Prediction {
            paths: Vec::<PathBuf>::new(),
            command: Some("ls".to_string()),
            known: false,
        };
        assert!(hash_paths_for_anchors("s1", 1, &prediction).is_empty());
    }
}
