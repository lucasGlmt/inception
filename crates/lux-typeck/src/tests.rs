use lux_hir::TargetEnvironment;

use crate::checker::check;
use crate::error::TypeError;
use crate::program::TypedProgram;
use crate::types::{SequenceElement, Type};

fn check_source(source: &str) -> Result<TypedProgram, Vec<TypeError>> {
    check_source_with_targets(source, &TargetEnvironment::new())
}

fn check_source_with_targets(
    source: &str,
    targets: &TargetEnvironment,
) -> Result<TypedProgram, Vec<TypeError>> {
    let ast = lux_syntax::parse(source).expect("source should parse");
    let hir = lux_hir::lower(&ast, targets).expect("source should resolve");
    check(&hir)
}

#[test]
fn typed_duration_wait_passes() {
    let result = check_source(
        r#"
        scene main {
            let duration: Duration = 1s;
            wait duration;
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn inferred_duration_wait_passes() {
    let result = check_source(
        r#"
        scene main {
            let duration = 1s;
            wait duration;
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn inline_duration_wait_passes() {
    assert!(check_source("scene main { wait 1s; }").is_ok());
}

#[test]
fn wait_requires_duration() {
    let errors = check_source("scene main { wait 50%; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("`wait` expects `Duration`"))
    );
}

#[test]
fn wait_rejects_color() {
    let errors = check_source("scene main { wait red; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("`wait` expects `Duration`"))
    );
}

#[test]
fn annotation_mismatch_is_an_error() {
    let errors = check_source("scene main { let x: Duration = red; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Duration`, found `Color`"))
    );
}

#[test]
fn annotation_mismatch_intensity_vs_duration() {
    let errors = check_source("scene main { let intensity: Intensity = 1s; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Intensity`, found `Duration`"))
    );
}

#[test]
fn intensity_out_of_range_is_an_error() {
    let errors = check_source("scene main { let x: Intensity = 150%; }").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("out of range")));
}

#[test]
fn inferred_intensity_out_of_range_is_an_error_without_annotation() {
    let errors = check_source("scene main { let x = 150%; }").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("out of range")));
}

#[test]
fn out_of_range_intensity_still_reports_later_misuse() {
    // Matches AGENTS.md's worked example: the checker should report BOTH
    // the out-of-range literal AND that `wait` needed a `Duration`.
    let errors = check_source(
        r#"
        scene main {
            let duration: Intensity = 150%;
            wait duration;
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("out of range")));
    assert!(errors.iter().any(|e| {
        e.message
            .contains("`wait` expects `Duration`, found `Intensity`")
    }));
}

#[test]
fn unknown_type_annotation_is_an_error() {
    let errors = check_source("scene main { let x: Fixture = 1s; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown type `Fixture`"))
    );
}

#[test]
fn arithmetic_on_matching_types_passes() {
    assert!(check_source("scene main { let x = 1 + 2; }").is_ok());
    assert!(check_source("scene main { let x = 1.0 + 2.0; }").is_ok());
    assert!(check_source("scene main { let x = 1s + 500ms; }").is_ok());
    assert!(check_source("scene main { let x = 1s - 500ms; }").is_ok());
    assert!(check_source("scene main { let x = 50% + 20%; }").is_ok());
    assert!(check_source("scene main { let x = 50% - 20%; }").is_ok());
    assert!(check_source("scene main { let x = 2 * 4; }").is_ok());
    assert!(check_source("scene main { let x = 8 / 2; }").is_ok());
}

#[test]
fn arithmetic_precedence_is_type_correct() {
    assert!(check_source("scene main { let x = 1 + 2 * 3; }").is_ok());
}

#[test]
fn mismatched_arithmetic_is_rejected() {
    let errors = check_source("scene main { let x = 1s + 50%; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("no implementation of `+`"))
    );
}

#[test]
fn arithmetic_on_colors_is_rejected() {
    let errors = check_source("scene main { let x = red + 1; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("no implementation of `+`"))
    );
}

#[test]
fn negating_a_color_is_rejected() {
    let errors = check_source("scene main { let x = -red; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("cannot negate `Color`"))
    );
}

