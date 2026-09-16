//! Typed identifiers used to reference HIR items without going through
//! textual names. Downstream stages (typeck, and eventually MIR/linker)
//! should compare/index by these IDs, never by [`String`] name.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SceneId(pub u32);

/// A local variable within a single scene. IDs are scene-scoped: they
/// restart at `0` for every scene, since locals never cross scene
/// boundaries in this language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u32);
