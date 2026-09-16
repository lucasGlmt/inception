use lux_hir::TargetEnvironment;

use crate::checker::check;
use crate::error::TypeError;
use crate::program::TypedProgram;
use crate::types::Type;

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
