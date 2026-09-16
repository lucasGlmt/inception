//! The small, closed set of value types a stdlib function signature can
//! mention. This is deliberately narrower than (and independent of)
//! `lux_typeck::Type` — `lux-stdlib` has zero dependencies, so it cannot
//! name that type directly. `lux-typeck` owns the one conversion between
//! the two (see its `checker`/`infer` modules), the same boundary-crossing
//! idiom this workspace already uses for `lux_typeck::Type` vs.
//! `lux_bytecode::ValueType`.

/// A parameter or return type for a stdlib function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamType {
    Int,
    Float,
    Angle,
    Intensity,
    Color,
    /// Not a real stdlib parameter type — no `Signature` in this registry
    /// ever declares a parameter or return type of `Unsupported`. It
    /// exists purely as a total target for callers (namely
    /// `lux-typeck`'s `Type` -> `ParamType` boundary conversion) that have
    /// a caller-side type with no stdlib representation (`Bool`,
    /// `Duration`, `Frequency`, `Tempo`): mapping such an argument here
    /// guarantees it can never accidentally match a real parameter,
    /// so [`crate::resolve_overload`] reports a clean type mismatch
    /// instead of the caller needing its own `Option`-shaped escape hatch.
    Unsupported,
}
