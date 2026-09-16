//! The resolved physical mapping: which DMX channel(s) a fixture's
//! attributes land on.
//!
//! Produced by `inception-linker` from validated fixture definitions and a
//! physical patch. The renderer itself never parses, resolves names, or
//! validates DMX address collisions; it only reads this numeric mapping.

use inception_core::{FixtureId, UniverseId};

use crate::dmx_channel::DmxChannel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmxChannelMapping {
    pub universe: UniverseId,
    pub channel: DmxChannel,
}

/// A single fixture's RGB attribute, mapped to three channels in one
/// universe. Fixtures spanning multiple universes for one color attribute
/// aren't modeled — an unusual setup this milestone doesn't need to
/// support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbChannelMapping {
    pub universe: UniverseId,
    pub red: DmxChannel,
    pub green: DmxChannel,
    pub blue: DmxChannel,
}

/// One fixture's resolved wiring. `None` for an attribute means this
/// fixture doesn't respond to it at all (e.g. an intensity-only fixture
/// has `color: None`) — not "unmapped due to an error".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedFixture {
    pub id: FixtureId,
    pub intensity: Option<DmxChannelMapping>,
    pub color: Option<RgbChannelMapping>,
}

/// The whole show's resolved physical mapping — what a future
/// `inception-linker` will eventually produce. A plain `Vec` (not a map):
/// render order only needs to visit every fixture once, in any order, so
/// there's no lookup-by-id need here that would justify anything fancier.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedRig {
    pub fixtures: Vec<ResolvedFixture>,
}
