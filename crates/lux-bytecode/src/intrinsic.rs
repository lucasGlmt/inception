//! Resolved stdlib intrinsic identities, as seen by bytecode/the VM.
//!
//! Mirrors `lux_stdlib::IntrinsicId` in shape, but is defined
//! independently: `lux-bytecode` must not depend on the compiler frontend
//! (see `AGENTS.md`), so it can be reused by `inception-vm` without
//! pulling in `lux-syntax`/`lux-hir`/`lux-typeck`/`lux-stdlib`. The
//! mapping from the frontend's `lux_stdlib::IntrinsicId` to this one lives
//! in `lux-mir`'s codegen, which is allowed to depend on both — the same
//! split already used for `lux_typeck::Type`/`ValueType` and
//! `lux_typeck::Attribute`/`Attribute`.
//!
//! A `CallIntrinsic` is dispatched purely by this numeric id, never by
//! name: name-to-intrinsic resolution is finished by the time this
//! instruction exists, back in `lux-hir`/`lux-typeck`.

use crate::value::{ScalarValueType, ValueType};

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
    /// Builds a constant `Signal<T>` from one popped `T` value. Monomorphized
    /// per element type (matching `MathAbsInt`/`MathAbsFloat`'s style)
    /// rather than carrying a payload, so `IntrinsicId` stays a flat `Copy`
    /// enum. Dispatched specially by `inception-vm`'s `Vm` rather than
    /// through `eval_intrinsic` — see that crate's `intrinsic` module doc.
    SignalConstantInt,
    SignalConstantFloat,
    SignalConstantAngle,
    SignalConstantIntensity,
    SignalConstantColor,
    /// Constructs a `Signal<Float>` oscillator. All four monomorphized the
    /// same way `SignalConstant*` is per element type — here the axis is
    /// waveform, not element type, since every oscillator returns
    /// `Signal<Float>`. Like `SignalConstant*`, dispatched specially by
    /// `inception-vm`'s `Vm` rather than through `eval_intrinsic`: besides
    /// inserting into `SignalStore`, construction also reads `clock.now()`
    /// for the oscillator's origin — see `inception-vm`'s `signal` module
    /// doc.
    EffectsSine,
    EffectsTriangle,
    EffectsSaw,
    EffectsSquare,
    /// `Signal<Float>.range(T, T) -> Signal<T>`, monomorphized per target
    /// `T` like `SignalConstant*`/`Effects*`. The receiver (a
    /// `Signal<Float>`) is operand 0 — see `param_types` — never a
    /// separate concept from the other two popped operands.
    SignalRangeFloat,
    SignalRangeIntensity,
    SignalRangeAngle,
    /// `Signal<Float>.phase(Angle) -> Signal<Float>`.
    SignalPhase,
    /// `Signal<Float>.spread(Angle) -> Signal<Float>`: distributes its
    /// `Angle` operand as a per-fixture phase offset across whatever
    /// group the signal ends up bound to (`amount * fixture_index /
    /// fixture_count`, resolved at sample time by
    /// `inception_vm::signal::SignalSampleContext` — see that type's
    /// docs). Shares `SignalPhase`'s operand shape exactly (receiver,
    /// then one `Angle`), just a different `IntrinsicId`.
    SignalSpread,
    /// `Signal<Float>.invert() -> Signal<Float>`.
    SignalInvert,
    /// Builds a `Sequence<T>` from `arg_count` popped `T` values (`T`
    /// fixed by which of the 5 monomorphized variants this is, matching
    /// `SignalConstant*`'s per-element-type style). Unlike every other
    /// intrinsic in this enum, **variadic**: `arg_count` varies per call
    /// site, so there is no fixed `param_types()` entry for these — see
    /// that method's docs — and the verifier checks them through
    /// [`IntrinsicId::variadic_element_type`] instead. Dispatched
    /// specially by `inception-vm`'s `Vm`, not through `eval_intrinsic`,
    /// for the same reason `SignalConstant*` is (it must insert into
    /// mutable `Vm`-owned state — see that crate's `intrinsic` module
    /// doc).
    SequenceOfInt,
    SequenceOfFloat,
    SequenceOfAngle,
    SequenceOfIntensity,
    SequenceOfColor,
    /// `Sequence<T>.length() -> Int`, monomorphized per element type like
    /// `SequenceOf*`. Fixed arity (exactly one operand, the receiver), so
    /// — unlike `SequenceOf*` — this fits `param_types()`/`return_type()`
    /// normally with no special-casing.
    SequenceLengthInt,
    SequenceLengthFloat,
    SequenceLengthAngle,
    SequenceLengthIntensity,
    SequenceLengthColor,
}

