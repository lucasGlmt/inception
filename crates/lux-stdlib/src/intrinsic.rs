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
    EffectsSine,
    EffectsTriangle,
    EffectsSaw,
    EffectsSquare,
    SignalRangeFloat,
    SignalRangeIntensity,
    SignalRangeAngle,
    SignalPhase,
    /// `Signal<T>.spread(Angle) -> Signal<T>`, monomorphized per element
    /// type — unlike `.range()`/`.phase()`/`.invert()` (Float-only,
    /// oscillator-lineage-only), `.spread()` is also defined directly on
    /// an `Effects.step(...)` signal, for any of the 5 `T`s a `Sequence<T>`
    /// may hold (see `crate::methods`'s `Sequence`-adjacent spread table
    /// and item 9 of the `Effects.step` task brief).
    SignalSpreadFloat,
    SignalSpreadInt,
    SignalSpreadAngle,
    SignalSpreadIntensity,
    SignalSpreadColor,
    SignalInvert,
    /// Builds a `Sequence<T>` from `arg_count` popped `T` values (checked
    /// homogeneous by `lux-typeck` before this is ever resolved to — see
    /// `crate::registry`'s `Sequence` module docs). Monomorphized per
    /// element type, matching `SignalConstant*`'s style, but — unlike
    /// every other intrinsic in this enum — variadic: `arg_count` is not
    /// fixed per `Signature`, so this is dispatched specially by
    /// `inception-vm`'s `Vm` rather than through `eval_intrinsic`, exactly
    /// like `SignalConstant*`/`Effects*` (see that crate's `intrinsic`
    /// module doc), and `lux_bytecode::IntrinsicId::param_types`
    /// deliberately has no fixed-arity entry for these — see that method's
    /// docs.
    SequenceOfInt,
    SequenceOfFloat,
    SequenceOfAngle,
    SequenceOfIntensity,
    SequenceOfColor,
    /// `Sequence<T>.length() -> Int`, monomorphized per element type like
    /// `SequenceOf*` — the receiver (a `Sequence<T>`) is the one popped
    /// operand, see `lux_bytecode::IntrinsicId::param_types`.
    SequenceLengthInt,
    SequenceLengthFloat,
    SequenceLengthAngle,
    SequenceLengthIntensity,
    SequenceLengthColor,
    /// `Effects.step(sequence: Sequence<T>, every: Duration) -> Signal<T>`,
    /// monomorphized per element type like `SequenceOf*`/`SequenceLength*`.
    /// Fixed arity (exactly 2 operands), so — unlike `SequenceOf*` — this
    /// needs no special variadic handling anywhere downstream.
    EffectsStepInt,
    EffectsStepFloat,
    EffectsStepAngle,
    EffectsStepIntensity,
    EffectsStepColor,
}
