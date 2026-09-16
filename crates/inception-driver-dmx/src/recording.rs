//! A [`DmxOutput`] that remembers the last frame sent to each universe —
//! the primary way this milestone's tests observe rendered DMX without
//! any real hardware.

use std::collections::HashMap;

use inception_core::UniverseId;
use inception_renderer::UniverseFrame;

use crate::output::DmxOutput;

#[derive(Debug, Clone, Default)]
pub struct RecordingDmxOutput {
    frames: HashMap<UniverseId, UniverseFrame>,
}

impl RecordingDmxOutput {
    pub fn new() -> Self {
        Self::default()
    }

    /// The most recent frame sent to `universe`, if any.
    pub fn last_frame(&self, universe: UniverseId) -> Option<&UniverseFrame> {
        self.frames.get(&universe)
    }
}

impl DmxOutput for RecordingDmxOutput {
    type Error = std::convert::Infallible;

    fn send(&mut self, universe: UniverseId, frame: &UniverseFrame) -> Result<(), Self::Error> {
        self.frames.insert(universe, frame.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use inception_renderer::DmxChannel;

    use super::*;

    #[test]
    fn records_the_last_frame_per_universe() {
        let mut output = RecordingDmxOutput::new();
        let mut frame = UniverseFrame::black();
        frame.set(DmxChannel::new(1).unwrap(), 128);

        output.send(UniverseId(1), &frame).unwrap();

        assert_eq!(output.last_frame(UniverseId(1)).unwrap()[0], 128);
    }

    #[test]
    fn unsent_universe_has_no_recorded_frame() {
        let output = RecordingDmxOutput::new();
        assert_eq!(output.last_frame(UniverseId(1)), None);
    }

    #[test]
    fn sending_again_replaces_the_previous_frame() {
        let mut output = RecordingDmxOutput::new();
        let mut first = UniverseFrame::black();
        first.set(DmxChannel::new(1).unwrap(), 10);
        output.send(UniverseId(1), &first).unwrap();

        let mut second = UniverseFrame::black();
        second.set(DmxChannel::new(1).unwrap(), 20);
        output.send(UniverseId(1), &second).unwrap();

        assert_eq!(output.last_frame(UniverseId(1)).unwrap()[0], 20);
    }

    #[test]
    fn tracks_multiple_universes_independently() {
        let mut output = RecordingDmxOutput::new();
        output.send(UniverseId(1), &UniverseFrame::black()).unwrap();
        output.send(UniverseId(2), &UniverseFrame::black()).unwrap();

        assert!(output.last_frame(UniverseId(1)).is_some());
        assert!(output.last_frame(UniverseId(2)).is_some());
        assert!(output.last_frame(UniverseId(3)).is_none());
    }
}