impl IntrinsicId {
    /// The `ValueType` every popped operand of a `SequenceOf*` intrinsic
    /// must match, or `None` for every other (fixed-arity) intrinsic.
    /// `SequenceOf*` is the one variadic case in this enum — `arg_count`
    /// varies per call site, so there is no fixed-length `param_types()`
    /// entry for it (calling `param_types` on one of these panics — see
    /// that method's docs); `verify.rs`'s `CallIntrinsic` handling checks
    /// this first and, when it returns `Some`, pops `arg_count` operands
    /// (whatever that instruction's own field says) against it directly
    /// instead of consulting `param_types()`/its declared length.
    pub fn variadic_element_type(self) -> Option<ValueType> {
        match self {
            IntrinsicId::SequenceOfInt => Some(ValueType::Int),
            IntrinsicId::SequenceOfFloat => Some(ValueType::Float),
            IntrinsicId::SequenceOfAngle => Some(ValueType::Angle),
            IntrinsicId::SequenceOfIntensity => Some(ValueType::Intensity),
            IntrinsicId::SequenceOfColor => Some(ValueType::Color),
            _ => None,
        }
    }

    /// The exact operand types the verifier requires, in argument order
    /// (last argument on top of the stack).
    ///
    /// # Panics
    ///
    /// On a `SequenceOf*` variant (see [`Self::variadic_element_type`]):
    /// those are the one variadic case in this enum, so no fixed-length
    /// slice can describe their operands — `verify.rs` never calls this
    /// for them, checking `variadic_element_type` first instead.
    pub fn param_types(self) -> &'static [ValueType] {
        use ValueType::*;
        match self {
            IntrinsicId::MathSin | IntrinsicId::MathCos => &[Angle],
            IntrinsicId::MathAbsInt => &[Int],
            IntrinsicId::MathAbsFloat => &[Float],
            IntrinsicId::MathMinInt | IntrinsicId::MathMaxInt => &[Int, Int],
            IntrinsicId::MathMinFloat | IntrinsicId::MathMaxFloat => &[Float, Float],
            IntrinsicId::MathClampInt => &[Int, Int, Int],
            IntrinsicId::MathClampFloat => &[Float, Float, Float],
            IntrinsicId::MathLerp => &[Float, Float, Float],
            IntrinsicId::ColorRgb => &[Int, Int, Int],
            IntrinsicId::ColorMix => &[Color, Color, Float],
            IntrinsicId::ColorHsv => &[Angle, Intensity, Intensity],
            IntrinsicId::SignalConstantInt => &[Int],
            IntrinsicId::SignalConstantFloat => &[Float],
            IntrinsicId::SignalConstantAngle => &[Angle],
            IntrinsicId::SignalConstantIntensity => &[Intensity],
            IntrinsicId::SignalConstantColor => &[Color],
            IntrinsicId::EffectsSine
            | IntrinsicId::EffectsTriangle
            | IntrinsicId::EffectsSaw
            | IntrinsicId::EffectsSquare => &[Duration],
            IntrinsicId::SignalRangeFloat => &[Signal(ScalarValueType::Float), Float, Float],
            IntrinsicId::SignalRangeIntensity => {
                &[Signal(ScalarValueType::Float), Intensity, Intensity]
            }
            IntrinsicId::SignalRangeAngle => &[Signal(ScalarValueType::Float), Angle, Angle],
            IntrinsicId::SignalPhase => &[Signal(ScalarValueType::Float), Angle],
            IntrinsicId::SignalSpread => &[Signal(ScalarValueType::Float), Angle],
            IntrinsicId::SignalInvert => &[Signal(ScalarValueType::Float)],
            IntrinsicId::SequenceOfInt
            | IntrinsicId::SequenceOfFloat
            | IntrinsicId::SequenceOfAngle
            | IntrinsicId::SequenceOfIntensity
            | IntrinsicId::SequenceOfColor => panic!(
                "IntrinsicId::param_types: {self:?} is variadic — see \
                 `variadic_element_type`, which `verify.rs` checks first"
            ),
            IntrinsicId::SequenceLengthInt => &[Sequence(ScalarValueType::Int)],
            IntrinsicId::SequenceLengthFloat => &[Sequence(ScalarValueType::Float)],
            IntrinsicId::SequenceLengthAngle => &[Sequence(ScalarValueType::Angle)],
            IntrinsicId::SequenceLengthIntensity => &[Sequence(ScalarValueType::Intensity)],
            IntrinsicId::SequenceLengthColor => &[Sequence(ScalarValueType::Color)],
        }
    }

    pub fn return_type(self) -> ValueType {
        match self {
            IntrinsicId::MathSin
            | IntrinsicId::MathCos
            | IntrinsicId::MathAbsFloat
            | IntrinsicId::MathMinFloat
            | IntrinsicId::MathMaxFloat
            | IntrinsicId::MathClampFloat
            | IntrinsicId::MathLerp => ValueType::Float,
            IntrinsicId::MathAbsInt | IntrinsicId::MathMinInt | IntrinsicId::MathMaxInt => {
                ValueType::Int
            }
            IntrinsicId::MathClampInt => ValueType::Int,
            IntrinsicId::ColorRgb | IntrinsicId::ColorMix | IntrinsicId::ColorHsv => {
                ValueType::Color
            }
            IntrinsicId::SignalConstantInt => ValueType::Signal(ScalarValueType::Int),
            IntrinsicId::SignalConstantFloat => ValueType::Signal(ScalarValueType::Float),
            IntrinsicId::SignalConstantAngle => ValueType::Signal(ScalarValueType::Angle),
            IntrinsicId::SignalConstantIntensity => ValueType::Signal(ScalarValueType::Intensity),
            IntrinsicId::SignalConstantColor => ValueType::Signal(ScalarValueType::Color),
            IntrinsicId::EffectsSine
            | IntrinsicId::EffectsTriangle
            | IntrinsicId::EffectsSaw
            | IntrinsicId::EffectsSquare
            | IntrinsicId::SignalPhase
            | IntrinsicId::SignalSpread
            | IntrinsicId::SignalInvert
            | IntrinsicId::SignalRangeFloat => ValueType::Signal(ScalarValueType::Float),
            IntrinsicId::SignalRangeIntensity => ValueType::Signal(ScalarValueType::Intensity),
            IntrinsicId::SignalRangeAngle => ValueType::Signal(ScalarValueType::Angle),
            IntrinsicId::SequenceOfInt => ValueType::Sequence(ScalarValueType::Int),
            IntrinsicId::SequenceOfFloat => ValueType::Sequence(ScalarValueType::Float),
            IntrinsicId::SequenceOfAngle => ValueType::Sequence(ScalarValueType::Angle),
            IntrinsicId::SequenceOfIntensity => ValueType::Sequence(ScalarValueType::Intensity),
            IntrinsicId::SequenceOfColor => ValueType::Sequence(ScalarValueType::Color),
            IntrinsicId::SequenceLengthInt
            | IntrinsicId::SequenceLengthFloat
            | IntrinsicId::SequenceLengthAngle
            | IntrinsicId::SequenceLengthIntensity
            | IntrinsicId::SequenceLengthColor => ValueType::Int,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_intrinsic_has_a_non_empty_arity() {
        for intrinsic in [
            IntrinsicId::MathSin,
            IntrinsicId::MathCos,
            IntrinsicId::MathAbsInt,
            IntrinsicId::MathAbsFloat,
            IntrinsicId::MathMinInt,
            IntrinsicId::MathMinFloat,
            IntrinsicId::MathMaxInt,
            IntrinsicId::MathMaxFloat,
            IntrinsicId::MathClampInt,
            IntrinsicId::MathClampFloat,
            IntrinsicId::MathLerp,
            IntrinsicId::ColorRgb,
            IntrinsicId::ColorMix,
            IntrinsicId::ColorHsv,
            IntrinsicId::SignalConstantInt,
            IntrinsicId::SignalConstantFloat,
            IntrinsicId::SignalConstantAngle,
            IntrinsicId::SignalConstantIntensity,
            IntrinsicId::SignalConstantColor,
            IntrinsicId::EffectsSine,
            IntrinsicId::EffectsTriangle,
            IntrinsicId::EffectsSaw,
            IntrinsicId::EffectsSquare,
        ] {
            assert!(!intrinsic.param_types().is_empty());
        }
    }

    #[test]
    fn effects_intrinsics_take_a_duration_and_return_signal_float() {
        for intrinsic in [
            IntrinsicId::EffectsSine,
            IntrinsicId::EffectsTriangle,
            IntrinsicId::EffectsSaw,
            IntrinsicId::EffectsSquare,
        ] {
            assert_eq!(intrinsic.param_types(), &[ValueType::Duration]);
            assert_eq!(
                intrinsic.return_type(),
                ValueType::Signal(crate::value::ScalarValueType::Float)
            );
        }
    }

    #[test]
    fn signal_constant_intrinsics_return_the_matching_signal_type() {
        use crate::value::ScalarValueType;

        let cases = [
            (
                IntrinsicId::SignalConstantInt,
                ValueType::Int,
                ScalarValueType::Int,
            ),
            (
                IntrinsicId::SignalConstantFloat,
                ValueType::Float,
                ScalarValueType::Float,
            ),
            (
                IntrinsicId::SignalConstantAngle,
                ValueType::Angle,
                ScalarValueType::Angle,
            ),
            (
                IntrinsicId::SignalConstantIntensity,
                ValueType::Intensity,
                ScalarValueType::Intensity,
            ),
            (
                IntrinsicId::SignalConstantColor,
                ValueType::Color,
                ScalarValueType::Color,
            ),
        ];
        for (intrinsic, param, elem) in cases {
            assert_eq!(intrinsic.param_types(), &[param]);
            assert_eq!(intrinsic.return_type(), ValueType::Signal(elem));
        }
    }

    #[test]
    fn sequence_of_intrinsics_are_variadic() {
        let cases = [
            (IntrinsicId::SequenceOfInt, ValueType::Int),
            (IntrinsicId::SequenceOfFloat, ValueType::Float),
            (IntrinsicId::SequenceOfAngle, ValueType::Angle),
            (IntrinsicId::SequenceOfIntensity, ValueType::Intensity),
            (IntrinsicId::SequenceOfColor, ValueType::Color),
        ];
        for (intrinsic, elem) in cases {
            assert_eq!(intrinsic.variadic_element_type(), Some(elem));
            assert_eq!(
                intrinsic.return_type(),
                ValueType::Sequence(match elem {
                    ValueType::Int => ScalarValueType::Int,
                    ValueType::Float => ScalarValueType::Float,
                    ValueType::Angle => ScalarValueType::Angle,
                    ValueType::Intensity => ScalarValueType::Intensity,
                    ValueType::Color => ScalarValueType::Color,
                    other => panic!("unexpected element type {other:?}"),
                })
            );
        }
    }

    #[test]
    #[should_panic(expected = "is variadic")]
    fn sequence_of_param_types_panics() {
        let _ = IntrinsicId::SequenceOfColor.param_types();
    }

    #[test]
    fn sequence_length_intrinsics_are_fixed_arity_and_return_int() {
        let cases = [
            (
                IntrinsicId::SequenceLengthInt,
                ValueType::Sequence(ScalarValueType::Int),
            ),
            (
                IntrinsicId::SequenceLengthFloat,
                ValueType::Sequence(ScalarValueType::Float),
            ),
            (
                IntrinsicId::SequenceLengthAngle,
                ValueType::Sequence(ScalarValueType::Angle),
            ),
            (
                IntrinsicId::SequenceLengthIntensity,
                ValueType::Sequence(ScalarValueType::Intensity),
            ),
            (
                IntrinsicId::SequenceLengthColor,
                ValueType::Sequence(ScalarValueType::Color),
            ),
        ];
        for (intrinsic, receiver) in cases {
            assert_eq!(intrinsic.param_types(), &[receiver]);
            assert_eq!(intrinsic.return_type(), ValueType::Int);
            assert_eq!(intrinsic.variadic_element_type(), None);
        }
    }
}
