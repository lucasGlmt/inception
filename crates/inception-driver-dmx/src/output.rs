//! DMX output abstraction — what actually happens to a rendered
//! [`UniverseFrame`] is entirely up to the implementor. Virtual outputs and
//! the serial ENTTEC implementation share this same narrow interface.

use inception_core::UniverseId;
use inception_renderer::UniverseFrame;

pub trait DmxOutput {
    type Error;

    fn send(&mut self, universe: UniverseId, frame: &UniverseFrame) -> Result<(), Self::Error>;

    /// Flushes/closes the underlying output when needed. Virtual outputs and
    /// transports closed automatically on drop may keep the default no-op.
    fn close(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
