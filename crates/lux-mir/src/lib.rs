//! MIR (mid-level IR) for Lux: explicit, typed, stack-oriented operations
//! lowered from checked HIR, plus the MIR → bytecode codegen pass.
//!
//! Depends on `lux-typeck` (for `Type` and the checked program's local
//! types) and on `lux-bytecode` (codegen's target) — matching the
//! workspace's dependency direction:
//! `lux-syntax -> lux-hir -> lux-typeck -> lux-mir -> lux-bytecode`.

pub mod codegen;
pub mod ids;
pub mod lower;
pub mod mir;
pub mod values;

#[cfg(test)]
mod tests;

pub use codegen::lower_to_bytecode;
pub use ids::{BlockId, FunctionId, LocalId};
pub use lower::lower;
pub use mir::*;
