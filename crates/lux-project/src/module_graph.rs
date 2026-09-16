//! Resolves `import`s to real files on disk and builds the resulting
//! module dependency graph.
//!
//! **Path convention**: `import show.Helpers;` resolves to
//! `<dir containing source.entry>/show/Helpers.lux` — every segment
//! joined with `/`, `.lux` appended, resolved relative to the entry
//! file's own directory (never the importing file's directory, and never
//! the project root directly). This keeps the convention anchored to the
//! one directory every project already has (`source.entry`, e.g.
//! `src/main.lux`), so `show.Helpers` and `main` naturally share a root.
//! `std.*` imports never touch the filesystem — resolved entirely inside
//! `lux-hir` against `lux_stdlib`'s registry.
//!
//! **Cycle detection**: a standard three-color (white/gray/black) DFS
//! over the file graph. A back-edge to a gray (currently-being-visited)
//! node is a cycle, reported as [`crate::ProjectError::ImportCycle`] with
//! the full cycle's paths — never a stack overflow, since recursion depth
//! is bounded by the number of distinct files in the project (finite and
//! small), not by anything a Lux program's own call graph could control.
//! An already-black (fully processed) node revisited via a different
//! import path is just reused, not re-parsed — so two files importing the
//! same third module is free.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lux_syntax::ast::Item;

use crate::ProjectError;

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleFile {
    pub source: String,
    /// The dotted path (e.g. `"show.Helpers"`) of every non-`std` import
    /// this file itself resolved to another real file.
    pub user_module_paths: Vec<String>,
}

/// Every file discovered starting from `entry`, keyed by absolute path.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleGraph {
    pub entry: PathBuf,
    pub files: HashMap<PathBuf, ModuleFile>,
}

impl ModuleGraph {
    pub fn entry_file(&self) -> &ModuleFile {
        self.files
            .get(&self.entry)
            .expect("entry is always present in `files` after a successful `build`")
    }

