//! Runtime values: what actually sits on the operand stack and in
//! locals, as opposed to `lux_bytecode::Constant` (a constant pool
//! entry) or `lux_bytecode::ValueType` (just a type tag).
//!
//! `Value::Duration` reuses `inception_core::Duration` rather than a bare
//! `u64`: it's exactly the domain type `inception-core` already defines
//! for this, and reusing it is what lets `WAIT`'s handler feed a popped
//! value straight into `Clock`/`Timestamp` arithmetic. `Value::Color`
//! reuses `lux_bytecode::ColorValue` directly for the same reason —
//! `inception-vm` already depends on `lux-bytecode`, so redefining an
//! identical struct here would be pure duplication, unlike the
//! `lux-bytecode`/`lux-typeck` boundary (which is architectural and must
//! stay duplicated).

use inception_core::{AttributeValue, Duration, Intensity, Rgb};
use lux_bytecode::{Attribute, ColorValue, Constant, ScalarValueType, ValueType};

use crate::signal::SignalId;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Duration(Duration),
    /// `0..=65535`, linearly mapped from the source's `0..=100%` — same
    /// representation as `lux_bytecode::Constant::Intensity`.
    Intensity(u16),
    Color(ColorValue),
    /// Millidegrees.
    Angle(i32),
    /// Hertz.
    Frequency(u32),
    /// Beats per minute.
    Tempo(u32),
    /// A handle into the executing `Vm`'s `crate::signal::SignalStore`,
    /// tagged with its element type — never the signal's definition
    /// inline (see that module's docs for why). The element type is
    /// carried alongside the id, not looked up through it: `value_type`
    /// below (used defensively — e.g. `exec_store_local`'s belt-and-braces
    /// check on already-verified bytecode) must stay a total, panic-free
    /// method on `Value` alone, with no access to the `SignalStore` that
    /// would otherwise be needed to answer "what kind of signal is this".
    /// Both fields are `Copy`, so this keeps `Value` itself `Copy`.
    Signal(ScalarValueType, SignalId),
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Bool(_) => ValueType::Bool,
            Value::Int(_) => ValueType::Int,
            Value::Float(_) => ValueType::Float,
            Value::Duration(_) => ValueType::Duration,
            Value::Intensity(_) => ValueType::Intensity,
            Value::Color(_) => ValueType::Color,
            Value::Angle(_) => ValueType::Angle,
            Value::Frequency(_) => ValueType::Frequency,
            Value::Tempo(_) => ValueType::Tempo,
            Value::Signal(elem, _) => ValueType::Signal(*elem),
        }
    }

    /// The explicit `Constant -> Value` mapping requested by the task
    /// brief: every constant pool entry becomes exactly one runtime
    /// value, with no conversions beyond wrapping.
    pub fn from_constant(constant: &Constant) -> Value {
        match *constant {
            Constant::Bool(b) => Value::Bool(b),
            Constant::Int(i) => Value::Int(i),
            Constant::Float(f) => Value::Float(f),
            Constant::Duration(ns) => Value::Duration(Duration::from_nanos(ns)),
            Constant::Intensity(v) => Value::Intensity(v),
            Constant::Color(c) => Value::Color(c),
            Constant::Angle(a) => Value::Angle(a),
            Constant::Frequency(f) => Value::Frequency(f),
            Constant::Tempo(t) => Value::Tempo(t),
        }
    }

    /// Converts this value into a `LightingState`-ready
    /// [`AttributeValue`] for a `SET_ATTRIBUTE` declaring `attribute` —
    /// `None` if this value's variant doesn't match what `attribute`
    /// expects. `lux_bytecode::verify` already guarantees this can't
    /// happen for a verified module (`SetAttribute`'s operand type is
    /// checked there), but `Vm` stays defensive regardless (see `error`
    /// module docs).
    ///
    /// `Color` is widened from the bytecode's 8-bit-per-channel
    /// `ColorValue` to `Rgb`'s `u16` channels by `channel * 257`: this is
    /// the standard exact 8-to-16-bit scale (`255 * 257 == 65535`), so
    /// `0` and `255` map to `0` and `65535` precisely, with every other
    /// value evenly spaced in between.
    pub fn into_attribute_value(self, attribute: Attribute) -> Option<AttributeValue> {
        match (attribute, self) {
            (Attribute::Intensity, Value::Intensity(raw)) => {
                Some(AttributeValue::Intensity(Intensity::new(raw)))
            }
            (Attribute::Strobe, Value::Intensity(raw)) => {
                Some(AttributeValue::Strobe(Intensity::new(raw)))
            }
            (Attribute::Color, Value::Color(ColorValue { r, g, b })) => {
                Some(AttributeValue::Color(Rgb {
                    red: r as u16 * 257,
                    green: g as u16 * 257,
                    blue: b as u16 * 257,
                }))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_map_to_matching_value_variants() {
        assert_eq!(
            Value::from_constant(&Constant::Bool(true)).value_type(),
            ValueType::Bool
        );
        assert_eq!(
            Value::from_constant(&Constant::Duration(1_000_000_000)),
            Value::Duration(Duration::from_secs(1))
        );
        assert_eq!(
            Value::from_constant(&Constant::Intensity(100)),
            Value::Intensity(100)
        );
    }

    #[test]
    fn matching_value_converts_to_attribute_value() {
        assert_eq!(
            Value::Intensity(32767).into_attribute_value(Attribute::Intensity),
            Some(AttributeValue::Intensity(Intensity::new(32767)))
        );
    }

    #[test]
    fn mismatched_value_does_not_convert() {
        assert_eq!(
            Value::Duration(Duration::from_secs(1)).into_attribute_value(Attribute::Intensity),
            None
        );
        assert_eq!(
            Value::Color(ColorValue { r: 0, g: 0, b: 0 })
                .into_attribute_value(Attribute::Intensity),
            None
        );
    }

    #[test]
    fn color_widens_exactly_from_8_to_16_bits() {
        let converted = Value::Color(ColorValue {
            r: 255,
            g: 128,
            b: 0,
        })
        .into_attribute_value(Attribute::Color);
        assert_eq!(
            converted,
            Some(AttributeValue::Color(Rgb {
                red: 65535,
                green: 32896,
                blue: 0
            }))
        );
    }
}
