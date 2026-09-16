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

use crate::value::ValueType;

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
}

impl IntrinsicId {
    /// The exact operand types the verifier requires, in argument order
    /// (last argument on top of the stack).
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
        ] {
            assert!(!intrinsic.param_types().is_empty());
        }
    }
}
