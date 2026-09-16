//! Pure, deterministic domain types and algorithms shared across the
//! Inception runtime. No USB, MIDI, filesystem, networking or compiler
//! dependencies — see `AGENTS.md`.

pub mod time;

pub use time::{Clock, Duration, MonotonicClock, Timestamp, VirtualClock};
