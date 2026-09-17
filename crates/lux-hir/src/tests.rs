use lux_syntax::ast::Literal;

use crate::environment::TargetEnvironment;
use crate::hir::{HirCallee, HirExpr, HirStatement};
use crate::ids::LocalId;
use crate::resolve::{lower, lower_with_modules};
use crate::user_modules::UserModuleEnvironment;

fn lower_ok(source: &str) -> crate::hir::HirFile {
    lower_ok_with(source, &TargetEnvironment::new())
}

fn lower_ok_with(source: &str, targets: &TargetEnvironment) -> crate::hir::HirFile {
    let ast = lux_syntax::parse(source).expect("source should parse");
    lower(&ast, targets).expect("should resolve")
}

fn lower_err(source: &str) -> Vec<crate::error::HirError> {
    lower_err_with(source, &TargetEnvironment::new())
}

fn lower_err_with(source: &str, targets: &TargetEnvironment) -> Vec<crate::error::HirError> {
    let ast = lux_syntax::parse(source).expect("source should parse");
    lower(&ast, targets).expect_err("should fail resolution")
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

#[test]
fn resolves_declared_target_in_assignment() {
    let mut targets = TargetEnvironment::new();
    let washes = targets.insert("Washes");

    let file = lower_ok_with("scene main { Washes.intensity = 50%; }", &targets);

    let HirStatement::Assign(assign) = &file.scenes[0].statements[0] else {
        panic!("expected assign statement");
    };
    assert_eq!(assign.target, washes);
    assert_eq!(assign.attribute_name, "intensity");
}

#[test]
fn unknown_target_is_a_resolution_error() {
    let errors = lower_err("scene main { Washes.intensity = 50%; }");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown target `Washes`"))
    );
}

#[test]
fn assignment_value_is_still_resolved_against_locals() {
    let mut targets = TargetEnvironment::new();
    targets.insert("Washes");

    let file = lower_ok_with(
        r#"
        scene main {
            let level = 50%;
            Washes.intensity = level;
        }
        "#,
        &targets,
    );
    let HirStatement::Assign(assign) = &file.scenes[0].statements[1] else {
        panic!("expected assign statement");
    };
    assert_eq!(
        assign.value,
        HirExpr::Local(LocalId(0), assign.value.span())
    );
}

#[test]
fn resolves_call_to_imported_std_function() {
    let file = lower_ok("import std.Math; scene main { let x = Math.sin(90deg); }");
    let HirStatement::Let(let_stmt) = &file.scenes[0].statements[0] else {
        panic!("expected let statement");
    };
    let HirExpr::Call(call) = &let_stmt.value else {
        panic!("expected call expression, got {:?}", let_stmt.value);
    };
    let HirCallee::Std {
        module_path, name, ..
    } = &call.callee;
    assert_eq!(*module_path, &["std", "Math"]);
    assert_eq!(name, "sin");
    assert_eq!(call.args.len(), 1);
}

#[test]
fn unknown_std_module_is_a_resolution_error() {
    let errors = lower_err("import std.Foo; scene main {}");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown module `std.Foo`"))
    );
}

#[test]
fn unknown_member_on_std_module_is_a_resolution_error() {
    let errors = lower_err("import std.Math; scene main { let x = Math.bogus(1); }");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("module `Math` has no member `bogus`"))
    );
}

#[test]
fn unknown_member_suggests_closest_name() {
    let errors = lower_err("import std.Math; scene main { let x = Math.sn(90deg); }");
    assert!(
        errors
            .iter()
            .any(|e| e.help.as_deref() == Some("did you mean `sin`?"))
    );
}

#[test]
fn qualifier_used_without_import_is_a_resolution_error() {
    let errors = lower_err("scene main { let x = Math.sin(90deg); }");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("module `Math` is not imported"))
    );
}

#[test]
fn duplicate_import_of_same_module_is_not_an_error() {
    let file = lower_ok("import std.Math; import std.Math; scene main { let x = Math.sin(0deg); }");
    assert_eq!(file.scenes[0].locals.len(), 1);
}

#[test]
fn user_module_import_resolves_but_has_no_members() {
    let ast = lux_syntax::parse("import show.Helpers; scene main { let x = Helpers.foo(1); }")
        .expect("should parse");
    let user_modules = UserModuleEnvironment::from_paths(["show.Helpers".to_string()]);
    let errors = lower_with_modules(&ast, &TargetEnvironment::new(), &user_modules)
        .expect_err("should fail resolution");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("module `Helpers` has no member `foo`"))
    );
}

#[test]
fn unresolved_user_module_is_a_resolution_error() {
    let errors = lower_err("import show.Helpers; scene main {}");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown module `show.Helpers`"))
    );
}

#[test]
fn resolves_sequence_of_call() {
    let file = lower_ok(
        r#"
        import std.Sequence;
        scene main {
            let s = Sequence.of(1, 2, 3);
        }
        "#,
    );
    let scene = &file.scenes[0];
    match &scene.statements[0] {
        HirStatement::Let(let_stmt) => match &let_stmt.value {
            HirExpr::Call(call) => {
                let HirCallee::Std {
                    module_path, name, ..
                } = &call.callee;
                assert_eq!(*module_path, &["std", "Sequence"]);
                assert_eq!(name, "of");
                assert_eq!(call.args.len(), 3);
            }
            other => panic!("expected a Call, got {other:?}"),
        },
        other => panic!("expected a Let statement, got {other:?}"),
    }
}

#[test]
fn resolves_index_expression() {
    let file = lower_ok(
        r#"
        import std.Sequence;
        scene main {
            let s = Sequence.of(1, 2);
            let first = s[0];
        }
        "#,
    );
    let scene = &file.scenes[0];
    match &scene.statements[1] {
        HirStatement::Let(let_stmt) => match &let_stmt.value {
            HirExpr::Index {
                receiver, index, ..
            } => {
                assert!(matches!(**receiver, HirExpr::Local(LocalId(0), _)));
                assert!(matches!(**index, HirExpr::Literal(Literal::Int(0), _)));
            }
            other => panic!("expected an Index expression, got {other:?}"),
        },
        other => panic!("expected a Let statement, got {other:?}"),
    }
}

#[test]
fn resolves_sequence_length_method_call() {
    let file = lower_ok(
        r#"
        import std.Sequence;
        scene main {
            let s = Sequence.of(1, 2);
            let n = s.length();
        }
        "#,
    );
    let scene = &file.scenes[0];
    match &scene.statements[1] {
        HirStatement::Let(let_stmt) => match &let_stmt.value {
            HirExpr::MethodCall { method, args, .. } => {
                assert_eq!(method, "length");
                assert!(args.is_empty());
            }
            other => panic!("expected a MethodCall, got {other:?}"),
        },
        other => panic!("expected a Let statement, got {other:?}"),
    }
}
