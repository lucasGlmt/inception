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

/// A resolved lighting target (e.g. what `Washes` refers to in
/// `Washes.intensity = 50%;`), file-scoped rather than scene-scoped: a
/// target name means the same thing everywhere in a program. See
/// [`crate::environment::TargetEnvironment`] for how a name becomes one
/// of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoleId(pub u32);
