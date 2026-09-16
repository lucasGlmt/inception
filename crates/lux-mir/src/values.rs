//! Conversions from frontend literal values (as validated by
//! `lux-typeck`) to MIR's domain-appropriate internal representations.
//!
//! Every conversion here is saturating/clamping rather than panicking:
//! `lux-typeck` guarantees a `Literal::Intensity` is in `0..=100` and
//! that `Literal::Duration`/`Angle`/`Frequency`/`Tempo` are non-negative
//! (unary `-` is only valid on `Int`/`Float`, see
//! `lux_typeck::rules::unary_result_type`), but it doesn't bound their
//! upper magnitude — an absurd literal like `999999999999999s` is
//! syntactically and semantically valid Lux today. Saturating here keeps
//! that a non-issue instead of an internal panic, per `AGENTS.md`'s "no
//! panics on user input" rule.

use lux_syntax::ast::{ColorLiteral, ColorName, Literal};

use crate::mir::{ColorValue, MirConstant};

pub fn lower_literal(lit: Literal) -> MirConstant {
    match lit {
        Literal::Bool(b) => MirConstant::Bool(b),
        Literal::Int(i) => MirConstant::Int(i),
        Literal::Float(f) => MirConstant::Float(f),
        Literal::Duration(ms) => MirConstant::Duration(duration_ms_to_ns(ms)),
        Literal::Intensity(percent) => MirConstant::Intensity(intensity_percent_to_u16(percent)),
        Literal::Angle(degrees) => MirConstant::Angle(angle_degrees_to_millidegrees(degrees)),
        Literal::Frequency(hz) => MirConstant::Frequency(saturate_u32(hz)),
        Literal::Tempo(bpm) => MirConstant::Tempo(saturate_u32(bpm)),
        Literal::Color(color) => MirConstant::Color(lower_color(color)),
    }
}

/// Milliseconds (as canonicalized by `lux-syntax`) to nanoseconds.
pub fn duration_ms_to_ns(ms: i64) -> u64 {
    (ms.max(0) as u64).saturating_mul(1_000_000)
}

/// Whole percent (`0..=100` once type-checked) to `0..=65535`.
pub fn intensity_percent_to_u16(percent: i64) -> u16 {
    let clamped = percent.clamp(0, 100) as u32;
    ((clamped * u16::MAX as u32) / 100) as u16
}

/// Whole degrees to millidegrees.
pub fn angle_degrees_to_millidegrees(degrees: i64) -> i32 {
    degrees
        .saturating_mul(1000)
        .clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn saturate_u32(value: i64) -> u32 {
    value.clamp(0, u32::MAX as i64) as u32
}

fn lower_color(color: ColorLiteral) -> ColorValue {
    match color {
        ColorLiteral::Hex(r, g, b) => ColorValue { r, g, b },
        ColorLiteral::Named(name) => named_color_rgb(name),
    }
}

fn named_color_rgb(name: ColorName) -> ColorValue {
    match name {
        ColorName::Red => ColorValue { r: 255, g: 0, b: 0 },
        ColorName::Blue => ColorValue { r: 0, g: 0, b: 255 },
        ColorName::Green => ColorValue { r: 0, g: 255, b: 0 },
        ColorName::White => ColorValue {
            r: 255,
            g: 255,
            b: 255,
        },
        ColorName::Black => ColorValue { r: 0, g: 0, b: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_converts_ms_to_ns() {
        assert_eq!(duration_ms_to_ns(1000), 1_000_000_000);
        assert_eq!(duration_ms_to_ns(500), 500_000_000);
    }

    #[test]
    fn intensity_scales_percent_to_u16_range() {
        assert_eq!(intensity_percent_to_u16(0), 0);
        assert_eq!(intensity_percent_to_u16(100), u16::MAX);
        assert_eq!(intensity_percent_to_u16(50), 32767);
    }

    #[test]
    fn angle_converts_degrees_to_millidegrees() {
        assert_eq!(angle_degrees_to_millidegrees(45), 45_000);
    }

    #[test]
    fn pathological_literals_saturate_instead_of_panicking() {
        // These would overflow their target type with a naive
        // multiply/cast; none of them should panic.
        assert_eq!(duration_ms_to_ns(i64::MAX), u64::MAX);
        assert_eq!(angle_degrees_to_millidegrees(i64::MAX), i32::MAX);
        assert_eq!(angle_degrees_to_millidegrees(i64::MIN), i32::MIN);
        assert_eq!(saturate_u32(i64::MAX), u32::MAX);
        assert_eq!(saturate_u32(i64::MIN), 0);
    }

    #[test]
    fn named_colors_map_to_expected_rgb() {
        assert_eq!(
            named_color_rgb(ColorName::Red),
            ColorValue { r: 255, g: 0, b: 0 }
        );
        assert_eq!(
            named_color_rgb(ColorName::Black),
            ColorValue { r: 0, g: 0, b: 0 }
        );
    }
}
