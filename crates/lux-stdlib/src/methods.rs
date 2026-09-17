//! Builtin instance methods on `Signal<Float>`: `.range()`, `.phase()`,
//! `.spread()`, `.invert()`.
//!
//! Deliberately **not** part of [`crate::registry::STD_MODULES`]: those
//! are looked up by `import`-qualified path (`std.Math`, `std.Effects`,
//! ...), but a method call (`wave.range(...)`) has no module qualifier at
//! all — its "namespace" is the receiver's *type*, `Signal<Float>`, not a
//! name anyone imports. So this is its own small, parallel table, reusing
//! [`Signature`]/[`Param`]/[`ParamType`] (the same shapes `registry`
//! uses) rather than inventing a second signature model.
//!
//! `range` is monomorphized per target element type, exactly like
//! `Signal.constant`'s 5 overloads: three `Signature`s (`Float`,
//! `Intensity`, `Angle`), disambiguated by argument type. This is also
//! what gives "range bounds must have the same type" for free — passing
//! `(Intensity, Angle)` matches none of the three overloads (each
//! requires both bounds to be the *same* `T`), so it falls out as an
//! ordinary `NoMatchingOverload`, not a special-cased check.
//!
//! `Signature.params` here lists only the method's own written arguments
//! (`min`/`max`, or `offset`), never the receiver — the receiver's type
//! (`Signal<Float>`) is checked separately, once, by the caller (see
//! `lux-typeck`'s `check_method_call`), since every method in this table
//! shares that same requirement. At the bytecode/VM level the receiver
//! *is* the first operand (see `lux_bytecode::IntrinsicId::param_types`
//! for `SignalRange*`/`SignalPhase`/`SignalInvert`) — `Signature.params`
//! and a bytecode intrinsic's operand list serve different purposes and
//! are allowed to disagree in shape, matching this workspace's existing
//! frontend/runtime duplication idiom.

use crate::intrinsic::IntrinsicId;
use crate::registry::OverloadError;
use crate::signature::{Param, Signature};
use crate::types::ParamType;

macro_rules! param {
    ($name:literal : $ty:ident) => {
        Param {
            name: $name,
            ty: ParamType::$ty,
        }
    };
}

