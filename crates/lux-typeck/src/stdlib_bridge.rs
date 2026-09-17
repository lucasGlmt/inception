//! The one boundary conversion between `lux-typeck`'s [`Type`] and
//! `lux-stdlib`'s [`lux_stdlib::ParamType`] — the same idiom this
//! workspace already uses to cross from a compiler-frontend type to a
//! narrower runtime-facing one (e.g. `Type` -> `lux_bytecode::ValueType`
//! in `lux-mir`).
//!
//! `lux-stdlib` has zero dependencies, so it cannot name `lux_typeck::Type`
//! itself — this conversion has to live on this (the higher) side.

use lux_stdlib::ParamType;

use crate::types::{SequenceElement, SignalElement, Type};

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
/// by `std.Effects`'s oscillator `period` parameters. `Type::Sequence(_)`
/// *is* representable too, as of `Effects.step(sequence: Sequence<T>,
/// every: Duration)` — the first stdlib function to accept one as an
/// argument (reuses [`sequence_element_param_type`], the same tag already
/// used for a `Sequence<T>` *receiver*).
pub fn to_param_type(ty: Type) -> ParamType {
    match ty {
        Type::Int => ParamType::Int,
        Type::Float => ParamType::Float,
        Type::Angle => ParamType::Angle,
        Type::Intensity => ParamType::Intensity,
        Type::Color => ParamType::Color,
        Type::Duration => ParamType::Duration,
        Type::Sequence(elem) => sequence_element_param_type(elem),
        Type::Bool | Type::Frequency | Type::Tempo | Type::Signal(_) => ParamType::Unsupported,
    }
}

/// The `ParamType` tag for a `Sequence<T>` value — used both as
/// [`to_param_type`]'s `Type::Sequence(_)` case (an ordinary argument
/// conversion, e.g. `Effects.step`'s `sequence` parameter) and to select
/// which of `.length()`'s 5 monomorphized overloads a `Sequence<Color>`
/// *receiver* resolves to (see `lux_stdlib::methods::resolve_sequence_method`),
/// the same dual role `Type::Signal(SignalElement::Float)` plays for
/// `Signal<Float>`'s builtin methods.
pub fn sequence_element_param_type(elem: SequenceElement) -> ParamType {
    match elem {
        SequenceElement::Int => ParamType::SequenceInt,
        SequenceElement::Float => ParamType::SequenceFloat,
        SequenceElement::Angle => ParamType::SequenceAngle,
        SequenceElement::Intensity => ParamType::SequenceIntensity,
        SequenceElement::Color => ParamType::SequenceColor,
    }
}

/// The `ParamType` tag for a non-`Float` `Signal<T>` *receiver* — selects
/// which of `.spread()`'s 4 non-`Float` monomorphized overloads a
/// `Signal<T>` value resolves to (see
/// `lux_stdlib::methods::resolve_non_float_signal_method`). `Float` itself
/// is never passed here: `Signal<Float>` has its own full method table
/// (`lux_stdlib::resolve_signal_float_method`), selected directly by
/// `crate::checker::Checker::check_signal_method` without going through
/// this tag at all.
pub fn signal_element_param_type(elem: SignalElement) -> ParamType {
    match elem {
        SignalElement::Int => ParamType::SignalInt,
        SignalElement::Float => ParamType::SignalFloat,
        SignalElement::Angle => ParamType::SignalAngle,
        SignalElement::Intensity => ParamType::SignalIntensity,
        SignalElement::Color => ParamType::SignalColor,
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
        ParamType::SequenceInt => Type::Sequence(SequenceElement::Int),
        ParamType::SequenceFloat => Type::Sequence(SequenceElement::Float),
        ParamType::SequenceAngle => Type::Sequence(SequenceElement::Angle),
        ParamType::SequenceIntensity => Type::Sequence(SequenceElement::Intensity),
        ParamType::SequenceColor => Type::Sequence(SequenceElement::Color),
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

    #[test]
    fn sequence_return_types_round_trip() {
        for (param, elem) in [
            (ParamType::SequenceInt, SequenceElement::Int),
            (ParamType::SequenceFloat, SequenceElement::Float),
            (ParamType::SequenceAngle, SequenceElement::Angle),
            (ParamType::SequenceIntensity, SequenceElement::Intensity),
            (ParamType::SequenceColor, SequenceElement::Color),
        ] {
            assert_eq!(from_param_type(param), Type::Sequence(elem));
        }
    }

    #[test]
    fn sequence_type_as_an_argument_round_trips() {
        // `Effects.step(sequence: Sequence<T>, ...)` accepts one.
        for elem in SequenceElement::ALL.iter().copied() {
            assert_eq!(
                to_param_type(Type::Sequence(elem)),
                sequence_element_param_type(elem)
            );
        }
    }

    #[test]
    fn sequence_element_param_type_matches_return_type_tags() {
        for elem in SequenceElement::ALL.iter().copied() {
            assert_eq!(
                from_param_type(sequence_element_param_type(elem)),
                Type::Sequence(elem)
            );
        }
    }
}
