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

use lux_hir::{HirCall, HirCallee, HirExpr};
use lux_stdlib::Signature;

use crate::rules::{binary_result_type, literal_type, unary_result_type};
use crate::stdlib_bridge::{
    from_param_type, sequence_element_param_type, signal_element_param_type, to_param_type,
};
use crate::types::{SequenceElement, SignalElement, Type};

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
        HirExpr::Call(call) => from_param_type(resolve_call(local_types, call).return_ty),
        HirExpr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => from_param_type(resolve_method_call(local_types, receiver, method, args).return_ty),
        HirExpr::Index { receiver, .. } => match expr_type(local_types, receiver) {
            Type::Sequence(elem) => elem.as_type(),
            other => unreachable!(
                "expr_type: Index receiver should already be a `Sequence<T>` (found `{other}`), \
                 `expr_type` must only be called on HIR that passed `lux_typeck::check`"
            ),
        },
    }
}

/// Trusted re-derivation of which stdlib overload `call` resolved to,
/// mirroring `expr_type`'s role for `Unary`/`Binary`: `call` must already
/// have passed `crate::checker::check`, so the resolution below can only
/// ever succeed. `lux-mir` uses this to know exactly which
/// [`lux_stdlib::IntrinsicId`] to lower `call` into, without `lux-typeck`
/// having to thread a resolved signature back through `TypedProgram`.
pub fn resolve_call(local_types: &[Type], call: &HirCall) -> &'static Signature {
    let HirCallee::Std {
        module_path, name, ..
    } = &call.callee;

    // `Sequence.of` bypasses `lux_stdlib::resolve_overload` entirely, the
    // trusted-re-derivation counterpart to
    // `crate::checker::Checker::check_sequence_of` — see that method's
    // docs for why (variadic arity, no fixed `Signature` shape can
    // describe it). The element type is re-derived from the first
    // argument, exactly as `check_sequence_of` originally inferred it.
    if *module_path == ["std", "Sequence"] && *name == "of" {
        let first = call.args.first().unwrap_or_else(|| {
            unreachable!(
                "resolve_call: `Sequence.of()` with no arguments should already have been \
                 rejected by `lux_typeck::check`"
            )
        });
        let elem = SequenceElement::from_type(expr_type(local_types, first)).unwrap_or_else(|| {
            unreachable!(
                "resolve_call: `Sequence.of`'s element type should already be valid, checked by \
                 `lux_typeck::check`"
            )
        });
        return sequence_of_signature(elem);
    }

    let arg_types: Vec<_> = call
        .args
        .iter()
        .map(|arg| to_param_type(expr_type(local_types, arg)))
        .collect();
    lux_stdlib::resolve_overload(module_path, name, &arg_types).unwrap_or_else(|_| {
        unreachable!(
            "resolve_call: `{module_path:?}.{name}` should already be a valid, \
             non-ambiguous call — `resolve_call` must only be called on HIR that \
             passed `lux_typeck::check`"
        )
    })
}

/// The one `SEQUENCE_OF_*` `Signature` matching `elem` — re-derived from
/// `lux_stdlib::candidates(&["std", "Sequence"], "of")` rather than a
/// hand-written match, so this can never drift from the registry's actual
/// 5 overloads.
fn sequence_of_signature(elem: SequenceElement) -> &'static Signature {
    let expected = sequence_element_param_type(elem);
    lux_stdlib::candidates(&["std", "Sequence"], "of")
        .iter()
        .find(|sig| {
            // `SEQUENCE_OF_*`'s `return_ty` is `SequenceInt`/etc., which
            // shares its `elem` 1:1 with `expected` here.
            from_param_type(sig.return_ty) == Type::Sequence(elem)
        })
        .unwrap_or_else(|| {
            unreachable!("sequence_of_signature: no `Sequence.of` overload returns `{expected:?}`")
        })
}

/// Trusted re-derivation of which `Signal<Float>` method `receiver.method(args)`
/// resolved to, mirroring [`resolve_call`]'s role. The receiver's own type
/// isn't re-checked here (that's `check_method_call`'s job, already run) —
/// only its argument types are needed to pick the right overload.
pub fn resolve_method_call(
    local_types: &[Type],
    receiver: &HirExpr,
    method: &str,
    args: &[HirExpr],
) -> &'static Signature {
    let receiver_ty = expr_type(local_types, receiver);
    let arg_types: Vec<_> = args
        .iter()
        .map(|arg| to_param_type(expr_type(local_types, arg)))
        .collect();

    if let Type::Sequence(elem) = receiver_ty {
        return lux_stdlib::resolve_sequence_method(
            sequence_element_param_type(elem),
            method,
            &arg_types,
        )
        .unwrap_or_else(|_| {
            unreachable!(
                "resolve_method_call: `.{method}(...)` on `Sequence<{elem}>` should already be \
                 a valid, non-ambiguous method call — `resolve_method_call` must only be called \
                 on HIR that passed `lux_typeck::check`"
            )
        });
    }

    if let Type::Signal(elem) = receiver_ty
        && elem != SignalElement::Float
    {
        return lux_stdlib::resolve_non_float_signal_method(
            signal_element_param_type(elem),
            method,
            &arg_types,
        )
        .unwrap_or_else(|_| {
            unreachable!(
                "resolve_method_call: `.{method}(...)` on `Signal<{elem}>` should already be a \
                 valid, non-ambiguous method call — `resolve_method_call` must only be called on \
                 HIR that passed `lux_typeck::check`"
            )
        });
    }

    lux_stdlib::resolve_signal_float_method(method, &arg_types).unwrap_or_else(|_| {
        unreachable!(
            "resolve_method_call: `.{method}(...)` should already be a valid, \
             non-ambiguous method call — `resolve_method_call` must only be called on \
             HIR that passed `lux_typeck::check`"
        )
    })
}
