//! Launchpad X input driver: MIDI raw -> semantic
//! `inception_core::InputEvent`. Everything above this crate — the
//! runtime, `EventRouter`, Lux source itself — never sees a note number,
//! velocity, MIDI channel or SysEx; see this crate's `listener` module
//! doc, `AGENTS.md`'s "do not leak protocol-specific types into
//! `inception-core`" driver rule, and `docs/rfcs/0007-event-system.md`.

pub mod error;
pub mod listener;
pub mod mapping;

pub use error::LaunchpadError;
pub use listener::{LAUNCHPAD_DEVICE_ID, LaunchpadListener};
pub use mapping::{note_to_pad, pad_to_note};
