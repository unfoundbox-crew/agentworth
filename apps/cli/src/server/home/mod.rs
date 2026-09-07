//! The gateway behind `apps/home` (docs: apps/home/DESIGN.md, docs/specs/home.md). Split into
//! four modules along the four parts of the job that built it:
//!
//! - `protocol`: the wire types, mirroring `apps/home/src/protocol.ts` field for field.
//! - `directions`: `home_directions` persistence and the derive-on-read `spentTokens`/
//!   `reached`/`state` logic.
//! - `transcript_feed`: harness-transcript-to-`speech`/`work`/`stop` frames.
//! - `gateway`: the WebSocket itself, the herdr presence poll/subscription, and steering.

mod directions;
mod gateway;
mod protocol;
mod transcript_feed;

pub use gateway::{router, spawn, HomeHandle};
