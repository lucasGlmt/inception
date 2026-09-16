//! The lighting attributes `SetAttribute` can target.
//!
//! Mirrors `lux_typeck::Attribute` in shape, defined independently for
//! the same reason `ValueType` mirrors `lux_typeck::Type`: this crate
//! must not depend on the compiler frontend.

use crate::value::ValueType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    Intensity,
    Color,
}

impl Attribute {
    /// The value type a `SetAttribute` for this attribute must pop.
    pub fn value_type(self) -> ValueType {
        match self {
            Attribute::Intensity => ValueType::Intensity,
            Attribute::Color => ValueType::Color,
        }
    }
}
