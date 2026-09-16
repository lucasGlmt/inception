//! A [`DmxOutput`] that discards every frame. Useful for running the
//! pipeline (benchmarks, a headless runtime) with no output at all and
//! no need to allocate anywhere to record one.

use inception_core::UniverseId;
use inception_renderer::UniverseFrame;

use crate::output::DmxOutput;

#[derive(Debug, Clone, Copy, Default)]
pub struct NullDmxOutput;

impl DmxOutput for NullDmxOutput {
    type Error = std::convert::Infallible;

    fn send(&mut self, _universe: UniverseId, _frame: &UniverseFrame) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_any_frame_without_error() {
        let mut output = NullDmxOutput;
        output.send(UniverseId(1), &UniverseFrame::black()).unwrap();
    }
}