#[test]
fn negative_int_literal_passes() {
    assert!(check_source("scene main { let x = -12; }").is_ok());
}

#[test]
fn typed_program_records_local_types() {
    let typed = check_source(
        r#"
        scene main {
            let duration: Duration = 1s;
            let intensity = 50%;
        }
        "#,
    )
    .expect("should type check");
    assert_eq!(typed.scenes.len(), 1);
    assert_eq!(
        typed.scenes[0].local_types,
        vec![Type::Duration, Type::Intensity]
    );
}

#[test]
fn expr_type_matches_literal_and_binary_inference() {
    let ast = lux_syntax::parse(
        r#"
        scene main {
            let a = 1s;
            let b = 500ms;
            let duration = a + b;
        }
        "#,
    )
    .expect("should parse");
    let hir = lux_hir::lower(&ast, &TargetEnvironment::new()).expect("should resolve");
    let typed = check(&hir).expect("should type check");

    let scene = &hir.scenes[0];
    let local_types = &typed.scenes[0].local_types;

    let lux_hir::HirStatement::Let(duration_let) = &scene.statements[2] else {
        panic!("expected let statement");
    };
    assert_eq!(
        crate::expr_type(local_types, &duration_let.value),
        Type::Duration
    );
}

fn washes_environment() -> TargetEnvironment {
    let mut targets = TargetEnvironment::new();
    targets.insert("Washes");
    targets
}

