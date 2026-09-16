//! Compile-pass / compile-fail harness.
//!
//! Every `.lux` file under `tests/compiler/pass/` (repo root) must be
//! accepted by `lux_compiler::check`; every file under
//! `tests/compiler/fail/` must be rejected. This is what CI exercises to
//! make sure the frontend's accept/reject behavior doesn't silently
//! regress as the compiler grows.

use std::fs;
use std::path::{Path, PathBuf};

use lux_compiler::TargetEnvironment;

/// Fixtures may reference this target — this stands in for the
/// rig/patch/linker this milestone doesn't have yet (see
/// `lux_compiler::TargetEnvironment`'s docs).
fn fixture_environment() -> TargetEnvironment {
    let mut targets = TargetEnvironment::new();
    targets.insert("Washes");
    targets.insert("Backs");
    targets
}

fn fixtures_dir(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/compiler")
        .join(kind)
}

fn lux_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read fixture directory {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "lux"))
        .collect();
    files.sort();
    files
}

#[test]
fn pass_fixtures_compile_successfully() {
    let dir = fixtures_dir("pass");
    let files = lux_files(&dir);
    assert!(
        !files.is_empty(),
        "expected at least one fixture in {}",
        dir.display()
    );

    for path in files {
        let source = fs::read_to_string(&path).expect("failed to read fixture");
        if let Err(diagnostics) = lux_compiler::check(&source, &fixture_environment()) {
            panic!(
                "expected {} to compile, got diagnostics: {diagnostics:#?}",
                path.display()
            );
        }
    }
}

#[test]
fn fail_fixtures_are_rejected() {
    let dir = fixtures_dir("fail");
    let files = lux_files(&dir);
    assert!(
        !files.is_empty(),
        "expected at least one fixture in {}",
        dir.display()
    );

    for path in files {
        let source = fs::read_to_string(&path).expect("failed to read fixture");
        if lux_compiler::check(&source, &fixture_environment()).is_ok() {
            panic!(
                "expected {} to fail to compile, but it compiled successfully",
                path.display()
            );
        }
    }
}
