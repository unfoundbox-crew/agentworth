//! How a session closed: the three states the `hook_recovery` gate reasons about.
//!
//! The names come from fleet decisions v1 section 6. A session that wrote a handoff closed
//! cleanly; one that died mid-lane but came back off the spool with its compaction history
//! (`forgotten`) intact is recovered, not lost; anything else is lost and says so instead of
//! passing off a partial resume as a whole one.
//!
//! This module is deliberately pure: it names the decision, while the evidence (a handoff
//! row, a spool drain, compaction rounds) is gathered by its callers.

use serde::{Deserialize, Serialize};

/// The three close states. Never two: "it crashed" without saying whether the spool brought
/// it back is how a partial resume gets mistaken for a whole one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CloseState {
    ClosedWithHandoff,
    CrashedRecovered,
    CrashedLost,
}

impl CloseState {
    pub fn as_str(self) -> &'static str {
        match self {
            CloseState::ClosedWithHandoff => "closed-with-handoff",
            CloseState::CrashedRecovered => "crashed-recovered",
            CloseState::CrashedLost => "crashed-lost",
        }
    }
}

/// Sorts a session into its close state.
///
/// - `has_handoff`: the session wrote a handoff (or a receipted bypass) at close.
/// - `recovered_from_spool`: every event the session's close depends on reached the index
///   through the spool after the crash -- the write-ahead path, not a re-telling.
/// - `forgotten_intact`: the session's compaction rounds survived the crash, so a resume
///   still knows what was summarised away and cannot re-propose it.
pub fn classify_close(
    has_handoff: bool,
    recovered_from_spool: bool,
    forgotten_intact: bool,
) -> CloseState {
    if has_handoff {
        CloseState::ClosedWithHandoff
    } else if recovered_from_spool && forgotten_intact {
        CloseState::CrashedRecovered
    } else {
        CloseState::CrashedLost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handoff_at_close_is_closed_no_matter_what_else_happened() {
        assert_eq!(
            classify_close(true, true, true),
            CloseState::ClosedWithHandoff
        );
        assert_eq!(
            classify_close(true, false, false),
            CloseState::ClosedWithHandoff,
            "a handoff outranks a crash that came before it"
        );
    }

    #[test]
    fn a_kill_mid_lane_recovers_when_the_spool_and_the_forgotten_survive() {
        assert_eq!(
            classify_close(false, true, true),
            CloseState::CrashedRecovered
        );
    }

    #[test]
    fn a_recovery_with_lost_compaction_history_is_lost_not_recovered() {
        assert_eq!(classify_close(false, true, false), CloseState::CrashedLost);
    }

    #[test]
    fn nothing_recovered_is_lost() {
        assert_eq!(classify_close(false, false, true), CloseState::CrashedLost);
        assert_eq!(classify_close(false, false, false), CloseState::CrashedLost);
    }

    #[test]
    fn every_state_names_itself_in_kebab_case() {
        assert_eq!(CloseState::ClosedWithHandoff.as_str(), "closed-with-handoff");
        assert_eq!(CloseState::CrashedRecovered.as_str(), "crashed-recovered");
        assert_eq!(CloseState::CrashedLost.as_str(), "crashed-lost");
    }
}