static SIGNAL_RANGE_FLOAT: Signature = Signature {
    module_path: &["Signal<Float>"],
    name: "range",
    params: &[param!("min": Float), param!("max": Float)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::SignalRangeFloat,
    pure: true,
    doc: "range(min: Float, max: Float) -> Signal<Float> — remaps the source's `0.0..1.0` \
          value (clamped) to `min..max`: `min + x * (max - min)`. `min` may be greater than \
          `max`, which reverses the ramp; both bounds must have the same type.",
};

static SIGNAL_RANGE_INTENSITY: Signature = Signature {
    module_path: &["Signal<Float>"],
    name: "range",
    params: &[param!("min": Intensity), param!("max": Intensity)],
    return_ty: ParamType::SignalIntensity,
    intrinsic: IntrinsicId::SignalRangeIntensity,
    pure: true,
    doc: "range(min: Intensity, max: Intensity) -> Signal<Intensity> — remaps the source's \
          `0.0..1.0` value (clamped) to `min..max`. `min` may be greater than `max`, which \
          reverses the ramp; both bounds must have the same type.",
};

static SIGNAL_RANGE_ANGLE: Signature = Signature {
    module_path: &["Signal<Float>"],
    name: "range",
    params: &[param!("min": Angle), param!("max": Angle)],
    return_ty: ParamType::SignalAngle,
    intrinsic: IntrinsicId::SignalRangeAngle,
    pure: true,
    doc: "range(min: Angle, max: Angle) -> Signal<Angle> — remaps the source's `0.0..1.0` \
          value (clamped) to `min..max`. `min` may be greater than `max`, which reverses the \
          ramp; both bounds must have the same type.",
};

static SIGNAL_PHASE: Signature = Signature {
    module_path: &["Signal<Float>"],
    name: "phase",
    params: &[param!("offset": Angle)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::SignalPhase,
    pure: true,
    doc: "phase(offset: Angle) -> Signal<Float> — shifts the signal's cycle by \
          `offset / 360deg` of a period (`450deg` behaves like `90deg`; negative offsets shift \
          the other way). Only valid directly on an `Effects` oscillator (`sine`/`triangle`/\
          `saw`/`square`), or on another `.phase(...)` call chained from one — see \
          `docs/stdlib/Signal.md`.",
};

static SIGNAL_SPREAD_FLOAT: Signature = Signature {
    module_path: &["Signal<Float>"],
    name: "spread",
    params: &[param!("amount": Angle)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::SignalSpreadFloat,
    pure: true,
    doc: "spread(amount: Angle) -> Signal<Float> — distributes `amount` as a phase offset \
          across a fixture group: fixture `i` of `n` gets `amount * i / n` added to its own \
          sampled phase (not `n - 1`, so a full `360deg` spread never puts the first and last \
          fixture back in phase). Only meaningful once bound to a multi-fixture target with \
          `<-` — outside a binding (`fixture_count == 1`), every fixture reads offset `0`. Valid \
          directly on an `Effects` oscillator, on a `.phase(...)` call chained from one (not \
          chainable with another `.spread(...)`), or directly on an `Effects.step(...)` signal \
          (any element type — see the non-`Float` `SIGNAL_SPREAD_*` signatures below).",
};

static SIGNAL_INVERT: Signature = Signature {
    module_path: &["Signal<Float>"],
    name: "invert",
    params: &[],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::SignalInvert,
    pure: true,
    doc: "invert() -> Signal<Float> — `1.0 - source`, sampled fresh each time. Not clamped: \
          inverting a signal outside `0.0..1.0` (e.g. after `.range(10.0, 20.0)`) produces a \
          value outside `0.0..1.0` too.",
};

/// Every builtin method on `Signal<Float>`, in a fixed, name-contiguous
/// order (see [`signal_float_method_candidates`]'s docs).
pub static SIGNAL_FLOAT_METHODS: &[Signature] = &[
    SIGNAL_RANGE_FLOAT,
    SIGNAL_RANGE_INTENSITY,
    SIGNAL_RANGE_ANGLE,
    SIGNAL_PHASE,
    SIGNAL_SPREAD_FLOAT,
    SIGNAL_INVERT,
];

/// Every overload named `name` on `Signal<Float>`, regardless of arity —
/// the method-call counterpart to [`crate::registry::candidates`]. Relies
/// on the same "overloads sharing a name are listed contiguously"
/// invariant, true by construction in [`SIGNAL_FLOAT_METHODS`] above.
pub fn signal_float_method_candidates(name: &str) -> &'static [Signature] {
    let Some(start) = SIGNAL_FLOAT_METHODS.iter().position(|s| s.name == name) else {
        return &[];
    };
    let len = SIGNAL_FLOAT_METHODS[start..]
        .iter()
        .take_while(|s| s.name == name)
        .count();
    &SIGNAL_FLOAT_METHODS[start..start + len]
}

/// Resolves `.name(arg_types)` on a `Signal<Float>` receiver to the one
/// matching [`Signature`] — the method-call counterpart to
/// [`crate::registry::resolve_overload`], minus the module-lookup step
/// (there is exactly one "module" here, so [`OverloadError::UnknownModule`]
/// can never occur).
pub fn resolve_signal_float_method(
    name: &str,
    arg_types: &[ParamType],
) -> Result<&'static Signature, OverloadError> {
    let by_name = signal_float_method_candidates(name);
    if by_name.is_empty() {
        return Err(OverloadError::UnknownMember);
    }
    let exact: Vec<&'static Signature> = by_name
        .iter()
        .filter(|sig| sig.params.len() == arg_types.len())
        .filter(|sig| {
            sig.params
                .iter()
                .zip(arg_types.iter())
                .all(|(p, a)| p.ty == *a)
        })
        .collect();
    match exact.len() {
        0 => {
            let arities: Vec<usize> = by_name.iter().map(Signature::arity).collect();
            if arities.contains(&arg_types.len()) {
                Err(OverloadError::NoMatchingOverload)
            } else {
                Err(OverloadError::ArityMismatch {
                    expected_arities: arities,
                    found: arg_types.len(),
                })
            }
        }
        1 => Ok(exact[0]),
        _ => Err(OverloadError::Ambiguous(exact)),
    }
}

/// `.spread()` on a non-`Float` `Signal<T>` — i.e. one built from
/// `Effects.step(sequence: Sequence<T>, ...)` for `T` in `{Int, Angle,
/// Intensity, Color}`. Unlike `Signal<Float>`'s full method set
/// (`range`/`phase`/`spread`/`invert`, all in [`SIGNAL_FLOAT_METHODS`]),
/// `.spread()` is the *only* method these element types support:
/// `.range()`/`.phase()`/`.invert()` stay meaningless (and thus
/// unsupported) for anything but a continuous `0.0..1.0` `Float` signal —
/// see item 9/10 of the `Effects.step` task brief for why `.spread()`
/// specifically generalizes to a discrete `Step` signal (a time-offset
/// within the sequence's full cycle) where the others don't.
static SIGNAL_SPREAD_INT: Signature = Signature {
    module_path: &["Signal<Int>"],
    name: "spread",
    params: &[param!("amount": Angle)],
    return_ty: ParamType::SignalInt,
    intrinsic: IntrinsicId::SignalSpreadInt,
    pure: true,
    doc: "spread(amount: Angle) -> Signal<Int> — on an `Effects.step(...)` signal, distributes \
          `amount` as a time offset within the sequence's full cycle (`every * length`): \
          fixture `i` of `n` gets `amount * i / n` of that cycle added to its own sampled \
          position. Only meaningful once bound to a multi-fixture target with `<-`.",
};

static SIGNAL_SPREAD_ANGLE: Signature = Signature {
    module_path: &["Signal<Angle>"],
    name: "spread",
    params: &[param!("amount": Angle)],
    return_ty: ParamType::SignalAngle,
    intrinsic: IntrinsicId::SignalSpreadAngle,
    pure: true,
    doc: "spread(amount: Angle) -> Signal<Angle> — on an `Effects.step(...)` signal, distributes \
          `amount` as a time offset within the sequence's full cycle (`every * length`): \
          fixture `i` of `n` gets `amount * i / n` of that cycle added to its own sampled \
          position. Only meaningful once bound to a multi-fixture target with `<-`.",
};

static SIGNAL_SPREAD_INTENSITY: Signature = Signature {
    module_path: &["Signal<Intensity>"],
    name: "spread",
    params: &[param!("amount": Angle)],
    return_ty: ParamType::SignalIntensity,
    intrinsic: IntrinsicId::SignalSpreadIntensity,
    pure: true,
    doc: "spread(amount: Angle) -> Signal<Intensity> — on an `Effects.step(...)` signal, \
          distributes `amount` as a time offset within the sequence's full cycle \
          (`every * length`): fixture `i` of `n` gets `amount * i / n` of that cycle added to \
          its own sampled position. Only meaningful once bound to a multi-fixture target with \
          `<-`.",
};

static SIGNAL_SPREAD_COLOR: Signature = Signature {
    module_path: &["Signal<Color>"],
    name: "spread",
    params: &[param!("amount": Angle)],
    return_ty: ParamType::SignalColor,
    intrinsic: IntrinsicId::SignalSpreadColor,
    pure: true,
    doc: "spread(amount: Angle) -> Signal<Color> — on an `Effects.step(...)` signal, distributes \
          `amount` as a time offset within the sequence's full cycle (`every * length`): \
          fixture `i` of `n` gets `amount * i / n` of that cycle added to its own sampled \
          position. Only meaningful once bound to a multi-fixture target with `<-`.",
};

fn non_float_signal_spread_for(receiver: ParamType) -> &'static [Signature] {
    match receiver {
        ParamType::SignalInt => std::slice::from_ref(&SIGNAL_SPREAD_INT),
        ParamType::SignalAngle => std::slice::from_ref(&SIGNAL_SPREAD_ANGLE),
        ParamType::SignalIntensity => std::slice::from_ref(&SIGNAL_SPREAD_INTENSITY),
        ParamType::SignalColor => std::slice::from_ref(&SIGNAL_SPREAD_COLOR),
        _ => &[],
    }
}

