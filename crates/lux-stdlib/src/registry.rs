//! The stdlib module/function tables and the overload resolution that
//! call sites go through. This is the single source of truth consulted by
//! `lux-hir` (import/call resolution), `lux-typeck` (type checking) and
//! `lux-lsp` (completion/hover/signature help) — none of them hardcode a
//! function list of their own.

use crate::intrinsic::IntrinsicId;
use crate::signature::{Param, Signature, StdModule};
use crate::types::ParamType;

macro_rules! param {
    ($name:literal : $ty:ident) => {
        Param {
            name: $name,
            ty: ParamType::$ty,
        }
    };
}

static MATH_SIN: Signature = Signature {
    module_path: &["std", "Math"],
    name: "sin",
    params: &[param!("angle": Angle)],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathSin,
    pure: true,
    doc: "sin(angle: Angle) -> Float — sine of `angle`. `sin(90deg) == 1.0`.",
};

static MATH_COS: Signature = Signature {
    module_path: &["std", "Math"],
    name: "cos",
    params: &[param!("angle": Angle)],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathCos,
    pure: true,
    doc: "cos(angle: Angle) -> Float — cosine of `angle`. `cos(0deg) == 1.0`.",
};

static MATH_ABS_INT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "abs",
    params: &[param!("value": Int)],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::MathAbsInt,
    pure: true,
    doc: "abs(value: Int) -> Int — absolute value.",
};

static MATH_ABS_FLOAT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "abs",
    params: &[param!("value": Float)],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathAbsFloat,
    pure: true,
    doc: "abs(value: Float) -> Float — absolute value.",
};

static MATH_MIN_INT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "min",
    params: &[param!("a": Int), param!("b": Int)],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::MathMinInt,
    pure: true,
    doc: "min(a: Int, b: Int) -> Int — the smaller of the two.",
};

static MATH_MIN_FLOAT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "min",
    params: &[param!("a": Float), param!("b": Float)],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathMinFloat,
    pure: true,
    doc: "min(a: Float, b: Float) -> Float — the smaller of the two.",
};

static MATH_MAX_INT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "max",
    params: &[param!("a": Int), param!("b": Int)],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::MathMaxInt,
    pure: true,
    doc: "max(a: Int, b: Int) -> Int — the larger of the two.",
};

static MATH_MAX_FLOAT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "max",
    params: &[param!("a": Float), param!("b": Float)],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathMaxFloat,
    pure: true,
    doc: "max(a: Float, b: Float) -> Float — the larger of the two.",
};

static MATH_CLAMP_INT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "clamp",
    params: &[param!("value": Int), param!("min": Int), param!("max": Int)],
    return_ty: ParamType::Int,
    intrinsic: IntrinsicId::MathClampInt,
    pure: true,
    doc: "clamp(value: Int, min: Int, max: Int) -> Int — `value` restricted to `[min, max]`. \
          Computed as `value.max(min).min(max)`; if `min > max`, always returns `max` (documented, not an error).",
};

static MATH_CLAMP_FLOAT: Signature = Signature {
    module_path: &["std", "Math"],
    name: "clamp",
    params: &[
        param!("value": Float),
        param!("min": Float),
        param!("max": Float),
    ],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathClampFloat,
    pure: true,
    doc: "clamp(value: Float, min: Float, max: Float) -> Float — `value` restricted to `[min, max]`. \
          Computed as `value.max(min).min(max)`; if `min > max`, always returns `max` (documented, not an error).",
};

static MATH_LERP: Signature = Signature {
    module_path: &["std", "Math"],
    name: "lerp",
    params: &[param!("a": Float), param!("b": Float), param!("t": Float)],
    return_ty: ParamType::Float,
    intrinsic: IntrinsicId::MathLerp,
    pure: true,
    doc: "lerp(a: Float, b: Float, t: Float) -> Float — linear interpolation `a + (b - a) * t`. \
          `t` is NOT clamped: `t > 1.0` extrapolates past `b`.",
};

