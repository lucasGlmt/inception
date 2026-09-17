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
    /// A time span — first used as a parameter type by `std.Effects`'s
    /// oscillators (`sine(period: Duration) -> Signal<Float>`, ...).
    /// `Signal.constant` never needed this: it's the first stdlib
    /// function to take a `Duration` argument, not the first to be able
    /// to represent one — `lux_typeck::Type::Duration` already existed
    /// (for `wait`), it just had no stdlib-facing counterpart before now.
    Duration,
    /// `Signal<T>` for each of the 5 `T`s above — only ever used as a
    /// `Signature::return_ty` in V1 (`Signal.constant`'s return type),
    /// never as a `Param::ty`: no stdlib function accepts a signal
    /// argument yet. Kept as 5 flat variants rather than one
    /// `Signal(Box<ParamType>)` payload, matching this registry's existing
    /// "monomorphize overloads as separate variants" style (see
    /// `MathAbsInt`/`MathAbsFloat` in `crate::intrinsic::IntrinsicId`)
    /// rather than introducing real generics here.
    SignalInt,
    SignalFloat,
    SignalAngle,
    SignalIntensity,
    SignalColor,
    /// `Sequence<T>` for each of the 5 `T`s above — used as
    /// `Signature::return_ty` for `Sequence.of`'s 5 monomorphized
    /// overloads, mirroring `SignalInt`/etc.'s docs above exactly (same
    /// "flat variants over `Sequence(Box<ParamType>)`" reasoning).
    SequenceInt,
    SequenceFloat,
    SequenceAngle,
    SequenceIntensity,
    SequenceColor,
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
