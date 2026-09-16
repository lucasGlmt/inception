//! High-level IR: the AST after name resolution.
//!
//! Literal values are reused directly from `lux_syntax::ast` ([`Literal`],
//! [`ColorLiteral`] etc.) rather than redefined here — a literal's value
//! doesn't change during lowering, only how names are referenced does, so
//! duplicating that type would just be churn. What *does* change: every
//! [`Expression::Identifier`](lux_syntax::ast::Expression::Identifier) in
//! the AST becomes a resolved [`HirExpr::Local`] here, carrying a
//! [`LocalId`] instead of a name.

use lux_syntax::Span;
use lux_syntax::ast::{BinaryOp, Literal, UnaryOp};

use crate::ids::{LocalId, SceneId, TargetId};

#[derive(Debug, Clone, PartialEq)]
pub struct HirFile {
    pub scenes: Vec<HirScene>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirScene {
    pub id: SceneId,
    pub name: String,
    pub name_span: Span,
    /// Every local declared in this scene, in declaration order.
    pub locals: Vec<LocalDecl>,
    pub statements: Vec<HirStatement>,
    pub span: Span,
}

impl HirScene {
    pub fn local(&self, id: LocalId) -> &LocalDecl {
        &self.locals[id.0 as usize]
    }
}

/// A `let` binding's declaration site. Keeps the original name only for
/// diagnostics/debugging/tooling; semantic references use [`LocalId`].
#[derive(Debug, Clone, PartialEq)]
pub struct LocalDecl {
    pub id: LocalId,
    pub name: String,
    pub span: Span,
    pub is_mut: bool,
    pub type_annotation: Option<TypeAnnotation>,
}

/// A type name written in source, not yet checked against the set of
/// known types (that happens in `lux-typeck`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAnnotation {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirStatement {
    Let(HirLet),
    Wait(HirWait),
    Expression(HirExprStatement),
    Assign(HirAssign),
}

impl HirStatement {
    pub fn span(&self) -> Span {
        match self {
            HirStatement::Let(s) => s.span,
            HirStatement::Wait(s) => s.span,
            HirStatement::Expression(s) => s.span,
            HirStatement::Assign(s) => s.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirLet {
    pub local: LocalId,
    pub value: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirWait {
    pub value: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirExprStatement {
    pub value: HirExpr,
    pub span: Span,
}

/// `<target>.<attribute> = <value>;`, resolved. `target` is a real
/// [`TargetId`] (validated against a
/// [`crate::environment::TargetEnvironment`] during lowering), but
/// `attribute_name` stays a raw name — like [`TypeAnnotation`], whether
/// it names a real attribute (and what type it expects) is `lux-typeck`'s
/// call, not this crate's.
#[derive(Debug, Clone, PartialEq)]
pub struct HirAssign {
    pub target: TargetId,
    pub attribute_name: String,
    pub attribute_span: Span,
    pub value: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Literal(Literal, Span),
    /// A resolved reference to a local variable.
    Local(LocalId, Span),
    Unary {
        op: UnaryOp,
        operand: Box<HirExpr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        span: Span,
    },
}

impl HirExpr {
    pub fn span(&self) -> Span {
        match self {
            HirExpr::Literal(_, span) => *span,
            HirExpr::Local(_, span) => *span,
            HirExpr::Unary { span, .. } => *span,
            HirExpr::Binary { span, .. } => *span,
        }
    }
}