#[test]
fn intensity_attribute_assignment_passes() {
    let result = check_source_with_targets(
        "scene main { Washes.intensity = 50%; }",
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn attribute_assignment_rejects_wrong_type() {
    let errors = check_source_with_targets(
        "scene main { Washes.intensity = red; }",
        &washes_environment(),
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Intensity`, found `Color`"))
    );
}

#[test]
fn attribute_assignment_rejects_duration() {
    let errors = check_source_with_targets(
        "scene main { Washes.intensity = 2s; }",
        &washes_environment(),
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Intensity`, found `Duration`"))
    );
}

#[test]
fn unknown_attribute_name_is_an_error() {
    let errors =
        check_source_with_targets("scene main { Washes.pan = 50%; }", &washes_environment())
            .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown attribute `pan`"))
    );
}

#[test]
fn color_attribute_assignment_passes() {
    let result =
        check_source_with_targets("scene main { Washes.color = red; }", &washes_environment());
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn intensity_transition_with_duration_passes() {
    let result = check_source_with_targets(
        "scene main { Washes.intensity -> 100% over 2s; }",
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn transition_checks_value_and_duration_independently() {
    let errors = check_source_with_targets(
        "scene main { Washes.intensity -> red over 50%; }",
        &washes_environment(),
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Intensity`, found `Color`"))
    );
    assert!(errors.iter().any(|e| {
        e.message
            .contains("transition duration expects `Duration`, found `Intensity`")
    }));
}

#[test]
fn transition_rejects_duration_as_intensity_value() {
    let errors = check_source_with_targets(
        "scene main { Washes.intensity -> 1s over 2s; }",
        &washes_environment(),
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Intensity`, found `Duration`"))
    );
}

#[test]
fn color_transition_is_explicitly_out_of_scope_for_v1() {
    let errors = check_source_with_targets(
        "scene main { Washes.color -> red over 2s; }",
        &washes_environment(),
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("not supported yet"))
    );
}

#[test]
fn role_rejects_attribute_not_declared_by_its_contract() {
    let errors = check_source(
        r#"
        rig contract DemoRig {
            role Dimmers: Group<Intensity>;
        }
        scene main { Dimmers.color = red; }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("role does not provide required capability `Color`")
    }));
}

#[test]
fn math_sin_call_is_well_typed() {
    let result = check_source("import std.Math; scene main { let x: Float = Math.sin(90deg); }");
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn color_rgb_call_is_well_typed() {
    let result =
        check_source("import std.Color; scene main { let x: Color = Color.rgb(255, 120, 20); }");
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn math_abs_resolves_int_and_float_overloads() {
    let result = check_source(
        r#"
        import std.Math;
        scene main {
            let a: Int = Math.abs(-5);
            let b: Float = Math.abs(-2.5);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn math_sin_rejects_color_argument() {
    let errors =
        check_source("import std.Math; scene main { let x = Math.sin(red); }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Angle`, found `Color`"))
    );
}

#[test]
fn color_rgb_rejects_wrong_argument_type() {
    let errors = check_source("import std.Color; scene main { let x = Color.rgb(255, 0, 2s); }")
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Int`, found `Duration`"))
    );
}

#[test]
fn math_clamp_rejects_mismatched_overload() {
    let errors = check_source("import std.Math; scene main { let x = Math.clamp(1.0, red, 5.0); }")
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("no matching overload of `Math.clamp`"))
    );
}

#[test]
fn call_arity_mismatch_is_reported() {
    let errors = check_source("import std.Math; scene main { let x = Math.sin(); }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected 1 argument, found 0"))
    );

    let errors =
        check_source("import std.Math; scene main { let x = Math.sin(1deg, 2deg); }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected 1 argument, found 2"))
    );
}

#[test]
fn color_rgb_out_of_range_literal_is_a_compile_time_error() {
    let errors = check_source("import std.Color; scene main { let x = Color.rgb(255, 300, 0); }")
        .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("color channel `300` is out of range 0..=255")
    }));
}

#[test]
fn math_clamp_min_greater_than_max_literal_is_a_compile_time_error() {
    let errors =
        check_source("import std.Math; scene main { let x = Math.clamp(1, 5, 2); }").unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("clamp's min (5) is greater than its max (2)")
    }));
}

#[test]
fn math_clamp_variants_type_check() {
    let result = check_source(
        r#"
        import std.Math;
        scene main {
            let below = Math.clamp(-5, 0, 10);
            let inside = Math.clamp(5, 0, 10);
            let above = Math.clamp(15, 0, 10);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn math_lerp_variants_type_check() {
    let result = check_source(
        r#"
        import std.Math;
        scene main {
            let a = Math.lerp(0.0, 10.0, 0.0);
            let b = Math.lerp(0.0, 10.0, 0.5);
            let c = Math.lerp(0.0, 10.0, 1.0);
            let d = Math.lerp(0.0, 10.0, 1.5);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn color_mix_and_hsv_type_check() {
    let result = check_source(
        r#"
        import std.Color;
        scene main {
            let a = Color.mix(red, blue, 0.5);
            let b = Color.hsv(180deg, 100%, 100%);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn signal_constant_infers_signal_of_argument_type() {
    let typed = check_source(
        r#"
        import std.Signal;
        scene main {
            let a = Signal.constant(50%);
            let b = Signal.constant(red);
            let c = Signal.constant(1.5);
            let d = Signal.constant(90deg);
        }
        "#,
    )
    .expect("should type check");
    assert_eq!(
        typed.scenes[0].local_types,
        vec![
            Type::Signal(crate::types::SignalElement::Intensity),
            Type::Signal(crate::types::SignalElement::Color),
            Type::Signal(crate::types::SignalElement::Float),
            Type::Signal(crate::types::SignalElement::Angle),
        ]
    );
}

#[test]
fn signal_types_are_distinguished_by_element_type() {
    assert_eq!(
        Type::Signal(crate::types::SignalElement::Intensity),
        Type::Signal(crate::types::SignalElement::Intensity)
    );
    assert_ne!(
        Type::Signal(crate::types::SignalElement::Intensity),
        Type::Signal(crate::types::SignalElement::Color)
    );
    assert_ne!(
        Type::Signal(crate::types::SignalElement::Intensity),
        Type::Intensity
    );
}

#[test]
fn signal_type_annotation_matches_inferred_value() {
    let result = check_source(
        r#"
        import std.Signal;
        scene main {
            let s: Signal<Intensity> = Signal.constant(50%);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn signal_type_annotation_mismatch_is_an_error() {
    let errors = check_source(
        r#"
        import std.Signal;
        scene main {
            let s: Signal<Color> = Signal.constant(50%);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Color>`, found `Signal<Intensity>`")
    }));
}

#[test]
fn signal_value_does_not_implicitly_unwrap() {
    let errors = check_source(
        r#"
        import std.Signal;
        scene main {
            let x: Intensity = Signal.constant(50%);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Intensity`, found `Signal<Intensity>`")
    }));
}

#[test]
fn plain_value_does_not_implicitly_wrap_into_a_signal() {
    let errors = check_source("scene main { let s: Signal<Intensity> = 50%; }").unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Intensity>`, found `Intensity`")
    }));
}