/// Every overload named `name` on a non-`Float` `Signal<T>` receiver
/// tagged `receiver` — the counterpart to [`signal_float_method_candidates`]
/// for the other 4 element types, which only ever have `.spread()`.
pub fn non_float_signal_method_candidates(receiver: ParamType, name: &str) -> &'static [Signature] {
    let methods = non_float_signal_spread_for(receiver);
    let Some(start) = methods.iter().position(|s| s.name == name) else {
        return &[];
    };
    let len = methods[start..]
        .iter()
        .take_while(|s| s.name == name)
        .count();
    &methods[start..start + len]
}

/// Resolves `.name(arg_types)` on a non-`Float` `Signal<T>` receiver
/// tagged `receiver` — the counterpart to [`resolve_signal_float_method`]
/// for the other 4 element types.
pub fn resolve_non_float_signal_method(
    receiver: ParamType,
    name: &str,
    arg_types: &[ParamType],
) -> Result<&'static Signature, OverloadError> {
    if non_float_signal_spread_for(receiver).is_empty() {
        return Err(OverloadError::UnknownModule);
    }
    let by_name = non_float_signal_method_candidates(receiver, name);
    if by_name.is_empty() {
        return Err(OverloadError::UnknownMember);
    }
    let exact: Vec<&'static Signature> = by_name
        .iter()
        .filter(|sig| sig.params.len() == arg_types.len())
        .filter(|sig| {
            sig.params
                .iter()
                .zip(arg_types.iter())
                .all(|(p, a)| p.ty == *a)
        })
        .collect();
    match exact.len() {
        0 => {
            let arities: Vec<usize> = by_name.iter().map(Signature::arity).collect();
            if arities.contains(&arg_types.len()) {
                Err(OverloadError::NoMatchingOverload)
            } else {
                Err(OverloadError::ArityMismatch {
                    expected_arities: arities,
                    found: arg_types.len(),
                })
            }
        }
        1 => Ok(exact[0]),
        _ => Err(OverloadError::Ambiguous(exact)),
    }
}

