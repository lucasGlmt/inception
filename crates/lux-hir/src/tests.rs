use lux_syntax::ast::Literal;

use crate::hir::{HirExpr, HirStatement};
use crate::ids::LocalId;
use crate::resolve::lower;

fn lower_ok(source: &str) -> crate::hir::HirFile {
    let ast = lux_syntax::parse(source).expect("source should parse");
    lower(&ast).expect("should resolve")
}

fn lower_err(source: &str) -> Vec<crate::error::HirError> {
    let ast = lux_syntax::parse(source).expect("source should parse");
    lower(&ast).expect_err("should fail resolution")
}

#[test]
fn resolves_let_then_wait() {
    let file = lower_ok(
        r#"
        scene main {
            let duration = 1s;
            wait duration;
        }
        "#,
    );
    let scene = &file.scenes[0];
    assert_eq!(scene.locals.len(), 1);
    assert_eq!(scene.locals[0].name, "duration");

    let HirStatement::Wait(wait) = &scene.statements[1] else {
        panic!("expected wait statement");
    };
    assert_eq!(wait.value, HirExpr::Local(LocalId(0), wait.value.span()));
}

#[test]
fn unknown_name_is_a_resolution_error() {
    let errors = lower_err("scene main { wait duration; }");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown name `duration`"))
    );
}

#[test]
fn duplicate_local_is_a_resolution_error() {
    let errors = lower_err(
        r#"
        scene main {
            let x = 1s;
            let x = 2s;
        }
        "#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("duplicate variable `x`"))
    );
}

#[test]
fn duplicate_scene_is_a_resolution_error() {
    let errors = lower_err(
        r#"
        scene main { wait 1s; }
        scene main { wait 2s; }
        "#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("scene `main` is already defined"))
    );
}

#[test]
fn call_to_unknown_function_is_a_resolution_error() {
    let errors = lower_err("scene main { foo(); }");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown function `foo`"))
    );
}

#[test]
fn let_initializer_cannot_reference_itself() {
    let errors = lower_err("scene main { let x = x; }");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown name `x`"))
    );
}

#[test]
fn reports_multiple_unknown_names_in_one_pass() {
    let errors = lower_err("scene main { wait a; wait b; }");
    assert_eq!(errors.len(), 2);
}

#[test]
fn literal_kinds_survive_lowering() {
    let file = lower_ok("scene main { let x = 50%; }");
    let HirStatement::Let(let_stmt) = &file.scenes[0].statements[0] else {
        panic!("expected let statement");
    };
    assert_eq!(
        let_stmt.value,
        HirExpr::Literal(Literal::Intensity(50), let_stmt.value.span())
    );
}
