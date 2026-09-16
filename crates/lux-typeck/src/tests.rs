use crate::checker::check;
use crate::error::TypeError;

fn check_source(source: &str) -> Result<(), Vec<TypeError>> {
    let ast = lux_syntax::parse(source).expect("source should parse");
    let hir = lux_hir::lower(&ast).expect("source should resolve");
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
