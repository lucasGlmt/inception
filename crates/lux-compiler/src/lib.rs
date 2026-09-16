//! Facade orchestrating the Lux frontend pipeline:
//! `source -> parse -> lower HIR -> resolve -> type check`.
//!
//! This is the only crate downstream tools (CLI, LSP, Studio) should
//! depend on for compiler behavior, per the workspace's "single source of
//! truth" rule — it doesn't add any semantics of its own, only wires the
//! stages together and normalizes their diagnostics into one type.
//!
//! MIR and bytecode generation are out of scope for this milestone: a
//! successfully checked program is represented here purely by its
//! validated HIR.

pub mod diagnostic;

pub use diagnostic::{Diagnostic, Stage};
pub use lux_hir::HirFile;

/// The result of successfully checking a Lux program: its fully resolved
/// and type-checked HIR.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedProgram {
    pub hir: HirFile,
}

/// Runs the full frontend pipeline over `source`. On success, every scene
/// has been parsed, every name resolved and every expression type
/// checked. On failure, returns every diagnostic collected by whichever
/// stage failed first — parsing and resolution stages don't proceed to
/// the next stage once they've failed, since a later stage can't
/// meaningfully run over a tree it knows is malformed.
pub fn check(source: &str) -> Result<CheckedProgram, Vec<Diagnostic>> {
    let ast = lux_syntax::parse(source).map_err(diagnostic::from_syntax_errors)?;
    let hir = lux_hir::lower(&ast).map_err(diagnostic::from_hir_errors)?;
    lux_typeck::check(&hir).map_err(diagnostic::from_type_errors)?;
    Ok(CheckedProgram { hir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_scene_compiles() {
        let source = r#"
            scene main {
                wait 1s;
            }
        "#;
        assert!(check(source).is_ok());
    }

    #[test]
    fn typed_duration_compiles() {
        let source = r#"
            scene main {
                let duration: Duration = 1s;
                wait duration;
            }
        "#;
        check(source).expect("should type check");
    }

    #[test]
    fn intensity_annotation_mismatch_reports_multiple_diagnostics() {
        let source = r#"
            scene main {
                let duration: Intensity = 150%;
                wait duration;
            }
        "#;
        let diagnostics = check(source).expect_err("should fail type checking");
        assert!(diagnostics.iter().all(|d| d.stage == Stage::Type));
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("out of range"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("`wait` expects `Duration`"))
        );
    }

    #[test]
    fn syntax_errors_short_circuit_before_resolution() {
        let source = "scene main { wait; }";
        let diagnostics = check(source).expect_err("should fail parsing");
        assert!(diagnostics.iter().all(|d| d.stage == Stage::Syntax));
    }

    #[test]
    fn unknown_name_is_a_resolve_diagnostic() {
        let source = "scene main { wait missing; }";
        let diagnostics = check(source).expect_err("should fail resolution");
        assert!(diagnostics.iter().all(|d| d.stage == Stage::Resolve));
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("unknown name `missing`"))
        );
    }
}
