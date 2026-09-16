//! DMX output abstraction — what actually happens to a rendered
//! [`UniverseFrame`] is entirely up to the implementor. This milestone
//! ships no real hardware driver (see `AGENTS.md`'s "hors scope": no
//! Enttec, USB, serial, Art-Net or sACN yet) — only virtual
//! implementations, for tests, benchmarks, and running the pipeline with
//! no physical rig attached at all.

use inception_core::UniverseId;
use inception_renderer::UniverseFrame;

pub trait DmxOutput {
    type Error;

    fn send(&mut self, universe: UniverseId, frame: &UniverseFrame) -> Result<(), Self::Error>;
}
