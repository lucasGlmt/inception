//! Typed domain identifiers. Newtypes over `u32`/`u16` rather than bare
//! integers or `String` names — an identity that's a `String` invites
//! runtime name lookups in hot paths, which `AGENTS.md`'s performance
//! section explicitly asks to avoid.

/// A resolved lighting target — a named group of fixtures an
/// `<target>.<attribute> = ...;` assignment addresses as one unit. See
/// [`crate::lighting::ResolvedTarget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetId(pub u32);

/// A single physical fixture, identified numerically. This milestone has
/// no fixture definition language (see `AGENTS.md`'s "hors scope") — a
/// `FixtureId` is just an opaque handle a test or, eventually,
/// `inception-linker` hands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FixtureId(pub u32);

/// A DMX universe, `0`-based here (the renderer's `DmxChannel` newtype is
/// what enforces the `1..=512` *channel* convention — see
/// `inception_renderer`'s docs on that distinction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UniverseId(pub u16);
