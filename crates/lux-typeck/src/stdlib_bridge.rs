//! The one boundary conversion between `lux-typeck`'s [`Type`] and
//! `lux-stdlib`'s [`lux_stdlib::ParamType`] — the same idiom this
//! workspace already uses to cross from a compiler-frontend type to a
//! narrower runtime-facing one (e.g. `Type` -> `lux_bytecode::ValueType`
//! in `lux-mir`).
//!
//! `lux-stdlib` has zero dependencies, so it cannot name `lux_typeck::Type`
//! itself — this conversion has to live on this (the higher) side.

use lux_stdlib::ParamType;

use crate::types::{SignalElement, Type};

/// Total: every `Type` maps to *some* `ParamType`. `Bool`/`Frequency`/
/// `Tempo` have no stdlib representation, so they map to
/// [`ParamType::Unsupported`] — a sentinel no real `Signature` parameter
/// ever uses, guaranteeing such an argument can never accidentally match
/// a real overload; it only ever produces a clean type-mismatch
/// diagnostic (see `checker.rs::check_call`). `Type::Signal(_)` maps the
/// same way: no stdlib function accepts a `Signal` argument in V1 (only
/// `Signal.constant`'s *return* type is a signal), so passing one as an
/// argument anywhere should behave exactly like passing a `Bool`.
/// `Type::Duration` *is* representable (`ParamType::Duration`), first used
/// by `std.Effects`'s oscillator `period` parameters.
pub fn to_param_type(ty: Type) -> ParamType {
    match ty {
        Type::Int => ParamType::Int,
        Type::Float => ParamType::Float,
        Type::Angle => ParamType::Angle,
        Type::Intensity => ParamType::Intensity,
        Type::Color => ParamType::Color,
        Type::Duration => ParamType::Duration,
        Type::Bool | Type::Frequency | Type::Tempo | Type::Signal(_) => ParamType::Unsupported,
    }
}

/// The inverse, used for a `Signature`'s `return_ty`. No real signature
/// ever returns `Unsupported`, so that case is an internal-invariant
/// violation, not a user-facing error.
pub fn from_param_type(ty: ParamType) -> Type {
    match ty {
        ParamType::Int => Type::Int,
        ParamType::Float => Type::Float,
        ParamType::Angle => Type::Angle,
        ParamType::Intensity => Type::Intensity,
        ParamType::Color => Type::Color,
        ParamType::Duration => Type::Duration,
        ParamType::SignalInt => Type::Signal(SignalElement::Int),
        ParamType::SignalFloat => Type::Signal(SignalElement::Float),
        ParamType::SignalAngle => Type::Signal(SignalElement::Angle),
        ParamType::SignalIntensity => Type::Signal(SignalElement::Intensity),
        ParamType::SignalColor => Type::Signal(SignalElement::Color),
        ParamType::Unsupported => unreachable!(
            "from_param_type: no stdlib signature returns `Unsupported` — \
             this would mean a `Signature::return_ty` was misconfigured"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_representable_types() {
        for ty in [
            Type::Int,
            Type::Float,
            Type::Angle,
            Type::Intensity,
            Type::Color,
            Type::Duration,
        ] {
            assert_eq!(from_param_type(to_param_type(ty)), ty);
        }
    }

    #[test]
    fn signal_return_types_round_trip() {
        for (param, elem) in [
            (ParamType::SignalInt, SignalElement::Int),
            (ParamType::SignalFloat, SignalElement::Float),
            (ParamType::SignalAngle, SignalElement::Angle),
            (ParamType::SignalIntensity, SignalElement::Intensity),
            (ParamType::SignalColor, SignalElement::Color),
        ] {
            assert_eq!(from_param_type(param), Type::Signal(elem));
        }
    }

    #[test]
    fn signal_type_as_an_argument_is_unsupported() {
        // No stdlib function accepts a `Signal` parameter in V1.
        assert_eq!(
            to_param_type(Type::Signal(SignalElement::Intensity)),
            ParamType::Unsupported
        );
    }

    #[test]
    fn unrepresentable_types_map_to_unsupported() {
        for ty in [Type::Bool, Type::Frequency, Type::Tempo] {
            assert_eq!(to_param_type(ty), ParamType::Unsupported);
        }
    }
}
