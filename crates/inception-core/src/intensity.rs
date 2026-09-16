//! The semantic representation of light intensity — never a DMX byte.
//! See `AGENTS.md`'s and this milestone's separation between
//! `LightingState` (semantic) and the renderer's DMX output.

/// `0` is `0%`, `65535` is `100%`. Every `u16` value is a valid
/// intensity — unlike, say, a percent string, there's no invalid raw
/// value to reject — so [`Intensity::new`] is infallible.
/// [`Intensity::from_percent`] exists for the (checked, `0..=100`) case
/// of constructing one from a percent instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Intensity(u16);

impl Intensity {
    pub const ZERO: Intensity = Intensity(0);
    pub const MAX: Intensity = Intensity(u16::MAX);

    pub const fn new(raw: u16) -> Self {
        Intensity(raw)
    }

    /// `None` if `percent > 100`.
    pub fn from_percent(percent: u8) -> Option<Intensity> {
        if percent > 100 {
            return None;
        }
        Some(Intensity(((percent as u32 * u16::MAX as u32) / 100) as u16))
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_and_max_match_the_documented_range() {
        assert_eq!(Intensity::ZERO.raw(), 0);
        assert_eq!(Intensity::MAX.raw(), 65535);
    }

    #[test]
    fn from_percent_rejects_out_of_range_values() {
        assert_eq!(Intensity::from_percent(101), None);
        assert!(Intensity::from_percent(100).is_some());
    }

    #[test]
    fn from_percent_extremes_match_zero_and_max() {
        assert_eq!(Intensity::from_percent(0), Some(Intensity::ZERO));
        assert_eq!(Intensity::from_percent(100), Some(Intensity::MAX));
    }
}
