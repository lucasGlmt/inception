//! Converting semantic values to 8-bit DMX channel values.
//!
//! Integer-only throughout — no `f64` (item 23 of the task brief) — using
//! round-to-nearest via the standard "add half the divisor before
//! shifting" trick for a 16-to-8-bit reduction: `(value + 128) >> 8`,
//! clamped to `255` since `65535 + 128` would otherwise overflow past it.
//! This rounds rather than truncates, so e.g. `50%` (`32767`) lands on
//! `128`, not `127`.

use inception_core::Intensity;

/// `Intensity::ZERO -> 0`, `Intensity::MAX -> 255`, linearly in between.
pub fn intensity_to_dmx8(intensity: Intensity) -> u8 {
    u16_to_dmx8(intensity.raw())
}

/// The same `0..=65535 -> 0..=255` rounding rule, reused for each `Rgb`
/// channel — there is nothing intensity-specific about it.
pub fn u16_to_dmx8(value: u16) -> u8 {
    (((value as u32) + 128) >> 8).min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extremes_map_exactly() {
        assert_eq!(u16_to_dmx8(0), 0);
        assert_eq!(u16_to_dmx8(u16::MAX), 255);
    }

    #[test]
    fn matches_the_documented_approximate_percentages() {
        // Item 22's exact worked examples.
        assert_eq!(intensity_to_dmx8(Intensity::from_percent(0).unwrap()), 0);
        assert_eq!(intensity_to_dmx8(Intensity::from_percent(25).unwrap()), 64);
        assert_eq!(intensity_to_dmx8(Intensity::from_percent(50).unwrap()), 128);
        assert_eq!(intensity_to_dmx8(Intensity::from_percent(75).unwrap()), 192);
        assert_eq!(
            intensity_to_dmx8(Intensity::from_percent(100).unwrap()),
            255
        );
    }

    #[test]
    fn is_deterministic() {
        for raw in [0u16, 1, 12345, 32767, 32768, 65534, 65535] {
            assert_eq!(u16_to_dmx8(raw), u16_to_dmx8(raw));
        }
    }
}
