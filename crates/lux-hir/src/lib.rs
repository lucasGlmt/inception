//! High-level IR (HIR) for Lux: the AST after name resolution.
//!
//! Depends only on `lux-syntax` (for the AST and spans). Has no notion of
//! types — that's `lux-typeck` — and no dependency on it, keeping the
//! dependency direction downward as required by the workspace layering
//! rules.

pub mod environment;
pub mod error;
pub mod hir;
pub mod ids;
pub mod resolve;

#[cfg(test)]
mod tests;

pub use environment::TargetEnvironment;
pub use error::HirError;
pub use hir::*;
pub use ids::{LocalId, RoleId, SceneId, TargetId};
pub use resolve::lower;