    /// Every file *other than* the entry — what `lux-cli`'s file watcher
    /// needs to additionally watch, and what `lux-compiler::ProgramSources`
    /// treats as `modules` rather than the `entry`.
    pub fn other_files(&self) -> impl Iterator<Item = (&PathBuf, &ModuleFile)> {
        self.files.iter().filter(|(path, _)| **path != self.entry)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Gray,
    Black,
}

/// `import show.Helpers;` -> `<base_dir>/show/Helpers.lux`.
fn user_module_path(base_dir: &Path, segments: &[String]) -> PathBuf {
    let mut path = base_dir.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    path.set_extension("lux");
    path
}

/// Builds the full module graph reachable from `entry`, reading every
/// file exactly once. `base_dir` is the directory every non-`std` import
/// path is resolved relative to (see this module's docs) — in practice
/// always `entry`'s own parent directory.
pub fn build(entry: &Path, base_dir: &Path) -> Result<ModuleGraph, ProjectError> {
    let mut files = HashMap::new();
    let mut colors: HashMap<PathBuf, Color> = HashMap::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    visit(entry, base_dir, &mut files, &mut colors, &mut stack)?;
    Ok(ModuleGraph {
        entry: entry.to_path_buf(),
        files,
    })
}

fn visit(
    path: &Path,
    base_dir: &Path,
    files: &mut HashMap<PathBuf, ModuleFile>,
    colors: &mut HashMap<PathBuf, Color>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), ProjectError> {
    match colors.get(path) {
        Some(Color::Black) => return Ok(()),
        Some(Color::Gray) => {
            let mut cycle = stack.clone();
            cycle.push(path.to_path_buf());
            return Err(ProjectError::ImportCycle(cycle));
        }
        None => {}
    }

    colors.insert(path.to_path_buf(), Color::Gray);
    stack.push(path.to_path_buf());

    let source = fs::read_to_string(path).map_err(|source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let (ast, _syntax_errors) = lux_syntax::parse_recovering(&source);
    // Malformed imports are surfaced later as real diagnostics by
    // `lux_compiler::check_program` (this file is also handed to it as a
    // `SourceUnit`) — a tolerant parse here is only used to discover
    // `Item::Import` paths so the graph can keep walking.
    let mut user_module_paths = Vec::new();
    for item in &ast.items {
        let Item::Import(import) = item else { continue };
        if import.path.is_empty() {
            continue;
        }
        let segments: Vec<String> = import.path.iter().map(|id| id.name.clone()).collect();
        if segments[0] == "std" {
            continue; // resolved entirely inside lux-hir, no file involved
        }
        let dotted = segments.join(".");
        let child_path = user_module_path(base_dir, &segments);
        visit(&child_path, base_dir, files, colors, stack)?;
        user_module_paths.push(dotted);
    }

    stack.pop();
    colors.insert(path.to_path_buf(), Color::Black);
    files.insert(
        path.to_path_buf(),
        ModuleFile {
            source,
            user_module_paths,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_file_graph_has_just_the_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.lux");
        fs::write(&entry, "scene main { wait 1s; }").unwrap();

        let graph = build(&entry, dir.path()).unwrap();
        assert_eq!(graph.files.len(), 1);
        assert_eq!(graph.entry_file().user_module_paths, Vec::<String>::new());
    }

    #[test]
    fn resolves_a_user_module_import_to_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.lux");
        fs::write(&entry, "import show.Helpers;\nscene main { wait 1s; }").unwrap();
        fs::create_dir_all(dir.path().join("show")).unwrap();
        fs::write(dir.path().join("show/Helpers.lux"), "scene helper {}").unwrap();

        let graph = build(&entry, dir.path()).unwrap();
        assert_eq!(graph.files.len(), 2);
        assert_eq!(
            graph.entry_file().user_module_paths,
            vec!["show.Helpers".to_string()]
        );
        let others: Vec<_> = graph.other_files().map(|(path, _)| path.clone()).collect();
        assert_eq!(others, vec![dir.path().join("show/Helpers.lux")]);
    }

    #[test]
    fn std_imports_never_touch_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.lux");
        fs::write(
            &entry,
            "import std.Math;\nscene main { let x = Math.sin(0deg); }",
        )
        .unwrap();

        let graph = build(&entry, dir.path()).unwrap();
        assert_eq!(graph.files.len(), 1, "std.Math must not become a file node");
    }

    #[test]
    fn missing_user_module_file_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.lux");
        fs::write(&entry, "import show.Missing;\nscene main {}").unwrap();

        let err = build(&entry, dir.path()).unwrap_err();
        assert!(matches!(err, ProjectError::Io { .. }));
    }

    #[test]
    fn direct_import_cycle_is_detected_without_overflowing_the_stack() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.lux");
        fs::write(&entry, "import show.A;\nscene main {}").unwrap();
        fs::create_dir_all(dir.path().join("show")).unwrap();
        fs::write(dir.path().join("show/A.lux"), "import show.B;\nscene a {}").unwrap();
        fs::write(dir.path().join("show/B.lux"), "import show.A;\nscene b {}").unwrap();

        let err = build(&entry, dir.path()).unwrap_err();
        assert!(matches!(err, ProjectError::ImportCycle(_)));
    }

    #[test]
    fn diamond_import_is_not_a_cycle_and_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.lux");
        fs::write(&entry, "import show.A;\nimport show.B;\nscene main {}").unwrap();
        fs::create_dir_all(dir.path().join("show")).unwrap();
        fs::write(dir.path().join("show/A.lux"), "import show.C;\nscene a {}").unwrap();
        fs::write(dir.path().join("show/B.lux"), "import show.C;\nscene b {}").unwrap();
        fs::write(dir.path().join("show/C.lux"), "scene c {}").unwrap();

        let graph = build(&entry, dir.path()).unwrap();
        assert_eq!(graph.files.len(), 4);
    }
}
