//! Typed IDs for MIR. `LocalId` is reused directly from `lux-hir`: a
//! local's identity doesn't change between HIR and MIR, only what's
//! attached to it (its resolved [`crate::mir::MirLocal::ty`]) does.

pub use lux_hir::LocalId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);