static MATH_FUNCTIONS: &[Signature] = &[
    MATH_SIN,
    MATH_COS,
    MATH_ABS_INT,
    MATH_ABS_FLOAT,
    MATH_MIN_INT,
    MATH_MIN_FLOAT,
    MATH_MAX_INT,
    MATH_MAX_FLOAT,
    MATH_CLAMP_INT,
    MATH_CLAMP_FLOAT,
    MATH_LERP,
];

static MATH: StdModule = StdModule {
    path: &["std", "Math"],
    short_name: "Math",
    functions: MATH_FUNCTIONS,
    doc: "Pure scalar math: trigonometry, min/max/abs/clamp, linear interpolation. \
          No clock, IO, randomness or hardware access — every function is deterministic.",
};

static COLOR_RGB: Signature = Signature {
    module_path: &["std", "Color"],
    name: "rgb",
    params: &[param!("r": Int), param!("g": Int), param!("b": Int)],
    return_ty: ParamType::Color,
    intrinsic: IntrinsicId::ColorRgb,
    pure: true,
    doc: "rgb(r: Int, g: Int, b: Int) -> Color — each channel in `0..=255`. \
          A constant channel outside that range is a compile-time error; a non-constant \
          out-of-range value saturates to `0`/`255` at runtime.",
};

static COLOR_MIX: Signature = Signature {
    module_path: &["std", "Color"],
    name: "mix",
    params: &[
        param!("a": Color),
        param!("b": Color),
        param!("ratio": Float),
    ],
    return_ty: ParamType::Color,
    intrinsic: IntrinsicId::ColorMix,
    pure: true,
    doc: "mix(a: Color, b: Color, ratio: Float) -> Color — blends `a` (ratio 0.0) to `b` (ratio 1.0). \
          `ratio` is clamped to `[0.0, 1.0]` (never an error).",
};

static COLOR_HSV: Signature = Signature {
    module_path: &["std", "Color"],
    name: "hsv",
    params: &[
        param!("hue": Angle),
        param!("saturation": Intensity),
        param!("value": Intensity),
    ],
    return_ty: ParamType::Color,
    intrinsic: IntrinsicId::ColorHsv,
    pure: true,
    doc: "hsv(hue: Angle, saturation: Intensity, value: Intensity) -> Color — standard HSV to RGB. \
          `hue` wraps modulo 360deg; `hsv(0deg, 100%, 100%)` is red, `hsv(120deg, 100%, 100%)` is green, \
          `hsv(240deg, 100%, 100%)` is blue.",
};

static COLOR_FUNCTIONS: &[Signature] = &[COLOR_RGB, COLOR_MIX, COLOR_HSV];

static COLOR: StdModule = StdModule {
    path: &["std", "Color"],
    short_name: "Color",
    functions: COLOR_FUNCTIONS,
    doc: "Color composition: build colors from channels, blend them, or build from hue/saturation/value.",
};

static SIGNAL_CONSTANT_INT: Signature = Signature {
    module_path: &["std", "Signal"],
    name: "constant",
    params: &[param!("value": Int)],
    return_ty: ParamType::SignalInt,
    intrinsic: IntrinsicId::SignalConstantInt,
    pure: true,
    doc: "constant(value: Int) -> Signal<Int> — a signal that always evaluates to `value`, for any timestamp.",
};

