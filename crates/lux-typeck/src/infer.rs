//! Trusted expression typing for already-checked HIR.
//!
//! This is deliberately separate from [`crate::checker`]: the checker's
//! `infer` produces diagnostics and tolerates partially-typed trees (a
//! local with no known type because its own `let` already failed).
//! [`expr_type`] instead *assumes* the tree is fully well-typed — its
//! only legitimate caller is downstream tooling (namely `lux-mir`) that
//! only ever runs after [`crate::checker::check`] has already returned
//! `Ok`. Under that precondition every sub-expression has a definite
//! type, so the `unreachable!()`s below can never actually trigger; they
//! exist only to document the precondition rather than silently return a
//! wrong type if it's ever violated.

use lux_hir::HirExpr;

use crate::rules::{binary_result_type, literal_type, unary_result_type};
use crate::types::Type;

/// Recomputes the type of `expr`. `local_types` must be indexed by
/// `LocalId` and must assign every local referenced in `expr` a type —
/// i.e. it should come from a [`crate::program::TypedScene`] produced by
/// a successful [`crate::checker::check`] of the same HIR.
pub fn expr_type(local_types: &[Type], expr: &HirExpr) -> Type {
    match expr {
        HirExpr::Literal(lit, _) => literal_type(*lit),
        HirExpr::Local(id, _) => local_types[id.0 as usize],
        HirExpr::Unary { op, operand, .. } => {
            let operand_ty = expr_type(local_types, operand);
            unary_result_type(*op, operand_ty).unwrap_or_else(|| {
                unreachable!(
                    "expr_type: unary operand type should already be valid, \
                     `expr_type` must only be called on HIR that passed `lux_typeck::check`"
                )
            })
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            let lhs_ty = expr_type(local_types, lhs);
            let rhs_ty = expr_type(local_types, rhs);
            binary_result_type(*op, lhs_ty, rhs_ty).unwrap_or_else(|| {
                unreachable!(
                    "expr_type: binary operand types should already be valid, \
                     `expr_type` must only be called on HIR that passed `lux_typeck::check`"
                )
            })
        }
    }
}