static SEQUENCE_LENGTH_INT: Signature = Signature {
    module_path: &["Sequence<Int>"],
    name: "length",
    params: &[],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::SequenceLengthInt,
    pure: true,
    doc: "length() -> Int — the number of elements in this `Sequence<T>`.",
};

static SEQUENCE_LENGTH_FLOAT: Signature = Signature {
    module_path: &["Sequence<Float>"],
    name: "length",
    params: &[],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::SequenceLengthFloat,
    pure: true,
    doc: "length() -> Int — the number of elements in this `Sequence<T>`.",
};

static SEQUENCE_LENGTH_ANGLE: Signature = Signature {
    module_path: &["Sequence<Angle>"],
    name: "length",
    params: &[],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::SequenceLengthAngle,
    pure: true,
    doc: "length() -> Int — the number of elements in this `Sequence<T>`.",
};

static SEQUENCE_LENGTH_INTENSITY: Signature = Signature {
    module_path: &["Sequence<Intensity>"],
    name: "length",
    params: &[],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::SequenceLengthIntensity,
    pure: true,
    doc: "length() -> Int — the number of elements in this `Sequence<T>`.",
};

static SEQUENCE_LENGTH_COLOR: Signature = Signature {
    module_path: &["Sequence<Color>"],
    name: "length",
    params: &[],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::SequenceLengthColor,
    pure: true,
    doc: "length() -> Int — the number of elements in this `Sequence<T>`.",
};

/// Every builtin method on a `Sequence<T>` receiver tagged `receiver` (one
/// of the 5 `ParamType::Sequence*` variants — see
/// `lux_typeck::stdlib_bridge::sequence_element_param_type`, the only
/// producer of this tag). `V1` has exactly one such method (`.length()`),
/// monomorphized per element type like `SEQUENCE_OF_*` in `crate::registry`
/// — kept as its own small table (mirroring `SIGNAL_FLOAT_METHODS`'s
/// module doc: a method call has no import-qualified module, its
/// "namespace" is the receiver's type) rather than folded into
/// `SIGNAL_FLOAT_METHODS`, since the receiver here isn't always the same
/// type.
fn sequence_methods_for(receiver: ParamType) -> &'static [Signature] {
    match receiver {
        ParamType::SequenceInt => std::slice::from_ref(&SEQUENCE_LENGTH_INT),
        ParamType::SequenceFloat => std::slice::from_ref(&SEQUENCE_LENGTH_FLOAT),
        ParamType::SequenceAngle => std::slice::from_ref(&SEQUENCE_LENGTH_ANGLE),
        ParamType::SequenceIntensity => std::slice::from_ref(&SEQUENCE_LENGTH_INTENSITY),
        ParamType::SequenceColor => std::slice::from_ref(&SEQUENCE_LENGTH_COLOR),
        _ => &[],
    }
}

