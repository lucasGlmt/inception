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

use inception_core::Duration;
use lux_bytecode::{ColorValue, Constant, ValueType};

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
}
