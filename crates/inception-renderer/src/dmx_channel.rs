//! The DMX channel-numbering convention.
//!
//! DMX addresses channels `1..=512` (the domain convention lighting
//! people actually use), but a Rust buffer is naturally `0`-indexed. To
//! avoid different parts of the codebase quietly disagreeing about which
//! convention is in effect (item 19 of the task brief), [`DmxChannel`]
//! is the *only* place a `1..=512` channel number is allowed to exist;
//! everything downstream (e.g. [`crate::frame::UniverseFrame`]) works in
//! plain `0`-based buffer indices, and the conversion between the two
//! happens in exactly one place — [`DmxChannel::index`].

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DmxChannel(u16);

impl DmxChannel {
    pub const MIN: u16 = 1;
    pub const MAX: u16 = 512;

    /// `None` if `channel` is outside `1..=512`.
    pub fn new(channel: u16) -> Option<DmxChannel> {
        if (Self::MIN..=Self::MAX).contains(&channel) {
            Some(DmxChannel(channel))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    /// The `0`-based buffer index this channel corresponds to.
    pub const fn index(self) -> usize {
        (self.0 - 1) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_full_valid_range() {
        assert!(DmxChannel::new(1).is_some());
        assert!(DmxChannel::new(512).is_some());
    }

    #[test]
    fn rejects_out_of_range_values() {
        assert_eq!(DmxChannel::new(0), None);
        assert_eq!(DmxChannel::new(513), None);
    }

    #[test]
    fn index_is_zero_based() {
        assert_eq!(DmxChannel::new(1).unwrap().index(), 0);
        assert_eq!(DmxChannel::new(512).unwrap().index(), 511);
    }
}
