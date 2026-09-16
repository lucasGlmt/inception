//! Type checker for Lux.
//!
//! Depends on `lux-hir` (for the resolved tree) and `lux-syntax` (for
//! spans and literal values); owns the set of builtin types and all
//! typing rules, per the workspace's "single source of truth" rule for
//! compiler semantics.

pub mod bounds;
pub mod checker;
pub mod error;
pub mod types;

#[cfg(test)]
mod tests;

pub use checker::check;
pub use error::TypeError;
pub use types::Type;
