//! Executes a resolved stdlib intrinsic call.
//!
//! Every function here is **total**: no `Result`, no panics, no runtime
//! failure path. The bytecode verifier (`lux_bytecode::verify`) already
//! guarantees a `CallIntrinsic`'s argument count and types are correct
//! before this code ever runs (see `lux-bytecode/src/verify.rs`), and
//! every `std.Math`/`std.Color` function is pure and defined for every
//! input in its parameter types — there is nothing left that could fail
//! at this point. Where a *value* could be considered "misused" (e.g.
//! `Math.clamp` with `min > max`, or an out-of-range `Color.rgb` channel
//! that isn't a compile-time constant), the behavior is a documented,
//! deterministic value rather than an error — see each function below.
//!
//! Dispatch is by [`lux_bytecode::IntrinsicId`] only, via an exhaustive
//! match with no wildcard arm: adding a new intrinsic that this file
//! doesn't yet implement is a compile error here, not a silent gap.
//!
//! The 5 `SignalConstant*` intrinsics, plus the 4 `Effects*` oscillator
//! constructors, are the exception to "every function here is total":
//! constructing a signal needs to insert into the executing `Vm`'s
//! `crate::signal::SignalStore`, which this function has no access to
//! (and shouldn't — every other intrinsic here is a pure computation over
//! its arguments alone, and mixing in `Vm`-owned mutable state would
//! break that for all of them). The `Effects*` constructors additionally
//! need `clock.now()` for their oscillator's time origin — another thing
//! this pure function has no access to; the 5 `EffectsStep*` constructors
//! need both, for the same two reasons. `Vm::exec_call_intrinsic`
//! recognizes those ids and handles them itself, the same way it
//! already handles `SetAttribute`/`TransitionAttribute`/`Wait`/`BindSignal`
//! outside this file for the same reason. They're still listed in the
//! match below (each just `unreachable!()`) so this stays a real
//! exhaustive match over `IntrinsicId` — a new intrinsic added to the
//! enum without updating either this file or `Vm::exec_call_intrinsic` is
//! still a compile error here, not a silent gap.

use lux_bytecode::{ColorValue, IntrinsicId};

use crate::value::Value;

