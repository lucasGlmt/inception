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
}

/// A typed attribute value. Pairing the attribute with its value in one
/// enum (rather than two separate `attribute: Attribute` and
/// `value: SomeUnion` fields) is what makes `Intensity ← Color` — item
/// 7's explicit worry — impossible to construct in the first place: there
/// is no `AttributeValue` variant that lets a `Color` masquerade as an
/// `Intensity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttributeValue {
    Intensity(Intensity),
    Color(Rgb),
}

impl AttributeValue {
    pub fn attribute(&self) -> Attribute {
        match self {
            AttributeValue::Intensity(_) => Attribute::Intensity,
            AttributeValue::Color(_) => Attribute::Color,
        }
    }
}
