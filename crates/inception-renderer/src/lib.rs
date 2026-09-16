//! Converts semantic lighting state into DMX universe buffers.
//!
//! ```text
//! LightingState (inception-core)
//!       +
//! ResolvedRig (already physically resolved — no linker yet)
//!       ↓
//!   render()
//!       ↓
//! UniverseFrame (per universe)
//! ```
//!
//! Never parses Lux, never does name resolution, never knows a role or
//! rig by name, and never talks to a physical device — see this crate's
//! module docs and `AGENTS.md`'s lighting-state/DMX separation.
//! `inception-driver-dmx` is the next layer down, consuming
//! [`UniverseFrame`] to actually send (or, for now, just record) it.

pub mod convert;
pub mod dmx_channel;
pub mod frame;
pub mod mapping;
pub mod render;

pub use convert::{intensity_to_dmx8, u16_to_dmx8};
pub use dmx_channel::DmxChannel;
pub use frame::UniverseFrame;
pub use mapping::{DmxChannelMapping, ResolvedFixture, ResolvedRig, RgbChannelMapping};
pub use render::render;
