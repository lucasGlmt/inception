//! Type checker for Lux.
//!
//! Depends on `lux-hir` (for the resolved tree) and `lux-syntax` (for
//! spans and literal values); owns the set of builtin types and all
//! typing rules, per the workspace's "single source of truth" rule for
//! compiler semantics.
//!
//! On success, [`check`] returns a [`TypedProgram`] carrying every
//! local's resolved type. `lux-mir` consumes this (together with the
//! checked HIR) to lower into MIR without re-running type inference;
//! [`infer::expr_type`] is the trusted, non-diagnostic entry point it
//! uses for that.

pub mod attribute;
pub mod bounds;
pub mod checker;
pub mod error;
pub mod expected;
pub mod infer;
pub mod program;
pub mod rules;
pub mod types;

#[cfg(test)]
mod tests;

pub use attribute::Attribute;
pub use checker::check;
pub use error::TypeError;
pub use expected::ExpectedType;
pub use infer::expr_type;
pub use program::{TypedProgram, TypedScene};
pub use rules::literal_type;
pub use types::Type;