/// Every overload named `name` on a `Sequence<T>` receiver tagged
/// `receiver` — the `Sequence` counterpart to
/// [`signal_float_method_candidates`]. `V1` never has more than one
/// overload per name, but this still returns a slice (not `Option`) to
/// keep the same shape LSP signature-help expects from every other
/// `*_candidates` function.
pub fn sequence_method_candidates(receiver: ParamType, name: &str) -> &'static [Signature] {
    let methods = sequence_methods_for(receiver);
    let Some(start) = methods.iter().position(|s| s.name == name) else {
        return &[];
    };
    let len = methods[start..]
        .iter()
        .take_while(|s| s.name == name)
        .count();
    &methods[start..start + len]
}

/// Resolves `.name(arg_types)` on a `Sequence<T>` receiver tagged
/// `receiver` to the one matching [`Signature`] — the `Sequence`
/// counterpart to [`resolve_signal_float_method`].
pub fn resolve_sequence_method(
    receiver: ParamType,
    name: &str,
    arg_types: &[ParamType],
) -> Result<&'static Signature, OverloadError> {
    if sequence_methods_for(receiver).is_empty() {
        return Err(OverloadError::UnknownModule);
    }
    let by_name = sequence_method_candidates(receiver, name);
    if by_name.is_empty() {
        return Err(OverloadError::UnknownMember);
    }
    let exact: Vec<&'static Signature> = by_name
        .iter()
        .filter(|sig| sig.params.len() == arg_types.len())
        .filter(|sig| {
            sig.params
                .iter()
                .zip(arg_types.iter())
                .all(|(p, a)| p.ty == *a)
        })
        .collect();
    match exact.len() {
        0 => {
            let arities: Vec<usize> = by_name.iter().map(Signature::arity).collect();
            if arities.contains(&arg_types.len()) {
                Err(OverloadError::NoMatchingOverload)
            } else {
                Err(OverloadError::ArityMismatch {
                    expected_arities: arities,
                    found: arg_types.len(),
                })
            }
        }
        1 => Ok(exact[0]),
        _ => Err(OverloadError::Ambiguous(exact)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_resolves_per_element_type() {
        let cases = [
            (
                ParamType::Float,
                IntrinsicId::SignalRangeFloat,
                ParamType::SignalFloat,
            ),
            (
                ParamType::Intensity,
                IntrinsicId::SignalRangeIntensity,
                ParamType::SignalIntensity,
            ),
            (
                ParamType::Angle,
                IntrinsicId::SignalRangeAngle,
                ParamType::SignalAngle,
            ),
        ];
        for (ty, intrinsic, return_ty) in cases {
            let sig = resolve_signal_float_method("range", &[ty, ty]).unwrap();
            assert_eq!(sig.intrinsic, intrinsic);
            assert_eq!(sig.return_ty, return_ty);
        }
    }

    #[test]
    fn range_with_mismatched_bound_types_has_no_matching_overload() {
        assert_eq!(
            resolve_signal_float_method("range", &[ParamType::Intensity, ParamType::Angle]),
            Err(OverloadError::NoMatchingOverload)
        );
    }

    #[test]
    fn range_does_not_support_color() {
        assert_eq!(
            resolve_signal_float_method("range", &[ParamType::Color, ParamType::Color]),
            Err(OverloadError::NoMatchingOverload)
        );
    }

    #[test]
    fn phase_spread_and_invert_resolve() {
        let phase = resolve_signal_float_method("phase", &[ParamType::Angle]).unwrap();
        assert_eq!(phase.intrinsic, IntrinsicId::SignalPhase);
        let spread = resolve_signal_float_method("spread", &[ParamType::Angle]).unwrap();
        assert_eq!(spread.intrinsic, IntrinsicId::SignalSpreadFloat);
        assert_eq!(spread.return_ty, ParamType::SignalFloat);
        let invert = resolve_signal_float_method("invert", &[]).unwrap();
        assert_eq!(invert.intrinsic, IntrinsicId::SignalInvert);
    }

    #[test]
    fn unknown_method_is_reported() {
        assert_eq!(
            resolve_signal_float_method("bogus", &[]),
            Err(OverloadError::UnknownMember)
        );
    }

    #[test]
    fn arity_mismatch_is_reported() {
        assert_eq!(
            resolve_signal_float_method("invert", &[ParamType::Float]),
            Err(OverloadError::ArityMismatch {
                expected_arities: vec![0],
                found: 1,
            })
        );
    }

    #[test]
    fn all_signal_float_methods_are_pure() {
        for sig in SIGNAL_FLOAT_METHODS {
            assert!(sig.pure, "{} should be pure", sig.name);
        }
    }

    #[test]
    fn candidates_returns_every_overload() {
        assert_eq!(signal_float_method_candidates("range").len(), 3);
        assert_eq!(signal_float_method_candidates("phase").len(), 1);
        assert_eq!(signal_float_method_candidates("spread").len(), 1);
        assert_eq!(signal_float_method_candidates("bogus").len(), 0);
    }

    #[test]
    fn sequence_length_resolves_per_element_type() {
        let cases = [
            (ParamType::SequenceInt, IntrinsicId::SequenceLengthInt),
            (ParamType::SequenceFloat, IntrinsicId::SequenceLengthFloat),
            (ParamType::SequenceAngle, IntrinsicId::SequenceLengthAngle),
            (
                ParamType::SequenceIntensity,
                IntrinsicId::SequenceLengthIntensity,
            ),
            (ParamType::SequenceColor, IntrinsicId::SequenceLengthColor),
        ];
        for (receiver, intrinsic) in cases {
            let sig = resolve_sequence_method(receiver, "length", &[]).unwrap();
            assert_eq!(sig.intrinsic, intrinsic);
            assert_eq!(sig.return_ty, ParamType::Int);
        }
    }

    #[test]
    fn sequence_length_rejects_arguments() {
        assert_eq!(
            resolve_sequence_method(ParamType::SequenceColor, "length", &[ParamType::Int]),
            Err(OverloadError::ArityMismatch {
                expected_arities: vec![0],
                found: 1,
            })
        );
    }

    #[test]
    fn sequence_unknown_method_is_reported() {
        assert_eq!(
            resolve_sequence_method(ParamType::SequenceColor, "bogus", &[]),
            Err(OverloadError::UnknownMember)
        );
    }

    #[test]
    fn non_sequence_receiver_is_unknown_module() {
        assert_eq!(
            resolve_sequence_method(ParamType::Int, "length", &[]),
            Err(OverloadError::UnknownModule)
        );
    }

    #[test]
    fn non_float_signal_spread_resolves_per_element_type() {
        let cases = [
            (ParamType::SignalInt, IntrinsicId::SignalSpreadInt),
            (ParamType::SignalAngle, IntrinsicId::SignalSpreadAngle),
            (
                ParamType::SignalIntensity,
                IntrinsicId::SignalSpreadIntensity,
            ),
            (ParamType::SignalColor, IntrinsicId::SignalSpreadColor),
        ];
        for (receiver, intrinsic) in cases {
            let sig =
                resolve_non_float_signal_method(receiver, "spread", &[ParamType::Angle]).unwrap();
            assert_eq!(sig.intrinsic, intrinsic);
        }
    }

    #[test]
    fn non_float_signal_only_supports_spread() {
        assert_eq!(
            resolve_non_float_signal_method(ParamType::SignalColor, "range", &[]),
            Err(OverloadError::UnknownMember)
        );
        assert_eq!(
            resolve_non_float_signal_method(ParamType::SignalColor, "invert", &[]),
            Err(OverloadError::UnknownMember)
        );
    }

    #[test]
    fn signal_float_receiver_is_unknown_module_for_non_float_table() {
        assert_eq!(
            resolve_non_float_signal_method(ParamType::SignalFloat, "spread", &[ParamType::Angle]),
            Err(OverloadError::UnknownModule)
        );
    }
}
