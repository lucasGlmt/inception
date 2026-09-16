//! DMX output drivers.
//!
//! Only virtual implementations exist here — see `AGENTS.md`'s "hors
//! scope" and this crate's module docs. A real hardware driver (Enttec,
//! Art-Net, sACN, ...) is a later, separate crate built against the same
//! [`DmxOutput`] trait, once the fully virtual pipeline is validated.

pub mod null;
pub mod output;
pub mod recording;

pub use null::NullDmxOutput;
pub use output::DmxOutput;
pub use recording::RecordingDmxOutput;
