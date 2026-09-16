//! Target-name environment used during HIR resolution.
//!
//! Portable compilation populates this deterministically from `rig contract`
//! roles. The public structure remains available for legacy/compiler unit tests
//! that exercise isolated target resolution without a contract.

use crate::ids::TargetId;

#[derive(Debug, Clone, Default)]
pub struct TargetEnvironment {
    /// Index = `TargetId`; insertion order decides the ID, so it's
    /// entirely caller-controlled and deterministic.
    names: Vec<String>,
}

impl TargetEnvironment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a new target name, returning its freshly assigned
    /// [`TargetId`]. Declaring the same name twice creates two distinct
    /// IDs (name resolution below always finds the *first* one) — this
    /// mirrors how a future rig/linker would treat a duplicate
    /// definition, without this crate needing to detect and diagnose it
    /// itself.
    pub fn insert(&mut self, name: impl Into<String>) -> TargetId {
        let id = TargetId(self.names.len() as u32);
        self.names.push(name.into());
        id
    }

    pub fn resolve(&self, name: &str) -> Option<TargetId> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| TargetId(i as u32))
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_declared_names() {
        let mut env = TargetEnvironment::new();
        let washes = env.insert("Washes");
        let backs = env.insert("Backs");

        assert_eq!(env.resolve("Washes"), Some(washes));
        assert_eq!(env.resolve("Backs"), Some(backs));
        assert_eq!(env.resolve("Unknown"), None);
    }

    #[test]
    fn ids_are_assigned_in_insertion_order() {
        let mut env = TargetEnvironment::new();
        assert_eq!(env.insert("A"), TargetId(0));
        assert_eq!(env.insert("B"), TargetId(1));
        assert_eq!(env.len(), 2);
    }
}
