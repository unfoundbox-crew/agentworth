//! The loop: AgentWorth stops reading the tape afterwards and becomes the return path.
//!
//! The harness's own hooks tell AgentWorth what an agent is about to do -- `PreToolUse` is the
//! efference copy of the motor command, and `crate::predict` turns it into a predicted write set.
//! `crate::state` folds the lifecycle events into four states per session, with one rule that
//! matters: a subagent bumps its parent's sequence and never revives an idle pane. At `Stop`,
//! `crate::observe` asks git what actually changed and splits it into what this session predicted
//! and what the world did, while `crate::support` re-hashes the paths the session read and names
//! the session that moved any of them. `crate::anchors` indexes the ids the tool results already
//! carry -- a SpacePilot `run_id`, a MotionVector receipt's hashes -- so "this task, on that
//! machine, produced this output" is one query and not an integration.
//!
//! Everything here is pure and synchronous: no socket, no database, no clock of its own, no model
//! call, and `crate::spool` is the offline path that makes the loop work with nothing running.
//! `docs/specs/loop.md` is the design.

pub mod anchors;
pub mod hook;
pub mod observe;
pub mod predict;
pub mod spool;
pub mod state;
pub mod support;

pub use anchors::{extract_anchors, hash_paths_for_anchors, Anchor, AnchorKind};
pub use hook::{HookEvent, HookEventName, PANE_ID_ENV};
pub use observe::{
    absolutise, classify, observe_checkout, observe_checkout_with_git, Classified, Observation,
    ObservedChange,
};
pub use predict::{predicted_writes, Intent, Prediction};
pub use spool::{SpoolRead, SpoolReader, SpoolWriter};
pub use state::{AgentState, LoopState, SessionLive, Transition};
pub use support::{
    drift, hash_file, support_from_read, Drift, SupportEntry, Writer, MAX_HASH_BYTES,
};
