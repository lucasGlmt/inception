//! The type checker.
//!
//! Walks each scene's already name-resolved HIR and infers/validates a
//! [`Type`] for every expression. Two kinds of failure are kept distinct:
//!
//! - A **type** error (e.g. `wait 50%;`) always stops inference for that
//!   expression (`None`), because there's no sound type to propagate.
//! - A **value** error (e.g. `let x: Intensity = 150%;`, out of range)
//!   does *not* stop inference: the expression's type is still
//!   `Intensity`, only the specific value is invalid. This matters
//!   because it lets a later, unrelated use of `x` still be checked
//!   against its real type instead of silently going unchecked — both
//!   the out-of-range literal *and* a later `wait x;` should be reported.
//!
//! `check` takes `&HirFile` rather than `&mut HirFile`: type information
//! belongs to `lux-typeck` alone (it's the single source of truth for
//! what a type is, see [`crate::types`]), so it isn't written back onto
//! HIR nodes owned by `lux-hir` — doing so would make `lux-hir` need to
//! know about `lux-typeck`'s `Type`, upward through the dependency graph.

use std::collections::HashMap;

use lux_hir::{HirExpr, HirFile, HirLet, HirScene, HirStatement, HirWait, LocalId};
use lux_syntax::Span;
use lux_syntax::ast::{BinaryOp, Literal, UnaryOp};

use crate::bounds::bound_for;
use crate::error::TypeError;
use crate::types::Type;

/// Type-checks every scene in `hir`. Returns every diagnostic collected
/// (not just the first) when checking fails anywhere in the file.
pub fn check(hir: &HirFile) -> Result<(), Vec<TypeError>> {
    let mut checker = Checker { errors: Vec::new() };
    for scene in &hir.scenes {
        checker.check_scene(scene);
    }
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct Checker {
    errors: Vec<TypeError>,
}

impl Checker {
    fn check_scene(&mut self, scene: &HirScene) {
        let mut local_types: HashMap<LocalId, Type> = HashMap::new();
        for stmt in &scene.statements {
            self.check_statement(scene, &mut local_types, stmt);
        }
    }

    fn check_statement(
        &mut self,
        scene: &HirScene,
        local_types: &mut HashMap<LocalId, Type>,
        stmt: &HirStatement,
    ) {
        match stmt {
            HirStatement::Let(let_stmt) => self.check_let(scene, local_types, let_stmt),
            HirStatement::Wait(wait_stmt) => self.check_wait(local_types, wait_stmt),
            HirStatement::Expression(expr_stmt) => {
                self.infer(local_types, &expr_stmt.value);
            }
        }
    }

    fn check_let(
        &mut self,
        scene: &HirScene,
        local_types: &mut HashMap<LocalId, Type>,
        let_stmt: &HirLet,
    ) {
        let inferred = self.infer(local_types, &let_stmt.value);
        let local = scene.local(let_stmt.local);

        let final_type = match &local.type_annotation {
            None => inferred,
            Some(annotation) => {
                let Some(declared_ty) = Type::from_name(&annotation.name) else {
                    self.errors.push(TypeError::new(
                        format!("unknown type `{}`", annotation.name),
                        annotation.span,
                    ));
                    return;
                };
                if let Some(inferred_ty) = inferred
                    && declared_ty != inferred_ty
                {
                    self.errors.push(
                        TypeError::new(
                            format!("expected `{declared_ty}`, found `{inferred_ty}`"),
                            let_stmt.value.span(),
                        )
                        .with_secondary_span(annotation.span)
                        .with_help(format!(
                            "`{}` is declared as `{declared_ty}` here",
                            local.name
                        )),
                    );
                }
                // Trust the annotation going forward even on a mismatch or
                // when the value itself failed to type: it's the clearest
                // signal of intent, and lets later uses of this local
                // still be checked instead of silently skipped.
                Some(declared_ty)
            }
        };

        if let Some(ty) = final_type {
            local_types.insert(let_stmt.local, ty);
        }
    }

    fn check_wait(&mut self, local_types: &mut HashMap<LocalId, Type>, wait_stmt: &HirWait) {
        if let Some(ty) = self.infer(local_types, &wait_stmt.value)
            && ty != Type::Duration
        {
            self.errors.push(TypeError::new(
                format!("`wait` expects `Duration`, found `{ty}`"),
                wait_stmt.value.span(),
            ));
        }
    }

    fn infer(&mut self, local_types: &HashMap<LocalId, Type>, expr: &HirExpr) -> Option<Type> {
        match expr {
            HirExpr::Literal(lit, span) => Some(self.check_literal(*lit, *span)),
            // No error here on a miss: the local's own `let` already
            // reported why it has no type, so staying silent avoids
            // reporting the same root cause twice.
            HirExpr::Local(id, _span) => local_types.get(id).copied(),
            HirExpr::Unary { op, operand, span } => {
                let operand_ty = self.infer(local_types, operand)?;
                self.check_unary(*op, operand_ty, *span)
            }
            HirExpr::Binary { op, lhs, rhs, span } => {
                let lhs_ty = self.infer(local_types, lhs);
                let rhs_ty = self.infer(local_types, rhs);
                match (lhs_ty, rhs_ty) {
                    (Some(l), Some(r)) => self.check_binary(*op, l, r, *span),
                    _ => None,
                }
            }
        }
    }

    fn check_literal(&mut self, lit: Literal, span: Span) -> Type {
        match lit {
            Literal::Bool(_) => Type::Bool,
            Literal::Int(_) => Type::Int,
            Literal::Float(_) => Type::Float,
            Literal::Duration(_) => Type::Duration,
            Literal::Angle(_) => Type::Angle,
            Literal::Frequency(_) => Type::Frequency,
            Literal::Tempo(_) => Type::Tempo,
            Literal::Color(_) => Type::Color,
            Literal::Intensity(value) => {
                if let Some(bound) = bound_for(Type::Intensity)
                    && !bound.contains(value)
                {
                    self.errors.push(TypeError::new(
                        format!(
                            "intensity `{value}%` is out of range {}..={}%",
                            bound.min, bound.max
                        ),
                        span,
                    ));
                }
                Type::Intensity
            }
        }
    }

    fn check_unary(&mut self, op: UnaryOp, operand: Type, span: Span) -> Option<Type> {
        match op {
            UnaryOp::Neg => match operand {
                Type::Int | Type::Float => Some(operand),
                other => {
                    self.errors
                        .push(TypeError::new(format!("cannot negate `{other}`"), span));
                    None
                }
            },
        }
    }

    fn check_binary(&mut self, op: BinaryOp, lhs: Type, rhs: Type, span: Span) -> Option<Type> {
        let result = match op {
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
        };

        if result.is_none() {
            self.errors.push(TypeError::new(
                format!(
                    "no implementation of `{}` for `{lhs}` and `{rhs}`",
                    binary_op_symbol(op)
                ),
                span,
            ));
        }
        result
    }
}

fn binary_op_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
    }
}
