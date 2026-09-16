//! Typed IDs used throughout the bytecode format instead of names or bare
//! integers.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstantId(pub u32);

/// A local variable slot within a single function. `u16`-sized: a
/// function is expected to have at most a few dozen locals, and bytecode
/// is a compact, fixed-width format (see `AGENTS.md`'s performance
/// section) — this also matches [`crate::module::Function`]'s
/// `max_stack: u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u16);

/// A resolved lighting target, referenced by `SetAttribute`. Independent
/// of `lux_hir::TargetId` — the same intentional, boundary-preserving
/// duplication already used for `ValueType`/`lux_typeck::Type`: this
/// crate must not depend on the compiler frontend. `lux-mir`'s codegen
/// does the trivial numeric conversion at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetId(pub u32);
