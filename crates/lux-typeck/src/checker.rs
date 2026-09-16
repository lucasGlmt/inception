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
//! `check` takes `&HirFile`: type information belongs to `lux-typeck`
//! alone (it's the single source of truth for what a type is, see
//! [`crate::types`]), so it isn't written back onto HIR nodes owned by
//! `lux-hir` — doing so would make `lux-hir` need to know about
//! `lux-typeck`'s `Type`, upward through the dependency graph. Instead,
//! on success, `check` returns a [`TypedProgram`] carrying every local's
//! resolved type, for `lux-mir` to consume.

use std::collections::HashMap;

use lux_hir::{
    HirAssign, HirExpr, HirFile, HirLet, HirScene, HirStatement, HirTransition, HirWait, LocalId,
};
use lux_syntax::Span;
use lux_syntax::ast::{BinaryOp, Literal, UnaryOp};

use crate::attribute::Attribute;
use crate::bounds::bound_for;
use crate::error::TypeError;
use crate::program::{TypedProgram, TypedScene};
use crate::rules::{binary_op_symbol, binary_result_type, unary_result_type};
use crate::types::Type;

/// Type-checks every scene in `hir`. Returns every diagnostic collected
/// (not just the first) when checking fails anywhere in the file; on
/// success, returns every local's resolved type.
pub fn check(hir: &HirFile) -> Result<TypedProgram, Vec<TypeError>> {
    let role_capabilities = hir
        .rig_contract
        .iter()
        .flat_map(|contract| &contract.roles)
        .map(|role| (role.target, role.capabilities))
        .collect();
    let mut checker = Checker {
        errors: Vec::new(),
        role_capabilities,
    };

    let scene_locals: Vec<HashMap<LocalId, Type>> = hir
        .scenes
        .iter()
        .map(|scene| checker.check_scene(scene))
        .collect();

    if !checker.errors.is_empty() {
        return Err(checker.errors);
    }

    // Every local is guaranteed present here: `infer` only ever returns
    // `None` for a local reference after an error was already recorded
    // for that local's own `let` (see `infer`'s `HirExpr::Local` arm), so
    // an empty `errors` implies every `let` reached the `Some` arm of
    // `check_let` and was inserted below.
    let scenes = hir
        .scenes
        .iter()
        .zip(scene_locals)
        .map(|(scene, local_types)| {
            let dense = (0..scene.locals.len() as u32)
                .map(|i| {
                    *local_types
                        .get(&LocalId(i))
                        .expect("internal invariant violated: every local should have a type when `check` reports no errors")
                })
                .collect();
            TypedScene { local_types: dense }
        })
        .collect();

    Ok(TypedProgram { scenes })
}

struct Checker {
    errors: Vec<TypeError>,
    role_capabilities: HashMap<lux_hir::TargetId, lux_hir::CapabilitySet>,
}

impl Checker {
    fn check_scene(&mut self, scene: &HirScene) -> HashMap<LocalId, Type> {
        let mut local_types: HashMap<LocalId, Type> = HashMap::new();
        for stmt in &scene.statements {
            self.check_statement(scene, &mut local_types, stmt);
        }
        local_types
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
            HirStatement::Assign(assign) => self.check_assign(local_types, assign),
            HirStatement::Transition(transition) => self.check_transition(local_types, transition),
        }
    }

    fn check_transition(
        &mut self,
        local_types: &HashMap<LocalId, Type>,
        transition: &HirTransition,
    ) {
        // Infer independently so an invalid value does not hide an invalid
        // duration (and vice versa).
        let value_ty = self.infer(local_types, &transition.value);
        let duration_ty = self.infer(local_types, &transition.duration);

        let Some(attribute) = Attribute::from_name(&transition.attribute_name) else {
            self.errors.push(TypeError::new(
                format!("unknown attribute `{}`", transition.attribute_name),
                transition.attribute_span,
            ));
            return;
        };
        self.check_role_capability(transition.target, attribute, transition.attribute_span);

        if attribute != Attribute::Intensity {
            self.errors.push(
                TypeError::new(
                    format!("transitions for `{attribute}` are not supported yet"),
                    transition.attribute_span,
                )
                .with_help("V1 transitions support `Intensity` only"),
            );
        }

        let expected = attribute.value_type();
        if let Some(found) = value_ty
            && found != expected
        {
            self.errors.push(
                TypeError::new(
                    format!("expected `{expected}`, found `{found}`"),
                    transition.value.span(),
                )
                .with_secondary_span(transition.attribute_span)
                .with_help(format!("`{attribute}` expects `{expected}`")),
            );
        }

        if let Some(found) = duration_ty
            && found != Type::Duration
        {
            self.errors.push(TypeError::new(
                format!("transition duration expects `Duration`, found `{found}`"),
                transition.duration.span(),
            ));
        }
    }

    fn check_role_capability(
        &mut self,
        target: lux_hir::TargetId,
        attribute: Attribute,
        span: Span,
    ) {
        let required = attribute.required_capability();
        if let Some(capabilities) = self.role_capabilities.get(&target)
            && !capabilities.contains(required)
        {
            self.errors.push(TypeError::new(
                format!("role does not provide required capability `{required:?}`"),
                span,
            ));
        }
    }

    fn check_assign(&mut self, local_types: &HashMap<LocalId, Type>, assign: &HirAssign) {
        let inferred = self.infer(local_types, &assign.value);

        let Some(attribute) = Attribute::from_name(&assign.attribute_name) else {
            self.errors.push(TypeError::new(
                format!("unknown attribute `{}`", assign.attribute_name),
                assign.attribute_span,
            ));
            return;
        };
        self.check_role_capability(assign.target, attribute, assign.attribute_span);

        let expected = attribute.value_type();
        if let Some(inferred_ty) = inferred
            && inferred_ty != expected
        {
            self.errors.push(
                TypeError::new(
                    format!("expected `{expected}`, found `{inferred_ty}`"),
                    assign.value.span(),
                )
                .with_secondary_span(assign.attribute_span)
                .with_help(format!("`{attribute}` expects `{expected}`")),
            );
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
        let result = unary_result_type(op, operand);
        if result.is_none() {
            self.errors
                .push(TypeError::new(format!("cannot negate `{operand}`"), span));
        }
        result
    }

    fn check_binary(&mut self, op: BinaryOp, lhs: Type, rhs: Type, span: Span) -> Option<Type> {
        let result = binary_result_type(op, lhs, rhs);
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