pub fn eval_intrinsic(intrinsic: IntrinsicId, args: &[Value]) -> Value {
    match intrinsic {
        IntrinsicId::MathSin => Value::Float(angle_radians(args[0]).sin()),
        IntrinsicId::MathCos => Value::Float(angle_radians(args[0]).cos()),

        // `saturating_abs`, not `abs`: `abs` panics on `i64::MIN` (its
        // magnitude doesn't fit in `i64`), and this function must be
        // total. Saturating to `i64::MAX` is the standard, documented
        // way to keep `abs` defined everywhere.
        IntrinsicId::MathAbsInt => Value::Int(int(args[0]).saturating_abs()),
        IntrinsicId::MathAbsFloat => Value::Float(float(args[0]).abs()),

        IntrinsicId::MathMinInt => Value::Int(int(args[0]).min(int(args[1]))),
        IntrinsicId::MathMinFloat => Value::Float(float(args[0]).min(float(args[1]))),
        IntrinsicId::MathMaxInt => Value::Int(int(args[0]).max(int(args[1]))),
        IntrinsicId::MathMaxFloat => Value::Float(float(args[0]).max(float(args[1]))),

        // Computed as `value.max(min).min(max)`, in that fixed order: if
        // `min > max`, this always returns `max` (a direct, provable
        // consequence of the evaluation order, not a special-cased
        // branch). A compile-time diagnostic catches this when all three
        // arguments are literal constants (see `lux-typeck`); a
        // non-constant misuse just gets this documented value, never a
        // runtime error.
        IntrinsicId::MathClampInt => {
            let (value, min, max) = (int(args[0]), int(args[1]), int(args[2]));
            Value::Int(value.max(min).min(max))
        }
        IntrinsicId::MathClampFloat => {
            let (value, min, max) = (float(args[0]), float(args[1]), float(args[2]));
            Value::Float(value.max(min).min(max))
        }

        // `t` is deliberately not clamped: `t > 1.0` extrapolates past
        // `b`, as documented on `lux_stdlib`'s `Signature` for `lerp`.
        IntrinsicId::MathLerp => {
            let (a, b, t) = (float(args[0]), float(args[1]), float(args[2]));
            Value::Float(a + (b - a) * t)
        }

        IntrinsicId::ColorRgb => {
            let channel = |v: Value| int(v).clamp(0, 255) as u8;
            Value::Color(ColorValue {
                r: channel(args[0]),
                g: channel(args[1]),
                b: channel(args[2]),
            })
        }

        // `ratio` is clamped to `[0.0, 1.0]`, never an error — the same
        // "clamp, never trap on a non-constant value" policy as
        // `Math.clamp`/`Color.rgb` above.
        IntrinsicId::ColorMix => {
            let (a, b, ratio) = (
                color(args[0]),
                color(args[1]),
                float(args[2]).clamp(0.0, 1.0),
            );
            let mix_channel = |a: u8, b: u8| -> u8 {
                (a as f64 + (b as f64 - a as f64) * ratio)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            Value::Color(ColorValue {
                r: mix_channel(a.r, b.r),
                g: mix_channel(a.g, b.g),
                b: mix_channel(a.b, b.b),
            })
        }

        IntrinsicId::ColorHsv => Value::Color(hsv_to_rgb(args[0], args[1], args[2])),

        IntrinsicId::SignalConstantInt
        | IntrinsicId::SignalConstantFloat
        | IntrinsicId::SignalConstantAngle
        | IntrinsicId::SignalConstantIntensity
        | IntrinsicId::SignalConstantColor => unreachable!(
            "eval_intrinsic: SignalConstant* intrinsics are handled by \
             Vm::exec_call_intrinsic directly, which needs mutable access to the \
             SignalStore this pure function doesn't have — see this module's doc"
        ),

        IntrinsicId::EffectsSine
        | IntrinsicId::EffectsTriangle
        | IntrinsicId::EffectsSaw
        | IntrinsicId::EffectsSquare => unreachable!(
            "eval_intrinsic: Effects* intrinsics are handled by Vm::exec_call_intrinsic \
             directly, which needs mutable access to the SignalStore and the clock this \
             pure function doesn't have — see this module's doc"
        ),

        IntrinsicId::SignalRangeFloat
        | IntrinsicId::SignalRangeIntensity
        | IntrinsicId::SignalRangeAngle
        | IntrinsicId::SignalPhase
        | IntrinsicId::SignalSpreadFloat
        | IntrinsicId::SignalSpreadInt
        | IntrinsicId::SignalSpreadAngle
        | IntrinsicId::SignalSpreadIntensity
        | IntrinsicId::SignalSpreadColor
        | IntrinsicId::SignalInvert => unreachable!(
            "eval_intrinsic: signal-transformation intrinsics are handled by \
             Vm::exec_call_intrinsic directly, which needs mutable access to the \
             SignalStore this pure function doesn't have — see this module's doc"
        ),

        IntrinsicId::SequenceOfInt
        | IntrinsicId::SequenceOfFloat
        | IntrinsicId::SequenceOfAngle
        | IntrinsicId::SequenceOfIntensity
        | IntrinsicId::SequenceOfColor => unreachable!(
            "eval_intrinsic: SequenceOf* intrinsics are handled by Vm::exec_call_intrinsic \
             directly, which needs mutable access to the SequenceStore this pure function \
             doesn't have — see this module's doc"
        ),

        IntrinsicId::SequenceLengthInt
        | IntrinsicId::SequenceLengthFloat
        | IntrinsicId::SequenceLengthAngle
        | IntrinsicId::SequenceLengthIntensity
        | IntrinsicId::SequenceLengthColor => unreachable!(
            "eval_intrinsic: SequenceLength* intrinsics are handled by Vm::exec_call_intrinsic \
             directly, which needs (read-only) access to the SequenceStore this pure function \
             doesn't have — see this module's doc"
        ),

        IntrinsicId::EffectsStepInt
        | IntrinsicId::EffectsStepFloat
        | IntrinsicId::EffectsStepAngle
        | IntrinsicId::EffectsStepIntensity
        | IntrinsicId::EffectsStepColor => unreachable!(
            "eval_intrinsic: EffectsStep* intrinsics are handled by Vm::exec_call_intrinsic \
             directly, which needs mutable access to the SignalStore and the clock this pure \
             function doesn't have — see this module's doc"
        ),
    }
}

fn int(value: Value) -> i64 {
    match value {
        Value::Int(v) => v,
        _ => unreachable!("eval_intrinsic: operand type already checked by the verifier"),
    }
}

fn float(value: Value) -> f64 {
    match value {
        Value::Float(v) => v,
        _ => unreachable!("eval_intrinsic: operand type already checked by the verifier"),
    }
}

fn color(value: Value) -> ColorValue {
    match value {
        Value::Color(c) => c,
        _ => unreachable!("eval_intrinsic: operand type already checked by the verifier"),
    }
}

/// An `Angle` operand (millidegrees) as degrees.
fn angle_degrees(value: Value) -> f64 {
    match value {
        Value::Angle(millideg) => millideg as f64 / 1000.0,
        _ => unreachable!("eval_intrinsic: operand type already checked by the verifier"),
    }
}

/// An `Angle` operand (millidegrees) as radians.
fn angle_radians(value: Value) -> f64 {
    angle_degrees(value).to_radians()
}

/// An `Intensity` operand (`0..=65535`) as a `0.0..=1.0` fraction.
fn intensity_fraction(value: Value) -> f64 {
    match value {
        Value::Intensity(raw) => raw as f64 / u16::MAX as f64,
        _ => unreachable!("eval_intrinsic: operand type already checked by the verifier"),
    }
}

/// Standard HSV -> RGB conversion. `hue` wraps modulo 360deg via
/// `rem_euclid`, so any `Angle` value (including negative or >360deg) is
/// handled the same as its canonical `0..360` equivalent.
fn hsv_to_rgb(hue: Value, saturation: Value, value: Value) -> ColorValue {
    let hue_deg = angle_degrees(hue).rem_euclid(360.0);
    let s = intensity_fraction(saturation);
    let v = intensity_fraction(value);

    let c = v * s;
    let h_prime = hue_deg / 60.0;
    let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = match h_prime.floor() as i64 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    let to_channel = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    ColorValue {
        r: to_channel(r1),
        g: to_channel(g1),
        b: to_channel(b1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1e-9;

    fn assert_close(actual: Value, expected: f64) {
        match actual {
            Value::Float(v) => assert!(
                (v - expected).abs() < TOLERANCE,
                "expected {expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn sin_at_key_angles() {
        assert_close(
            eval_intrinsic(IntrinsicId::MathSin, &[Value::Angle(0)]),
            0.0,
        );
        assert_close(
            eval_intrinsic(IntrinsicId::MathSin, &[Value::Angle(90_000)]),
            1.0,
        );
        assert_close(
            eval_intrinsic(IntrinsicId::MathSin, &[Value::Angle(180_000)]),
            0.0,
        );
    }

    #[test]
    fn cos_at_key_angles() {
        assert_close(
            eval_intrinsic(IntrinsicId::MathCos, &[Value::Angle(0)]),
            1.0,
        );
        assert_close(
            eval_intrinsic(IntrinsicId::MathCos, &[Value::Angle(90_000)]),
            0.0,
        );
    }

    #[test]
    fn abs_saturates_instead_of_panicking() {
        assert_eq!(
            eval_intrinsic(IntrinsicId::MathAbsInt, &[Value::Int(i64::MIN)]),
            Value::Int(i64::MAX)
        );
        assert_eq!(
            eval_intrinsic(IntrinsicId::MathAbsInt, &[Value::Int(-5)]),
            Value::Int(5)
        );
    }

    #[test]
    fn clamp_below_inside_above_and_min_greater_than_max() {
        let clamp = |v, min, max| {
            eval_intrinsic(
                IntrinsicId::MathClampInt,
                &[Value::Int(v), Value::Int(min), Value::Int(max)],
            )
        };
        assert_eq!(clamp(-5, 0, 10), Value::Int(0)); // below
        assert_eq!(clamp(5, 0, 10), Value::Int(5)); // inside
        assert_eq!(clamp(15, 0, 10), Value::Int(10)); // above
        assert_eq!(clamp(1, 5, 2), Value::Int(2)); // min > max: documented, returns max
    }

    #[test]
    fn lerp_variants() {
        let lerp = |t| {
            eval_intrinsic(
                IntrinsicId::MathLerp,
                &[Value::Float(0.0), Value::Float(10.0), Value::Float(t)],
            )
        };
        assert_eq!(lerp(0.0), Value::Float(0.0));
        assert_eq!(lerp(0.5), Value::Float(5.0));
        assert_eq!(lerp(1.0), Value::Float(10.0));
        assert_eq!(lerp(1.5), Value::Float(15.0)); // t > 1 extrapolates
    }

    #[test]
    fn rgb_valid_and_saturating() {
        assert_eq!(
            eval_intrinsic(
                IntrinsicId::ColorRgb,
                &[Value::Int(255), Value::Int(120), Value::Int(20)]
            ),
            Value::Color(ColorValue {
                r: 255,
                g: 120,
                b: 20
            })
        );
        // Non-constant out-of-range input saturates rather than trapping.
        assert_eq!(
            eval_intrinsic(
                IntrinsicId::ColorRgb,
                &[Value::Int(300), Value::Int(-10), Value::Int(0)]
            ),
            Value::Color(ColorValue { r: 255, g: 0, b: 0 })
        );
    }

    #[test]
    fn mix_at_zero_half_and_one() {
        let red = Value::Color(ColorValue { r: 255, g: 0, b: 0 });
        let blue = Value::Color(ColorValue { r: 0, g: 0, b: 255 });
        let mix = |ratio| eval_intrinsic(IntrinsicId::ColorMix, &[red, blue, Value::Float(ratio)]);
        assert_eq!(mix(0.0), red);
        assert_eq!(mix(1.0), blue);
        assert_eq!(
            mix(0.5),
            Value::Color(ColorValue {
                r: 128,
                g: 0,
                b: 128
            })
        );
    }

    #[test]
    fn hsv_primaries() {
        let hsv = |deg| {
            eval_intrinsic(
                IntrinsicId::ColorHsv,
                &[
                    Value::Angle(deg * 1000),
                    Value::Intensity(u16::MAX),
                    Value::Intensity(u16::MAX),
                ],
            )
        };
        assert_eq!(hsv(0), Value::Color(ColorValue { r: 255, g: 0, b: 0 }));
        assert_eq!(hsv(120), Value::Color(ColorValue { r: 0, g: 255, b: 0 }));
        assert_eq!(hsv(240), Value::Color(ColorValue { r: 0, g: 0, b: 255 }));
    }
}