#[test]
fn nested_signal_annotation_is_rejected() {
    let errors = check_source("scene main { let s: Signal<Signal<Intensity>> = 1; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("Signal<Signal<...>>"))
    );
}

#[test]
fn signal_element_type_not_in_the_supported_set_is_rejected() {
    let errors = check_source("scene main { let s: Signal<Bool> = 1; }").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("Signal<Bool>")));
}

#[test]
fn non_generic_type_with_type_argument_is_rejected() {
    let errors = check_source("scene main { let s: Intensity<Color> = 50%; }").unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("does not take type arguments"))
    );
}

#[test]
fn signal_as_stdlib_argument_is_not_a_matching_overload() {
    let errors = check_source(
        r#"
        import std.Math;
        import std.Signal;
        scene main {
            let s = Signal.constant(1.0);
            let x = Math.abs(s);
        }
        "#,
    )
    .unwrap_err();
    assert!(!errors.is_empty());
}

#[test]
fn signal_binding_with_a_local_of_the_matching_signal_type_passes() {
    let result = check_source_with_targets(
        r#"
        import std.Signal;
        scene main {
            let level = Signal.constant(50%);
            Washes.intensity <- level;
        }
        "#,
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn signal_binding_with_an_inline_constant_passes() {
    let result = check_source_with_targets(
        r#"
        import std.Signal;
        scene main {
            Washes.intensity <- Signal.constant(50%);
        }
        "#,
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn signal_binding_rejects_a_direct_value_with_no_implicit_wrap() {
    let errors = check_source_with_targets(
        "scene main { Washes.intensity <- 50%; }",
        &washes_environment(),
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Intensity>`, found `Intensity`")
    }));
}

#[test]
fn signal_binding_rejects_a_signal_of_the_wrong_element_type() {
    let errors = check_source_with_targets(
        r#"
        import std.Signal;
        scene main {
            let color = Signal.constant(red);
            Washes.intensity <- color;
        }
        "#,
        &washes_environment(),
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Intensity>`, found `Signal<Color>`")
    }));
}

#[test]
fn signal_binding_on_an_attribute_the_role_lacks_capability_for_is_rejected() {
    let errors = check_source(
        r#"
        import std.Signal;
        rig contract DemoRig {
            role Dimmers: Group<Intensity>;
        }
        scene main {
            Dimmers.color <- Signal.constant(red);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("role does not provide required capability `Color`")
    }));
}

#[test]
fn signal_binding_then_transition_type_checks() {
    let result = check_source_with_targets(
        r#"
        import std.Signal;
        scene main {
            let dimmed = Signal.constant(20%);
            Washes.intensity <- dimmed;
            wait 2s;
            Washes.intensity -> 100% over 1s;
        }
        "#,
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

// --- std.Effects -----------------------------------------------------

#[test]
fn effects_sine_call_infers_signal_float() {
    let result = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn effects_explicit_signal_float_annotation_passes() {
    let result = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave: Signal<Float> = Effects.sine(2s);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

/// Item 33.
#[test]
fn effects_wrong_annotation_is_rejected() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave: Signal<Intensity> = Effects.sine(2s);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Intensity>`, found `Signal<Float>`")
    }));
}

/// Item 4: a non-`Duration` argument is rejected with a `Duration`
/// diagnostic, for all four oscillators.
#[test]
fn effects_oscillators_require_a_duration_argument() {
    for (call, found) in [
        ("Effects.sine(50%)", "Intensity"),
        ("Effects.triangle(red)", "Color"),
        ("Effects.saw(90deg)", "Angle"),
        ("Effects.square(1.5)", "Float"),
    ] {
        let source = format!(
            r#"
            import std.Effects;
            scene main {{
                let wave = {call};
            }}
            "#
        );
        let errors = check_source(&source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.message == format!("expected `Duration`, found `{found}`")),
            "call {call} did not produce the expected diagnostic: {errors:?}"
        );
    }
}

/// Item 5: a literal `0s` period is rejected at compile time, for all
/// four oscillators.
#[test]
fn effects_zero_literal_period_is_a_compile_time_error() {
    for call in [
        "Effects.sine(0s)",
        "Effects.triangle(0s)",
        "Effects.saw(0s)",
        "Effects.square(0s)",
    ] {
        let source = format!(
            r#"
            import std.Effects;
            scene main {{
                let wave = {call};
            }}
            "#
        );
        let errors = check_source(&source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("period must be greater than zero")),
            "call {call} did not produce the expected diagnostic: {errors:?}"
        );
    }
}

/// A non-zero literal period is fine, of course.
#[test]
fn effects_nonzero_literal_period_passes() {
    let result = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(500ms);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

/// Item 29: `Signal<Float>` can never bind directly to `Intensity` (or
/// `Color`) — this is deliberate, see the task brief's ".range() is a
/// future milestone" rationale.
#[test]
fn effects_signal_float_cannot_bind_to_intensity() {
    let errors = check_source_with_targets(
        r#"
        import std.Effects;
        scene main {
            Washes.intensity <- Effects.sine(2s);
        }
        "#,
        &washes_environment(),
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Intensity>`, found `Signal<Float>`")
    }));
}

#[test]
fn effects_signal_float_cannot_bind_to_color() {
    let errors = check_source_with_targets(
        r#"
        import std.Effects;
        scene main {
            Washes.color <- Effects.saw(2s);
        }
        "#,
        &washes_environment(),
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Color>`, found `Signal<Float>`")
    }));
}

/// All four oscillators are independently wired into the registry, not
/// just `sine`.
#[test]
fn every_effects_oscillator_type_checks() {
    for name in ["sine", "triangle", "saw", "square"] {
        let source = format!(
            r#"
            import std.Effects;
            scene main {{
                let wave: Signal<Float> = Effects.{name}(1s);
            }}
            "#
        );
        let result = check_source(&source);
        assert!(result.is_ok(), "{name}: unexpected errors: {result:?}");
    }
}

// --- Signal composition: .range()/.phase()/.invert() ------------------

#[test]
fn range_infers_the_target_element_type() {
    for (bounds, expected) in [
        ("0.0, 10.0", "Signal<Float>"),
        ("0%, 100%", "Signal<Intensity>"),
        ("0deg, 180deg", "Signal<Angle>"),
    ] {
        let source = format!(
            r#"
            import std.Effects;
            scene main {{
                let wave = Effects.sine(2s).range({bounds});
            }}
            "#
        );
        let hir = {
            let ast = lux_syntax::parse(&source).expect("source should parse");
            lux_hir::lower(&ast, &TargetEnvironment::new()).expect("source should resolve")
        };
        let typed = check(&hir).unwrap_or_else(|e| panic!("{bounds}: unexpected errors: {e:?}"));
        assert_eq!(typed.scenes[0].local_types[0].to_string(), expected);
    }
}

#[test]
fn range_bounds_must_have_the_same_type() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).range(0%, 180deg);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("range bounds must have the same type: found `Intensity` and `Angle`")
    }));
}

#[test]
fn range_does_not_support_color() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).range(red, blue);
        }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("range does not support `Color`"))
    );
}

#[test]
fn range_is_only_available_on_signal_float() {
    let errors = check_source(
        r#"
        import std.Signal;
        scene main {
            let level: Signal<Intensity> = Signal.constant(50%);
            let bad = level.range(0%, 100%);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("no method `range` on type `Signal<Intensity>`")
    }));
}

#[test]
fn range_wrong_arity_is_reported() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).range(0%);
        }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected 2 arguments, found 1"))
    );
}

#[test]
fn phase_on_a_direct_oscillator_passes() {
    for name in ["sine", "triangle", "saw", "square"] {
        let source = format!(
            r#"
            import std.Effects;
            scene main {{
                let wave = Effects.{name}(2s).phase(90deg);
            }}
            "#
        );
        let result = check_source(&source);
        assert!(result.is_ok(), "{name}: unexpected errors: {result:?}");
    }
}

#[test]
fn phase_chained_on_another_phase_passes() {
    let result = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).phase(90deg).phase(90deg);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn phase_on_a_constant_signal_is_rejected() {
    let errors = check_source(
        r#"
        import std.Signal;
        scene main {
            let level = Signal.constant(1.5);
            let wave = level.phase(90deg);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("can only be applied directly to an `Effects` oscillator")
    }));
}

#[test]
fn phase_after_range_is_rejected() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).range(0.0, 1.0).phase(90deg);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("can only be applied directly to an `Effects` oscillator")
    }));
}

#[test]
fn spread_on_a_direct_oscillator_passes() {
    for name in ["sine", "triangle", "saw", "square"] {
        let source = format!(
            r#"
            import std.Effects;
            scene main {{
                let wave = Effects.{name}(2s).spread(360deg);
            }}
            "#
        );
        let result = check_source(&source);
        assert!(result.is_ok(), "{name}: unexpected errors: {result:?}");
    }
}

#[test]
fn spread_chained_on_phase_passes() {
    let result = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).phase(45deg).spread(360deg);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn spread_then_range_passes() {
    let result = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).spread(360deg).range(5%, 100%);
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn spread_on_a_constant_signal_is_rejected() {
    let errors = check_source(
        r#"
        import std.Signal;
        scene main {
            let level = Signal.constant(1.5);
            let wave = level.spread(360deg);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("can only be applied directly to an `Effects` oscillator")
    }));
}

#[test]
fn spread_after_range_is_rejected() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).range(0.0, 1.0).spread(360deg);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("can only be applied directly to an `Effects` oscillator")
    }));
}

#[test]
fn spread_chained_on_another_spread_is_rejected() {
    let errors = check_source(
        r#"
        import std.Effects;
        scene main {
            let wave = Effects.sine(2s).spread(180deg).spread(180deg);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("can only be applied directly to an `Effects` oscillator")
    }));
}

#[test]
fn signal_binding_with_spread_and_range_passes() {
    let result = check_source_with_targets(
        r#"
        import std.Effects;
        scene main {
            Washes.intensity <- Effects.sine(2s).spread(360deg).range(5%, 100%);
        }
        "#,
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn invert_passes_and_returns_signal_float() {
    let hir = {
        let source = r#"
            import std.Effects;
            scene main {
                let wave = Effects.sine(2s).invert();
            }
        "#;
        let ast = lux_syntax::parse(source).expect("source should parse");
        lux_hir::lower(&ast, &TargetEnvironment::new()).expect("source should resolve")
    };
    let typed = check(&hir).expect("unexpected errors");
    assert_eq!(typed.scenes[0].local_types[0].to_string(), "Signal<Float>");
}

#[test]
fn fluent_composition_infers_the_final_element_type() {
    let hir = {
        let source = r#"
            import std.Effects;
            scene main {
                let breathe =
                    Effects.sine(2s)
                        .phase(90deg)
                        .range(10%, 100%);
            }
        "#;
        let ast = lux_syntax::parse(source).expect("source should parse");
        lux_hir::lower(&ast, &TargetEnvironment::new()).expect("source should resolve")
    };
    let typed = check(&hir).expect("unexpected errors");
    assert_eq!(
        typed.scenes[0].local_types[0].to_string(),
        "Signal<Intensity>"
    );
}

#[test]
fn signal_binding_with_range_to_intensity_passes() {
    let result = check_source_with_targets(
        r#"
        import std.Effects;
        scene main {
            Washes.intensity <- Effects.sine(2s).range(5%, 100%);
        }
        "#,
        &washes_environment(),
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn signal_binding_with_range_to_angle_is_rejected() {
    let errors = check_source_with_targets(
        r#"
        import std.Effects;
        scene main {
            Washes.intensity <- Effects.sine(2s).range(0deg, 180deg);
        }
        "#,
        &washes_environment(),
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Signal<Intensity>`, found `Signal<Angle>`")
    }));
}

