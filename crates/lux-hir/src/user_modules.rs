//! What `lux-hir` needs to know about *user* (non-`std`) modules to
//! resolve an `import`.
//!
//! `lux-hir` never touches the filesystem — file discovery, parsing and
//! cycle detection for `import show.Helpers;`-style paths all live in
//! `lux-project` (the only crate with filesystem access). This type is
//! the narrow, in-memory interface between the two: "does a qualifier
//! resolve to some real user module file?" Nothing more — in particular,
//! *what* that module exports is not represented here, because today it
//! always exports nothing (Lux has no function-declaration syntax yet;
//! see `resolve.rs`'s handling of `ResolvedImport::User`).
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct UserModuleEnvironment {
    /// The dotted path (e.g. `"show.Helpers"`) of every user module this
    /// file is allowed to import, as resolved ahead of time by
    /// `lux-project`'s file graph.
    resolved_paths: HashSet<String>,
}

impl UserModuleEnvironment {
    /// No user modules available — every non-`std` import fails to
    /// resolve. This is what every single-file compilation path (the
    /// existing `lux_hir::lower`, and every test/compiler entry point
    /// that predates modules) uses.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_paths(paths: impl IntoIterator<Item = String>) -> Self {
        Self {
            resolved_paths: paths.into_iter().collect(),
        }
    }

    pub fn resolves(&self, dotted_path: &str) -> bool {
        self.resolved_paths.contains(dotted_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_resolves_nothing() {
        assert!(!UserModuleEnvironment::empty().resolves("show.Helpers"));
    }

    #[test]
    fn resolves_known_paths_only() {
        let env = UserModuleEnvironment::from_paths(["show.Helpers".to_string()]);
        assert!(env.resolves("show.Helpers"));
        assert!(!env.resolves("show.Other"));
    }
}
