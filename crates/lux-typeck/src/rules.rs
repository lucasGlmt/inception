//! Pure typing rules, shared by two callers with different needs:
//!
//! - [`crate::checker`] uses these to decide *whether* an operation type
//!   checks, turning a `None` into a diagnostic.
//! - [`crate::infer`] uses the exact same rules to recompute the type of
//!   an expression that's already known to be well-typed (because
//!   [`crate::checker::check`] already returned `Ok`), for downstream
//!   consumers like `lux-mir` — without re-deriving or duplicating the
//!   rule tables themselves.
//!
//! Keeping the rules here as pure functions (no diagnostics, no state) is
//! what makes that reuse possible.

use lux_syntax::ast::{BinaryOp, Literal, UnaryOp};

use crate::types::Type;

/// The type of a literal value, ignoring whether its value is in range
/// (e.g. an out-of-range `Intensity` percent is still typed `Intensity`;
/// range validation is a separate, value-level concern — see
/// [`crate::bounds`]).
pub fn literal_type(lit: Literal) -> Type {
    match lit {
        Literal::Bool(_) => Type::Bool,
        Literal::Int(_) => Type::Int,
        Literal::Float(_) => Type::Float,
        Literal::Duration(_) => Type::Duration,
        Literal::Intensity(_) => Type::Intensity,
        Literal::Angle(_) => Type::Angle,
        Literal::Frequency(_) => Type::Frequency,
        Literal::Tempo(_) => Type::Tempo,
        Literal::Color(_) => Type::Color,
    }
}

/// The result type of a unary operation, or `None` if the operand type
/// doesn't support it.
pub fn unary_result_type(op: UnaryOp, operand: Type) -> Option<Type> {
    match op {
        UnaryOp::Neg => match operand {
            Type::Int | Type::Float => Some(operand),
            _ => None,
        },
    }
}

/// The result type of a binary operation, or `None` if the operand
/// combination isn't supported. Deliberately conservative (see
/// `AGENTS.md`): no implicit conversions, only the combinations listed
/// here are valid.
pub fn binary_result_type(op: BinaryOp, lhs: Type, rhs: Type) -> Option<Type> {
    match op {
        BinaryOp::Add | BinaryOp::Sub => match (lhs, rhs) {
            (Type::Int, Type::Int) => Some(Type::Int),
            (Type::Float, Type::Float) => Some(Type::Float),
            (Type::Duration, Type::Duration) => Some(Type::Duration),
            (Type::Intensity, Type::Intensity) => Some(Type::Intensity),
            _ => None,
        },
        BinaryOp::Mul | BinaryOp::Div => match (lhs, rhs) {
            (Type::Int, Type::Int) => Some(Type::Int),
            (Type::Float, Type::Float) => Some(Type::Float),
            _ => None,
        },
    }
}

pub fn binary_op_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
    }
}
