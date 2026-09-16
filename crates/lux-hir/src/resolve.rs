//! Lowers an AST into HIR while resolving names.
//!
//! Lowering and resolution happen in a single pass: by design, HIR never
//! represents an unresolved local reference (see [`crate::hir`]), so a
//! [`HirExpr::Local`] can only be built once its [`LocalId`] is already
//! known.
//!
//! ## Function calls
//!
//! Lux still defines no *user-defined* functions, so an unqualified call
//! (`foo()`) is always an `unknown function` error, exactly as before
//! module support existed. A *qualified* call (`Math.sin(...)`) is
//! resolved against this file's `import`s: first against `std`'s
//! signature registry ([`lux_stdlib`]), and for a non-`std` qualifier,
//! against the [`UserModuleEnvironment`] passed in by the caller (which
//! `lux-project` populates from real files on disk — this crate never
//! touches the filesystem itself). A resolved user module currently
//! exposes zero members (Lux has no function-declaration syntax yet), so
//! every member access on one fails with "has no member" — a documented,
//! temporary limitation, not a bug.
//!
//! ## Error recovery
//!
//! Like the parser, resolution tries to surface more than one diagnostic
//! per file: an unresolved sub-expression makes its parent unresolved too
//! (represented as `None`), but sibling statements and sibling operands
//! are still visited.

use std::collections::HashMap;

use lux_syntax::Span;
use lux_syntax::ast::{self, CallExpr, Expression, Item, SourceFile, Statement};

use crate::environment::TargetEnvironment;
use crate::error::HirError;
use crate::hir::{
    Capability, CapabilitySet, HirAssign, HirCall, HirCallee, HirExpr, HirExprStatement, HirFile,
    HirLet, HirRigContract, HirRole, HirScene, HirStatement, HirTransition, HirWait, LocalDecl,
    RoleCardinality, TypeAnnotation,
};
use crate::ids::{LocalId, RoleId, SceneId};
use crate::user_modules::UserModuleEnvironment;

/// Lowers a parsed source file into HIR, resolving every name reference —
/// both local variables and, per `targets`, lighting target names (see
/// [`TargetEnvironment`]'s docs for why that's a parameter here rather
/// than something this crate resolves on its own). Returns every
/// diagnostic collected (never just the first) when resolution fails
/// anywhere in the file.
///
/// Equivalent to [`lower_with_modules`] with an empty
/// [`UserModuleEnvironment`] — every existing single-file caller keeps
/// this signature and behavior unchanged; only `std.*` imports resolve.
pub fn lower(ast: &SourceFile, targets: &TargetEnvironment) -> Result<HirFile, Vec<HirError>> {
    lower_with_modules(ast, targets, &UserModuleEnvironment::empty())
}

/// Like [`lower`], but non-`std` imports (`import show.Helpers;`) are
/// resolved against `user_modules` instead of always failing. Used by
/// `lux-compiler`'s multi-file orchestration once `lux-project` has built
/// a real file graph.
pub fn lower_with_modules(
    ast: &SourceFile,
    targets: &TargetEnvironment,
    user_modules: &UserModuleEnvironment,
) -> Result<HirFile, Vec<HirError>> {
    let mut resolved_targets = targets.clone();
    let mut pre_errors = Vec::new();
    let rig_contract = lower_rig_contract(ast, &mut resolved_targets, &mut pre_errors);
    let imports = build_import_table(ast, user_modules, &mut pre_errors);
    let mut lowering = Lowering {
        errors: pre_errors,
        targets: &resolved_targets,
        imports,
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
            Item::Scene(_) | Item::Import(_) => None,
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

/// What a single `import`'s qualifier (its path's last segment) resolved
/// to.
enum ResolvedImport {
    Std(&'static lux_stdlib::StdModule),
    /// A real file `lux-project` found on disk, exposing zero members
    /// today (see this module's doc comment).
    User,
}

struct ImportEntry {
    resolved: ResolvedImport,
    /// The full dotted path as written (`"std.Math"`, `"show.Helpers"`),
    /// kept only to report *which* two imports collide when a qualifier
    /// is ambiguous.
    full_path: String,
}

/// Maps each imported qualifier (`Math` in `import std.Math;`) to what it
/// resolved to, built once per file before any scene is lowered.
#[derive(Default)]
struct ImportTable {
    entries: HashMap<String, ImportEntry>,
}

impl ImportTable {
    fn get(&self, qualifier: &str) -> Option<&ResolvedImport> {
        self.entries.get(qualifier).map(|e| &e.resolved)
    }
}

fn build_import_table(
    ast: &SourceFile,
    user_modules: &UserModuleEnvironment,
    errors: &mut Vec<HirError>,
) -> ImportTable {
    let mut table = ImportTable::default();

    for item in &ast.items {
        let Item::Import(decl) = item else { continue };
        if decl.path.is_empty() {
            continue; // only reachable via a parser-recovery placeholder
        }
        let segments: Vec<&str> = decl.path.iter().map(|id| id.name.as_str()).collect();
        let full_path = segments.join(".");
        let qualifier = segments.last().copied().unwrap_or_default().to_string();

        let resolved = if segments[0] == "std" {
            match lux_stdlib::find_module(&segments) {
                Some(module) => ResolvedImport::Std(module),
                None => {
                    errors.push(HirError::new(
                        format!("unknown module `{full_path}`"),
                        decl.span,
                    ));
                    continue;
                }
            }
        } else if user_modules.resolves(&full_path) {
            ResolvedImport::User
        } else {
            errors.push(HirError::new(
                format!("unknown module `{full_path}`"),
                decl.span,
            ));
            continue;
        };

        match table.entries.get(&qualifier) {
            None => {
                table.entries.insert(
                    qualifier,
                    ImportEntry {
                        resolved,
                        full_path,
                    },
                );
            }
            Some(existing) if existing.full_path == full_path => {
                // Importing the exact same module twice is an idempotent
                // no-op, not an error — see `docs/rfcs/0001-modules-and-stdlib.md`.
            }
            Some(existing) => {
                errors.push(HirError::new(
                    format!(
                        "module name `{qualifier}` is ambiguous between `{}` and `{full_path}`",
                        existing.full_path
                    ),
                    decl.span,
                ));
            }
        }
    }

    table
}

/// Levenshtein edit distance, used only for "did you mean" suggestions
/// over a module's (always tiny — a handful of names) member list. O(n*m)
/// is more than cheap enough at that size.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev_diag = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let tmp = row[j];
            row[j] = if a[i - 1] == b[j - 1] {
                prev_diag
            } else {
                1 + prev_diag.min(row[j]).min(row[j - 1])
            };
            prev_diag = tmp;
        }
    }
    row[b.len()]
}

