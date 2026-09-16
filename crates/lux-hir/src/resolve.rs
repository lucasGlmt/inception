//! Lowers an AST into HIR while resolving names.
//!
//! Lowering and resolution happen in a single pass: by design, HIR never
//! represents an unresolved local reference (see [`crate::hir`]), so a
//! [`HirExpr::Local`] can only be built once its [`LocalId`] is already
//! known.
//!
//! ## Function calls
//!
//! This milestone defines no functions — builtin or user — so there is no
//! function namespace to resolve calls against. Every call expression is
//! therefore reported as an `unknown function` error; syntax support for
//! calls exists (see `lux-syntax`) purely so call *syntax* doesn't need to
//! be redesigned once functions are introduced. A call's arguments are
//! still visited so unrelated errors inside them are also reported.
//!
//! ## Error recovery
//!
//! Like the parser, resolution tries to surface more than one diagnostic
//! per file: an unresolved sub-expression makes its parent unresolved too
//! (represented as `None`), but sibling statements and sibling operands
//! are still visited.

use std::collections::HashMap;

use lux_syntax::Span;
use lux_syntax::ast::{self, Expression, Item, SourceFile, Statement};

use crate::environment::TargetEnvironment;
use crate::error::HirError;
use crate::hir::{
    Capability, CapabilitySet, HirAssign, HirExpr, HirExprStatement, HirFile, HirLet,
    HirRigContract, HirRole, HirScene, HirStatement, HirTransition, HirWait, LocalDecl,
    RoleCardinality, TypeAnnotation,
};
use crate::ids::{LocalId, RoleId, SceneId};

/// Lowers a parsed source file into HIR, resolving every name reference —
/// both local variables and, per `targets`, lighting target names (see
/// [`TargetEnvironment`]'s docs for why that's a parameter here rather
/// than something this crate resolves on its own). Returns every
/// diagnostic collected (never just the first) when resolution fails
/// anywhere in the file.
pub fn lower(ast: &SourceFile, targets: &TargetEnvironment) -> Result<HirFile, Vec<HirError>> {
    let mut resolved_targets = targets.clone();
    let mut pre_errors = Vec::new();
    let rig_contract = lower_rig_contract(ast, &mut resolved_targets, &mut pre_errors);
    let mut lowering = Lowering {
        errors: pre_errors,
        targets: &resolved_targets,
    };
    let mut scenes = Vec::new();
    let mut scene_names: HashMap<String, Span> = HashMap::new();

    for item in &ast.items {
        let Item::Scene(decl) = item else {
            continue;
        };

        if let Some(&first_span) = scene_names.get(&decl.name.name) {
            lowering.errors.push(
                HirError::new(
                    format!("scene `{}` is already defined", decl.name.name),
                    decl.name.span,
                )
                .with_secondary_span(first_span)
                .with_help("rename one of the two scenes"),
            );
        } else {
            scene_names.insert(decl.name.name.clone(), decl.name.span);
        }

        scenes.push(lowering.lower_scene(SceneId(scenes.len() as u32), decl));
    }

    if lowering.errors.is_empty() {
        Ok(HirFile {
            scenes,
            rig_contract,
            target_count: resolved_targets.len() as u32,
        })
    } else {
        Err(lowering.errors)
    }
}

