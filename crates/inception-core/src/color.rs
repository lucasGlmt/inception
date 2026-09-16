//! The semantic representation of color — never a DMX byte.

/// High-precision RGB: `u16` per channel rather than the `u8` a DMX
/// channel ultimately needs, per this milestone's preference for
/// headroom in the semantic model even though today's Lux frontend only
/// ever produces 8-bit-sourced color values (hex/named literals) — see
/// `inception_vm`'s `Value -> AttributeValue` conversion for where that
/// gets widened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb {
        red: 0,
        green: 0,
        blue: 0,
    };
}