/// The closest member name in `candidates` to `name`, if within edit
/// distance 2 — cheap and reliable given how short these names are.
fn suggest_member<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .map(|candidate| (candidate, edit_distance(name, candidate)))
        .filter(|(_, dist)| *dist <= 2)
        .min_by_key(|(_, dist)| *dist)
        .map(|(candidate, _)| candidate)
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
    imports: ImportTable,
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
            Expression::Call(call) => self.resolve_call(scope, call),
        }
    }

    fn resolve_call(&mut self, scope: &Scope, call: &CallExpr) -> Option<HirExpr> {
        // Resolve every argument regardless of whether the callee itself
        // resolves, so unrelated errors inside them are still reported.
        let args: Vec<Option<HirExpr>> = call
            .args
            .iter()
            .map(|arg| self.resolve_expr(scope, arg))
            .collect();

        let Some(qualifier) = &call.callee.qualifier else {
            // Unqualified call: Lux has no user-defined-function
            // namespace, so this always fails — unchanged from before
            // modules existed.
            self.errors.push(
                HirError::new(
                    format!("unknown function `{}`", call.callee.name.name),
                    call.callee.name.span,
                )
                .with_help("no functions are defined in this program"),
            );
            return None;
        };

        let member_name = &call.callee.name.name;
        match self.imports.get(&qualifier.name) {
            None => {
                // Not imported. If it's a real std module the author
                // just forgot to import, say so specifically.
                match lux_stdlib::find_module_by_short_name(&qualifier.name) {
                    Some(module) => {
                        self.errors.push(
                            HirError::new(
                                format!("module `{}` is not imported", qualifier.name),
                                qualifier.span,
                            )
                            .with_help(format!("add `import {};`", module.path.join("."))),
                        );
                    }
                    None => {
                        self.errors.push(HirError::new(
                            format!("unknown module `{}`", qualifier.name),
                            qualifier.span,
                        ));
                    }
                }
                None
            }
            Some(ResolvedImport::Std(module)) => {
                if lux_stdlib::candidates(module.path, member_name).is_empty() {
                    let mut error = HirError::new(
                        format!("module `{}` has no member `{member_name}`", qualifier.name),
                        call.callee.name.span,
                    );
                    if let Some(suggestion) =
                        suggest_member(member_name, module.functions.iter().map(|f| f.name))
                    {
                        error = error.with_help(format!("did you mean `{suggestion}`?"));
                    }
                    self.errors.push(error);
                    return None;
                }
                let mut resolved_args = Vec::with_capacity(args.len());
                for arg in args {
                    resolved_args.push(arg?);
                }
                Some(HirExpr::Call(HirCall {
                    callee: HirCallee::Std {
                        module_path: module.path,
                        name: member_name.clone(),
                        name_span: call.callee.name.span,
                    },
                    args: resolved_args,
                    span: call.span,
                }))
            }
            Some(ResolvedImport::User) => {
                // Every user module exports zero members today (see this
                // module's doc comment) — always fails, regardless of
                // `member_name`.
                self.errors.push(HirError::new(
                    format!("module `{}` has no member `{member_name}`", qualifier.name),
                    call.callee.name.span,
                ));
                None
            }
        }
    }
}
