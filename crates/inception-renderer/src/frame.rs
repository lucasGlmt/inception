//! A single DMX universe's channel values.

use crate::dmx_channel::DmxChannel;

/// 512 DMX channel slots. `[0]` is DMX channel 1, `[511]` is channel 512
/// — see [`crate::dmx_channel`]'s docs on why the `1..=512`/`0..512`
/// distinction is confined to [`DmxChannel`] rather than showing up here
/// too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseFrame {
    slots: [u8; 512],
}

impl UniverseFrame {
    /// A frame with every channel at `0`. Also this type's `Default`.
    pub fn black() -> Self {
        Self { slots: [0; 512] }
    }

    pub fn set(&mut self, channel: DmxChannel, value: u8) {
        self.slots[channel.index()] = value;
    }

    pub fn get(&self, channel: DmxChannel) -> u8 {
        self.slots[channel.index()]
    }

    /// The raw `0`-based buffer, for bulk inspection/transmission.
    pub fn as_slice(&self) -> &[u8; 512] {
        &self.slots
    }
}

impl Default for UniverseFrame {
    fn default() -> Self {
        Self::black()
    }
}

/// `frame[0]` is DMX channel 1. Mainly for tests, which tend to want to
/// assert on raw buffer positions directly (e.g. `slots[0]`, `slots[4]`).
impl std::ops::Index<usize> for UniverseFrame {
    type Output = u8;

    fn index(&self, index: usize) -> &u8 {
        &self.slots[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_frame_is_all_zero() {
        let frame = UniverseFrame::black();
        assert_eq!(frame.as_slice(), &[0u8; 512]);
    }

    #[test]
    fn default_matches_black() {
        assert_eq!(UniverseFrame::default(), UniverseFrame::black());
    }

    #[test]
    fn set_and_get_round_trip_through_the_same_channel() {
        let mut frame = UniverseFrame::black();
        let channel = DmxChannel::new(1).unwrap();
        frame.set(channel, 200);
        assert_eq!(frame.get(channel), 200);
        assert_eq!(frame[0], 200);
    }

    #[test]
    fn channels_are_independent() {
        let mut frame = UniverseFrame::black();
        frame.set(DmxChannel::new(1).unwrap(), 128);
        assert_eq!(frame[1], 0);
        assert_eq!(frame[4], 0);
    }
}
