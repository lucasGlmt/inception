//! The lighting attributes `SetAttribute` can target.
//!
//! Mirrors `lux_typeck::Attribute` in shape, defined independently for
//! the same reason `ValueType` mirrors `lux_typeck::Type`: this crate
//! must not depend on the compiler frontend.

use crate::value::{ScalarValueType, ValueType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    Intensity,
    Color,
    Strobe,
}

impl Attribute {
    /// The value type a `SetAttribute` for this attribute must pop.
    pub fn value_type(self) -> ValueType {
        match self {
            Attribute::Intensity => ValueType::Intensity,
            Attribute::Color => ValueType::Color,
            Attribute::Strobe => ValueType::Intensity,
        }
    }

    /// The `Signal<T>` value type a `BindSignal` for this attribute must
    /// pop — the same `T` as [`Attribute::value_type`], wrapped in
    /// `Signal`.
    pub fn signal_value_type(self) -> ValueType {
        match self {
            Attribute::Intensity => ValueType::Signal(ScalarValueType::Intensity),
            Attribute::Color => ValueType::Signal(ScalarValueType::Color),
            Attribute::Strobe => ValueType::Signal(ScalarValueType::Intensity),
        }
    }
}
