//! Lighting attributes and their values.
//!
//! Deliberately just two attributes for this milestone (see item 6 of
//! the task brief: "ne pas généraliser prématurément à 50 attributs") —
//! adding a third later is a new `Attribute`/`AttributeValue` variant,
//! not a redesign.

use crate::color::Rgb;
use crate::intensity::Intensity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attribute {
    Intensity,
    Color,
    Strobe,
}

/// A typed attribute value. Pairing the attribute with its value in one
/// enum (rather than two separate `attribute: Attribute` and
/// `value: SomeUnion` fields) is what makes `Intensity ← Color` — item
/// 7's explicit worry — impossible to construct in the first place: there
/// is no `AttributeValue` variant that lets a `Color` masquerade as an
/// `Intensity`.
///
/// `Strobe` carries an `Intensity` too — same `0%..=100%` domain, read as
/// a strobe speed/duty knob rather than a light level. A distinct variant
/// (not a reuse of `AttributeValue::Intensity`) keeps it a separate
/// `(fixture, attribute)` slot in `LightingState`, so setting a fixture's
/// strobe never overwrites its dimmer level or vice versa.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttributeValue {
    Intensity(Intensity),
    Color(Rgb),
    Strobe(Intensity),
}

impl AttributeValue {
    pub fn attribute(&self) -> Attribute {
        match self {
            AttributeValue::Intensity(_) => Attribute::Intensity,
            AttributeValue::Color(_) => Attribute::Color,
            AttributeValue::Strobe(_) => Attribute::Strobe,
        }
    }
}