static SIGNAL_CONSTANT_FLOAT: Signature = Signature {
    module_path: &["std", "Signal"],
    name: "constant",
    params: &[param!("value": Float)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::SignalConstantFloat,
    pure: true,
    doc: "constant(value: Float) -> Signal<Float> — a signal that always evaluates to `value`, for any timestamp.",
};

static SIGNAL_CONSTANT_ANGLE: Signature = Signature {
    module_path: &["std", "Signal"],
    name: "constant",
    params: &[param!("value": Angle)],
    return_ty: ParamType::SignalAngle,
    intrinsic: IntrinsicId::SignalConstantAngle,
    pure: true,
    doc: "constant(value: Angle) -> Signal<Angle> — a signal that always evaluates to `value`, for any timestamp.",
};

static SIGNAL_CONSTANT_INTENSITY: Signature = Signature {
    module_path: &["std", "Signal"],
    name: "constant",
    params: &[param!("value": Intensity)],
    return_ty: ParamType::SignalIntensity,
    intrinsic: IntrinsicId::SignalConstantIntensity,
    pure: true,
    doc: "constant(value: Intensity) -> Signal<Intensity> — a signal that always evaluates to `value`, for any timestamp.",
};

static SIGNAL_CONSTANT_COLOR: Signature = Signature {
    module_path: &["std", "Signal"],
    name: "constant",
    params: &[param!("value": Color)],
    return_ty: ParamType::SignalColor,
    intrinsic: IntrinsicId::SignalConstantColor,
    pure: true,
    doc: "constant(value: Color) -> Signal<Color> — a signal that always evaluates to `value`, for any timestamp.",
};

static SIGNAL_FUNCTIONS: &[Signature] = &[
    SIGNAL_CONSTANT_INT,
    SIGNAL_CONSTANT_FLOAT,
    SIGNAL_CONSTANT_ANGLE,
    SIGNAL_CONSTANT_INTENSITY,
    SIGNAL_CONSTANT_COLOR,
];

static SIGNAL: StdModule = StdModule {
    path: &["std", "Signal"],
    short_name: "Signal",
    functions: SIGNAL_FUNCTIONS,
    doc: "Time-sampled values: `Signal<T>` evaluates to a `T` at a given timestamp. \
          V1 only supports `Signal.constant`, a signal whose value never depends on time.",
};

static EFFECTS_SINE: Signature = Signature {
    module_path: &["std", "Effects"],
    name: "sine",
    params: &[param!("period": Duration)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::EffectsSine,
    pure: false,
    doc: "sine(period: Duration) -> Signal<Float> — a sine-wave oscillator normalized to `0.0..1.0`: \
          `phase 0.00 -> 0.5, 0.25 -> 1.0, 0.50 -> 0.5, 0.75 -> 0.0`. `period` must be greater than zero. \
          The oscillator's origin (`phase == 0`) is the timestamp it was created at.",
};

static EFFECTS_TRIANGLE: Signature = Signature {
    module_path: &["std", "Effects"],
    name: "triangle",
    params: &[param!("period": Duration)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::EffectsTriangle,
    pure: false,
    doc: "triangle(period: Duration) -> Signal<Float> — a triangle-wave oscillator normalized to `0.0..1.0`: \
          `phase 0.00 -> 0.0, 0.25 -> 0.5, 0.50 -> 1.0, 0.75 -> 0.5`. `period` must be greater than zero. \
          The oscillator's origin (`phase == 0`) is the timestamp it was created at.",
};

static EFFECTS_SAW: Signature = Signature {
    module_path: &["std", "Effects"],
    name: "saw",
    params: &[param!("period": Duration)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::EffectsSaw,
    pure: false,
    doc: "saw(period: Duration) -> Signal<Float> — a rising sawtooth oscillator normalized to `0.0..1.0`: \
          `phase 0.00 -> 0.0, 0.25 -> 0.25, 0.50 -> 0.5, 0.75 -> 0.75`, then it snaps back to `0.0`. \
          `period` must be greater than zero. The oscillator's origin (`phase == 0`) is the timestamp \
          it was created at. There is no falling/descending variant in V1.",
};

static EFFECTS_SQUARE: Signature = Signature {
    module_path: &["std", "Effects"],
    name: "square",
    params: &[param!("period": Duration)],
    return_ty: ParamType::SignalFloat,
    intrinsic: IntrinsicId::EffectsSquare,
    pure: false,
    doc: "square(period: Duration) -> Signal<Float> — a 50% duty-cycle square-wave oscillator: \
          `1.0` for the first half of each period, `0.0` for the second half. `period` must be \
          greater than zero. The oscillator's origin (`phase == 0`) is the timestamp it was created at.",
};

static EFFECTS_FUNCTIONS: &[Signature] =
    &[EFFECTS_SINE, EFFECTS_TRIANGLE, EFFECTS_SAW, EFFECTS_SQUARE];

static EFFECTS: StdModule = StdModule {
    path: &["std", "Effects"],
    short_name: "Effects",
    functions: EFFECTS_FUNCTIONS,
    doc: "Time-varying oscillators: each function returns a `Signal<Float>` normalized to `0.0..1.0`, \
          whose value depends only on absolute time — never on a frame count or tick delta. Unlike \
          `std.Math`/`std.Color`, these are not pure functions of their arguments alone: each call \
          captures the current runtime clock as the oscillator's time origin.",
};

pub static STD_MODULES: &[StdModule] = &[MATH, COLOR, SIGNAL, EFFECTS];

pub fn find_module(path: &[&str]) -> Option<&'static StdModule> {
    STD_MODULES.iter().find(|m| m.path == path)
}

pub fn find_module_by_short_name(short_name: &str) -> Option<&'static StdModule> {
    STD_MODULES.iter().find(|m| m.short_name == short_name)
}

/// Every signature named `name` in the module at `module_path`, regardless
/// of arity/types — used for arity-mismatch diagnostics and for LSP
/// signature help, which wants to show every overload at once.
///
/// Relies on the invariant (true by construction in `MATH_FUNCTIONS`/
/// `COLOR_FUNCTIONS` above) that overloads sharing a name are listed
/// contiguously, so the matching run can be returned as a single
/// sub-slice without allocating.
pub fn candidates(module_path: &[&str], name: &str) -> &'static [Signature] {
    let Some(module) = find_module(module_path) else {
        return &[];
    };
    let Some(start) = module.functions.iter().position(|s| s.name == name) else {
        return &[];
    };
    let len = module.functions[start..]
        .iter()
        .take_while(|s| s.name == name)
        .count();
    &module.functions[start..start + len]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverloadError {
    UnknownModule,
    UnknownMember,
    ArityMismatch {
        expected_arities: Vec<usize>,
        found: usize,
    },
    NoMatchingOverload,
    /// Structurally unreachable in V1 (Int/Float overloads never overlap),
    /// kept as a real, tested error path for forward compatibility rather
    /// than removed.
    Ambiguous(Vec<&'static Signature>),
}

/// Resolves `module_path.name(arg_types)` to the one matching `Signature`,
/// with exact positional type matching — no implicit conversions.
pub fn resolve_overload(
    module_path: &[&str],
    name: &str,
    arg_types: &[ParamType],
) -> Result<&'static Signature, OverloadError> {
    if find_module(module_path).is_none() {
        return Err(OverloadError::UnknownModule);
    }
    let by_name = candidates(module_path, name);
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
    fn resolves_exact_match() {
        let sig = resolve_overload(&["std", "Math"], "sin", &[ParamType::Angle]).unwrap();
        assert_eq!(sig.intrinsic, IntrinsicId::MathSin);
    }

    #[test]
    fn resolves_signal_constant_overload_per_element_type() {
        let cases = [
            (
                ParamType::Int,
                IntrinsicId::SignalConstantInt,
                ParamType::SignalInt,
            ),
            (
                ParamType::Float,
                IntrinsicId::SignalConstantFloat,
                ParamType::SignalFloat,
            ),
            (
                ParamType::Angle,
                IntrinsicId::SignalConstantAngle,
                ParamType::SignalAngle,
            ),
            (
                ParamType::Intensity,
                IntrinsicId::SignalConstantIntensity,
                ParamType::SignalIntensity,
            ),
            (
                ParamType::Color,
                IntrinsicId::SignalConstantColor,
                ParamType::SignalColor,
            ),
        ];
        for (arg, intrinsic, return_ty) in cases {
            let sig = resolve_overload(&["std", "Signal"], "constant", &[arg]).unwrap();
            assert_eq!(sig.intrinsic, intrinsic);
            assert_eq!(sig.return_ty, return_ty);
        }
    }

    #[test]
    fn signal_constant_has_five_overloads() {
        assert_eq!(candidates(&["std", "Signal"], "constant").len(), 5);
    }

    #[test]
    fn resolves_int_and_float_overloads_distinctly() {
        let int_sig = resolve_overload(&["std", "Math"], "abs", &[ParamType::Int]).unwrap();
        assert_eq!(int_sig.intrinsic, IntrinsicId::MathAbsInt);
        let float_sig = resolve_overload(&["std", "Math"], "abs", &[ParamType::Float]).unwrap();
        assert_eq!(float_sig.intrinsic, IntrinsicId::MathAbsFloat);
    }

    #[test]
    fn unknown_module_is_reported() {
        assert_eq!(
            resolve_overload(&["std", "Foo"], "bar", &[]),
            Err(OverloadError::UnknownModule)
        );
    }

    #[test]
    fn unknown_member_is_reported() {
        assert_eq!(
            resolve_overload(&["std", "Math"], "bogus", &[ParamType::Int]),
            Err(OverloadError::UnknownMember)
        );
    }

    #[test]
    fn arity_mismatch_is_reported() {
        assert_eq!(
            resolve_overload(&["std", "Math"], "sin", &[]),
            Err(OverloadError::ArityMismatch {
                expected_arities: vec![1],
                found: 0,
            })
        );
    }

    #[test]
    fn wrong_type_with_matching_arity_is_no_matching_overload() {
        assert_eq!(
            resolve_overload(&["std", "Math"], "sin", &[ParamType::Color]),
            Err(OverloadError::NoMatchingOverload)
        );
    }

    #[test]
    fn candidates_returns_every_overload() {
        assert_eq!(candidates(&["std", "Math"], "clamp").len(), 2);
        assert_eq!(candidates(&["std", "Math"], "sin").len(), 1);
        assert_eq!(candidates(&["std", "Math"], "bogus").len(), 0);
    }

    #[test]
    fn find_module_by_full_path_and_short_name() {
        assert_eq!(find_module(&["std", "Color"]).unwrap().short_name, "Color");
        assert_eq!(
            find_module_by_short_name("Color").unwrap().path,
            &["std", "Color"]
        );
        assert!(find_module(&["std", "Foo"]).is_none());
    }

    #[test]
    fn color_and_math_signatures_are_pure() {
        for module in [&MATH, &COLOR, &SIGNAL] {
            for sig in module.functions {
                assert!(
                    sig.pure,
                    "{}.{} should be pure",
                    module.short_name, sig.name
                );
            }
        }
    }

    /// `std.Effects` is the deliberate exception: each oscillator captures
    /// `clock.now()` as its time origin at construction, so two calls to
    /// the exact same expression at different times produce signals that
    /// sample differently — see `Signature::pure`'s docs.
    #[test]
    fn effects_signatures_are_deliberately_impure() {
        for sig in EFFECTS_FUNCTIONS {
            assert!(
                !sig.pure,
                "{}.{} reads the clock and must not be marked pure",
                EFFECTS.short_name, sig.name
            );
        }
    }

    #[test]
    fn effects_module_exposes_all_four_oscillators() {
        let names: Vec<_> = EFFECTS_FUNCTIONS.iter().map(|s| s.name).collect();
        assert_eq!(names, ["sine", "triangle", "saw", "square"]);
        for sig in EFFECTS_FUNCTIONS {
            assert_eq!(
                sig.params,
                &[Param {
                    name: "period",
                    ty: ParamType::Duration
                }]
            );
            assert_eq!(sig.return_ty, ParamType::SignalFloat);
        }
    }
}
