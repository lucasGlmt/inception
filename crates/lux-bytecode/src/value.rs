//! Runtime value types and constants.
//!
//! `ValueType` mirrors `lux_typeck::Type` in shape, but is defined
//! independently: `lux-bytecode` must not depend on the compiler
//! frontend (see `AGENTS.md`), so it can be reused by `inception-vm`
//! without pulling in `lux-syntax`/`lux-hir`/`lux-typeck`. The mapping
//! from a frontend `Type` to a `ValueType` lives in `lux-mir`'s codegen,
//! which is allowed to depend on both.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    Int,
    Float,
    Duration,
    Intensity,
    Color,
    Angle,
    Frequency,
    Tempo,
    /// `Signal<T>`, mirroring `lux_typeck::Type::Signal`. Payload is a
    /// separate, non-recursive enum (not `Box<ValueType>`) so `ValueType`
    /// itself stays `Copy` — see `ScalarValueType`'s docs.
    Signal(ScalarValueType),
    /// `Sequence<T>`, mirroring `lux_typeck::Type::Sequence`. Reuses
    /// `ScalarValueType` for its element (the same 5-type universe as
    /// `Signal`) rather than defining a second, identical enum.
    Sequence(ScalarValueType),
}

/// The element types a `Signal<T>` may carry — mirrors
/// `lux_typeck::SignalElement` independently, per this crate's
/// frontend/runtime duplication idiom (see this module's doc comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarValueType {
    Int,
    Float,
    Angle,
    Intensity,
    Color,
}

impl ScalarValueType {
    /// The plain `ValueType` this scalar corresponds to — used when
    /// unwrapping a `Signal<T>`/`Sequence<T>`'s element type back to a
    /// standalone `ValueType`, e.g. `Instruction::Index`'s result type in
    /// `verify.rs`.
    pub fn as_value_type(self) -> ValueType {
        match self {
            ScalarValueType::Int => ValueType::Int,
            ScalarValueType::Float => ValueType::Float,
            ScalarValueType::Angle => ValueType::Angle,
            ScalarValueType::Intensity => ValueType::Intensity,
            ScalarValueType::Color => ValueType::Color,
        }
    }
}

/// An RGB color value. Bytecode never sees a named color (`red`, `blue`,
/// ...) — that's a source-level convenience resolved to a concrete RGB
/// triplet during MIR lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorValue {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A constant pool entry. Instructions reference these by [`crate::ConstantId`]
/// rather than embedding values inline, so the same instruction encoding
/// works regardless of a value's size.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Nanoseconds.
    Duration(u64),
    /// `0..=65535`, linearly mapped from the source's `0..=100%`.
    Intensity(u16),
    Color(ColorValue),
    /// Millidegrees.
    Angle(i32),
    /// Hertz.
    Frequency(u32),
    /// Beats per minute.
    Tempo(u32),
}

impl Constant {
    pub fn value_type(&self) -> ValueType {
        match self {
            Constant::Bool(_) => ValueType::Bool,
            Constant::Int(_) => ValueType::Int,
            Constant::Float(_) => ValueType::Float,
            Constant::Duration(_) => ValueType::Duration,
            Constant::Intensity(_) => ValueType::Intensity,
            Constant::Color(_) => ValueType::Color,
            Constant::Angle(_) => ValueType::Angle,
            Constant::Frequency(_) => ValueType::Frequency,
            Constant::Tempo(_) => ValueType::Tempo,
        }
    }
}