fn lower_rig_contract(
    ast: &SourceFile,
    targets: &mut TargetEnvironment,
    errors: &mut Vec<HirError>,
) -> Option<HirRigContract> {
    let contracts: Vec<_> = ast
        .items
        .iter()
        .filter_map(|item| match item {
            Item::RigContract(contract) => Some(contract),
            Item::Scene(_) => None,
        })
        .collect();
    if contracts.len() > 1 {
        for contract in &contracts[1..] {
            errors.push(HirError::new(
                "only one rig contract is allowed per Lux program",
                contract.span,
            ));
        }
    }
    let contract = contracts.first()?;
    let mut names = HashMap::new();
    let mut roles = Vec::new();
    for declaration in &contract.roles {
        if let Some(first_span) = names.insert(declaration.name.name.clone(), declaration.name.span)
        {
            errors.push(
                HirError::new(
                    format!("role `{}` is already defined", declaration.name.name),
                    declaration.name.span,
                )
                .with_secondary_span(first_span),
            );
            continue;
        }
        let mut capabilities = CapabilitySet::empty();
        for capability in &declaration.capabilities {
            match capability.name.as_str() {
                "Intensity" => capabilities.insert(Capability::Intensity),
                "Color" => capabilities.insert(Capability::Color),
                _ => errors.push(HirError::new(
                    format!("unknown capability `{}`", capability.name),
                    capability.span,
                )),
            }
        }
        let id = RoleId(roles.len() as u32);
        let target = targets.insert(declaration.name.name.clone());
        roles.push(HirRole {
            id,
            target,
            name: declaration.name.name.clone(),
            capabilities,
            cardinality: RoleCardinality::GroupNonEmpty,
        });
    }
    Some(HirRigContract {
        name: contract.name.name.clone(),
        roles,
    })
}

/// A flat, scene-local namespace mapping variable names to their
/// [`LocalId`]. The language has no nested blocks or shadowing scopes in
/// this milestone, so a single flat map is sufficient.
#[derive(Default)]
struct Scope {
    locals: HashMap<String, LocalId>,
}

impl Scope {
    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.locals.get(name).copied()
    }

    fn is_declared(&self, name: &str) -> bool {
        self.locals.contains_key(name)
    }

    fn declare(&mut self, name: String, id: LocalId) {
        self.locals.insert(name, id);
    }
}

struct Lowering<'a> {
    errors: Vec<HirError>,
    targets: &'a TargetEnvironment,
}

