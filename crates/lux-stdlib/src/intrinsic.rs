//! The compiler-side intrinsic identity. Every stdlib function that is
//! implemented as an intrinsic (all of them, in V1 — see `Signature::pure`
//! and the module doc) carries one of these instead of a name, so nothing
//! downstream of `lux-hir` ever dispatches on a string.
//!
//! `lux-bytecode` and `inception-vm` define their **own**, independent
//! copy of this enum rather than depending on `lux-stdlib` (which is a
//! compiler-frontend-shaped crate) — `lux-mir::codegen` performs the one
//! exhaustive conversion between the two, the same idiom already used for
//! `lux_typeck::Type` -> `lux_bytecode::ValueType`. See
//! `docs/rfcs/0001-modules-and-stdlib.md` for the full rationale.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicId {
    MathSin,
    MathCos,
    MathAbsInt,
    MathAbsFloat,
    MathMinInt,
    MathMinFloat,
    MathMaxInt,
    MathMaxFloat,
    MathClampInt,
    MathClampFloat,
    MathLerp,
    ColorRgb,
    ColorMix,
    ColorHsv,
    SignalConstantInt,
    SignalConstantFloat,
    SignalConstantAngle,
    SignalConstantIntensity,
    SignalConstantColor,
}