// --- Sequence<T> --------------------------------------------------------

#[test]
fn sequence_of_infers_the_element_type() {
    for (values, expected) in [
        ("red, blue", "Sequence<Color>"),
        ("10%, 20%, 30%", "Sequence<Intensity>"),
        ("1.0, 2.0, 3.0", "Sequence<Float>"),
        ("0deg, 90deg", "Sequence<Angle>"),
        ("1, 2, 3", "Sequence<Int>"),
    ] {
        let source = format!(
            r#"
            import std.Sequence;
            scene main {{
                let a = Sequence.of({values});
            }}
            "#
        );
        let typed =
            check_source(&source).unwrap_or_else(|e| panic!("{values}: unexpected errors: {e:?}"));
        assert_eq!(typed.scenes[0].local_types[0].to_string(), expected);
    }
}

#[test]
fn sequence_of_mixed_types_is_rejected() {
    let errors = check_source(
        r#"
        import std.Sequence;
        scene main {
            let bad = Sequence.of(red, 50%);
        }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("expected `Color`, found `Intensity`"))
    );
}

#[test]
fn sequence_annotation_mismatch_is_rejected() {
    let errors = check_source(
        r#"
        import std.Sequence;
        scene main {
            let x: Sequence<Color> = Sequence.of(10%, 20%);
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Sequence<Color>`, found `Sequence<Intensity>`")
    }));
}

#[test]
fn empty_sequence_of_is_rejected() {
    let errors = check_source(
        r#"
        import std.Sequence;
        scene main {
            let bad = Sequence.of();
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("cannot infer element type of empty Sequence")
    }));
}

#[test]
fn sequence_length_returns_int() {
    let result = check_source(
        r#"
        import std.Sequence;
        scene main {
            let s = Sequence.of(red, blue, white);
            let n = s.length();
        }
        "#,
    );
    let typed = result.unwrap_or_else(|e| panic!("unexpected errors: {e:?}"));
    assert_eq!(typed.scenes[0].local_types[1], Type::Int);
}

#[test]
fn sequence_length_on_non_sequence_is_rejected() {
    let errors = check_source(
        r#"
        scene main {
            let x = 5;
            let bad = x.length();
        }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("no method `length`"))
    );
}

#[test]
fn sequence_indexing_returns_element_type() {
    let result = check_source(
        r#"
        import std.Sequence;
        scene main {
            let s = Sequence.of(red, blue, white);
            let first: Color = s[0];
        }
        "#,
    );
    assert!(result.is_ok(), "unexpected errors: {result:?}");
}

#[test]
fn sequence_index_with_non_int_is_rejected() {
    let errors = check_source(
        r#"
        import std.Sequence;
        scene main {
            let s = Sequence.of(red, blue);
            let bad = s[50%];
        }
        "#,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message
            .contains("expected `Int` index, found `Intensity`")
    }));
}

#[test]
fn indexing_a_non_sequence_is_rejected() {
    let errors = check_source(
        r#"
        scene main {
            let x = 5;
            let bad = x[0];
        }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("cannot index into type `Int`"))
    );
}

#[test]
fn nested_sequence_annotation_is_rejected() {
    let errors = check_source(
        r#"
        import std.Sequence;
        scene main {
            let bad: Sequence<Sequence<Color>> = Sequence.of(red, blue);
        }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("Sequence<Sequence<...>>"))
    );
}

#[test]
fn sequence_element_from_type_matches_registered_types() {
    for elem in SequenceElement::ALL.iter().copied() {
        assert_eq!(SequenceElement::from_type(elem.as_type()), Some(elem));
    }
}