impl Lowering<'_> {
    fn lower_scene(&mut self, id: SceneId, decl: &ast::SceneDecl) -> HirScene {
        let mut scope = Scope::default();
        let mut locals = Vec::new();
        let mut statements = Vec::new();

        for stmt in &decl.body.statements {
            if let Some(hir_stmt) = self.lower_statement(&mut scope, &mut locals, stmt) {
                statements.push(hir_stmt);
            }
        }

        HirScene {
            id,
            name: decl.name.name.clone(),
            name_span: decl.name.span,
            locals,
            statements,
            span: decl.span,
        }
    }

    fn lower_statement(
        &mut self,
        scope: &mut Scope,
        locals: &mut Vec<LocalDecl>,
        stmt: &Statement,
    ) -> Option<HirStatement> {
        match stmt {
            Statement::Let(let_stmt) => self
                .lower_let(scope, locals, let_stmt)
                .map(HirStatement::Let),
            Statement::Wait(wait_stmt) => self.lower_wait(scope, wait_stmt).map(HirStatement::Wait),
            Statement::Expression(expr_stmt) => self
                .lower_expr_statement(scope, expr_stmt)
                .map(HirStatement::Expression),
            Statement::Assign(assign) => self.lower_assign(scope, assign).map(HirStatement::Assign),
            Statement::Transition(transition) => self
                .lower_transition(scope, transition)
                .map(HirStatement::Transition),
        }
    }

    fn lower_transition(
        &mut self,
        scope: &Scope,
        transition: &ast::TransitionStatement,
    ) -> Option<HirTransition> {
        let value = self.resolve_expr(scope, &transition.value);
        let duration = self.resolve_expr(scope, &transition.duration);
        let target = match self.targets.resolve(&transition.target.name) {
            Some(target) => Some(target),
            None => {
                self.errors.push(HirError::new(
                    format!("unknown target `{}`", transition.target.name),
                    transition.target.span,
                ));
                None
            }
        };

        Some(HirTransition {
            target: target?,
            attribute_name: transition.attribute.name.clone(),
            attribute_span: transition.attribute.span,
            value: value?,
            duration: duration?,
            span: transition.span,
        })
    }

    fn lower_assign(&mut self, scope: &Scope, assign: &ast::AssignStatement) -> Option<HirAssign> {
        let value = self.resolve_expr(scope, &assign.value);

        let target = match self.targets.resolve(&assign.target.name) {
            Some(target) => Some(target),
            None => {
                self.errors.push(HirError::new(
                    format!("unknown target `{}`", assign.target.name),
                    assign.target.span,
                ));
                None
            }
        };

        Some(HirAssign {
            target: target?,
            attribute_name: assign.attribute.name.clone(),
            attribute_span: assign.attribute.span,
            value: value?,
            span: assign.span,
        })
    }

    fn lower_let(
        &mut self,
        scope: &mut Scope,
        locals: &mut Vec<LocalDecl>,
        let_stmt: &ast::LetStatement,
    ) -> Option<HirLet> {
        // Resolved against the scope *before* this binding is declared:
        // `let x = x;` must report `x` as unknown, not refer to itself.
        let value = self.resolve_expr(scope, &let_stmt.value);

        if scope.is_declared(&let_stmt.name.name) {
            self.errors.push(HirError::new(
                format!("duplicate variable `{}`", let_stmt.name.name),
                let_stmt.name.span,
            ));
        }

        let id = LocalId(locals.len() as u32);
        locals.push(LocalDecl {
            id,
            name: let_stmt.name.name.clone(),
            span: let_stmt.name.span,
            is_mut: let_stmt.is_mut,
            type_annotation: let_stmt.type_annotation.as_ref().map(|t| TypeAnnotation {
                name: t.name.clone(),
                span: t.span,
            }),
        });
        scope.declare(let_stmt.name.name.clone(), id);

        Some(HirLet {
            local: id,
            value: value?,
            span: let_stmt.span,
        })
    }

    fn lower_wait(&mut self, scope: &Scope, wait_stmt: &ast::WaitStatement) -> Option<HirWait> {
        let value = self.resolve_expr(scope, &wait_stmt.value)?;
        Some(HirWait {
            value,
            span: wait_stmt.span,
        })
    }

    fn lower_expr_statement(
        &mut self,
        scope: &Scope,
        expr_stmt: &ast::ExpressionStatement,
    ) -> Option<HirExprStatement> {
        let value = self.resolve_expr(scope, &expr_stmt.expr)?;
        Some(HirExprStatement {
            value,
            span: expr_stmt.span,
        })
    }

    fn resolve_expr(&mut self, scope: &Scope, expr: &Expression) -> Option<HirExpr> {
        match expr {
            Expression::Literal(lit, span) => Some(HirExpr::Literal(*lit, *span)),
            Expression::Identifier(id) => match scope.lookup(&id.name) {
                Some(local) => Some(HirExpr::Local(local, id.span)),
                None => {
                    self.errors.push(HirError::new(
                        format!("unknown name `{}`", id.name),
                        id.span,
                    ));
                    None
                }
            },
            Expression::Unary(u) => {
                let operand = self.resolve_expr(scope, &u.operand)?;
                Some(HirExpr::Unary {
                    op: u.op,
                    operand: Box::new(operand),
                    span: u.span,
                })
            }
            Expression::Binary(b) => {
                // Resolve both sides even if one fails, to surface as many
                // diagnostics as possible in one pass.
                let lhs = self.resolve_expr(scope, &b.lhs);
                let rhs = self.resolve_expr(scope, &b.rhs);
                Some(HirExpr::Binary {
                    op: b.op,
                    lhs: Box::new(lhs?),
                    rhs: Box::new(rhs?),
                    span: b.span,
                })
            }
            Expression::Grouped(inner, _) => self.resolve_expr(scope, inner),
            Expression::Call(call) => {
                // Still visit arguments so errors inside them are reported
                // too, even though the call itself always fails to resolve.
                for arg in &call.args {
                    self.resolve_expr(scope, arg);
                }
                self.errors.push(
                    HirError::new(
                        format!("unknown function `{}`", call.callee.name),
                        call.callee.span,
                    )
                    .with_help("no functions are defined in this program"),
                );
                None
            }
        }
    }
}
