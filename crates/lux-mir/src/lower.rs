//! Lowers checked HIR into MIR.
//!
//! # Precondition
//!
//! `hir` must already have passed [`lux_typeck::check`], and `typed` must
//! be the [`lux_typeck::TypedProgram`] that call returned *for this same
//! `hir`* — `lower` does not re-validate anything. Calling it on
//! unchecked or mismatched HIR is a misuse of the API (undefined
//! behavior in the sense of "may produce nonsense MIR or panic", not
//! memory-unsafe) rather than something a Lux source file can trigger, so
//! this is not a place that reports user-facing diagnostics: see
//! `lux_typeck::infer::expr_type`'s docs for why panicking here is
//! considered acceptable per `AGENTS.md`, item 24 of the task brief.

use lux_hir::{HirExpr, HirFile, HirScene, HirStatement};
use lux_syntax::ast::{BinaryOp, UnaryOp};
use lux_typeck::{Type, TypedProgram};

use crate::ids::{BlockId, FunctionId};
use crate::mir::{
    BasicBlock, MirConstant, MirFunction, MirInstruction, MirLocal, MirModule, Terminator,
};
use crate::values::lower_literal;

/// The scene conventionally used as a program's entry point, mirroring a
/// `fn main` convention. Not enforced anywhere upstream: a program with
/// no scene named `main` is still valid, it just has no entry point
/// (`MirModule::entry` is `None`).
const ENTRY_SCENE_NAME: &str = "main";

/// `target_count` is the number of distinct lighting targets known at
/// this point — i.e. the length of the `lux_hir::TargetEnvironment` that
/// was passed to `lux_hir::lower` for this same `hir`. It flows straight
/// through to `MirModule::target_count` and, from there, to
/// `lux_bytecode::BytecodeModule::target_count`.
pub fn lower(hir: &HirFile, typed: &TypedProgram) -> MirModule {
    let mut functions = Vec::with_capacity(hir.scenes.len());
    let mut entry = None;

    for (index, scene) in hir.scenes.iter().enumerate() {
        let id = FunctionId(index as u32);
        if scene.name == ENTRY_SCENE_NAME {
            entry = Some(id);
        }
        functions.push(lower_scene(id, scene, &typed.scenes[index].local_types));
    }

    MirModule {
        functions,
        entry,
        target_count: hir.target_count,
        rig_contract: hir
            .rig_contract
            .as_ref()
            .map(|contract| crate::mir::MirRigContract {
                name: contract.name.clone(),
                roles: contract
                    .roles
                    .iter()
                    .map(|role| crate::mir::MirRole {
                        id: role.id,
                        target: role.target,
                        name: role.name.clone(),
                        capabilities: role.capabilities,
                        cardinality: role.cardinality,
                    })
                    .collect(),
            }),
    }
}

fn lower_scene(id: FunctionId, scene: &HirScene, local_types: &[Type]) -> MirFunction {
    let locals = scene
        .locals
        .iter()
        .zip(local_types)
        .map(|(decl, &ty)| MirLocal {
            id: decl.id,
            name: decl.name.clone(),
            ty,
        })
        .collect();

    let mut instructions = Vec::new();
    for stmt in &scene.statements {
        lower_statement(stmt, local_types, &mut instructions);
    }

    let block = BasicBlock {
        id: BlockId(0),
        instructions,
        terminator: Terminator::Return,
    };

    MirFunction {
        id,
        name: scene.name.clone(),
        locals,
        blocks: vec![block],
    }
}

fn lower_statement(stmt: &HirStatement, local_types: &[Type], out: &mut Vec<MirInstruction>) {
    match stmt {
        HirStatement::Let(let_stmt) => {
            lower_expr(&let_stmt.value, local_types, out);
            out.push(MirInstruction::StoreLocal(let_stmt.local));
        }
        HirStatement::Wait(wait_stmt) => {
            lower_expr(&wait_stmt.value, local_types, out);
            out.push(MirInstruction::Wait);
        }
        HirStatement::Expression(expr_stmt) => {
            lower_expr(&expr_stmt.value, local_types, out);
            out.push(MirInstruction::Pop);
        }
        HirStatement::Assign(assign) => {
            lower_expr(&assign.value, local_types, out);
            // Trusted re-derivation, not re-validation: `lux_typeck::check`
            // already rejected any unknown attribute name for this HIR to
            // exist here (see this module's precondition docs). Cheap
            // enough (a linear scan over `Attribute::ALL`, currently 2
            // entries) that threading the resolved `Attribute` through
            // `TypedProgram` instead wasn't worth the extra plumbing.
            let attribute = lux_typeck::Attribute::from_name(&assign.attribute_name).unwrap_or_else(|| {
                unreachable!(
                    "lower: attribute `{}` should already be valid, checked by lux_typeck::check",
                    assign.attribute_name
                )
            });
            out.push(MirInstruction::SetAttribute {
                target: assign.target,
                attribute,
            });
        }
        HirStatement::Transition(transition) => {
            lower_expr(&transition.value, local_types, out);
            lower_expr(&transition.duration, local_types, out);
            let attribute = lux_typeck::Attribute::from_name(&transition.attribute_name)
                .unwrap_or_else(|| {
                    unreachable!(
                        "lower: transition attribute `{}` should already be valid",
                        transition.attribute_name
                    )
                });
            out.push(MirInstruction::TransitionAttribute {
                target: transition.target,
                attribute,
            });
        }
        HirStatement::BindSignal(bind) => {
            lower_expr(&bind.signal, local_types, out);
            let attribute =
                lux_typeck::Attribute::from_name(&bind.attribute_name).unwrap_or_else(|| {
                    unreachable!(
                        "lower: signal binding attribute `{}` should already be valid",
                        bind.attribute_name
                    )
                });
            out.push(MirInstruction::BindSignal {
                target: bind.target,
                attribute,
            });
        }
    }
}

fn lower_expr(expr: &HirExpr, local_types: &[Type], out: &mut Vec<MirInstruction>) {
    match expr {
        HirExpr::Literal(lit, _) => out.push(MirInstruction::Const(lower_literal(*lit))),
        HirExpr::Local(id, _) => out.push(MirInstruction::LoadLocal(*id)),
        HirExpr::Unary { op, operand, .. } => lower_unary(*op, operand, local_types, out),
        HirExpr::Binary { op, lhs, rhs, .. } => {
            lower_expr(lhs, local_types, out);
            lower_expr(rhs, local_types, out);
            out.push(match op {
                BinaryOp::Add => MirInstruction::Add,
                BinaryOp::Sub => MirInstruction::Sub,
                BinaryOp::Mul => MirInstruction::Mul,
                BinaryOp::Div => MirInstruction::Div,
            });
        }
        HirExpr::Call(call) => {
            for arg in &call.args {
                lower_expr(arg, local_types, out);
            }
            // Trusted re-derivation, not re-validation — same precondition
            // as `lower_unary`'s use of `lux_typeck::expr_type` above: by
            // the time MIR lowering runs, `lux_typeck::check` has already
            // resolved this exact overload once, deterministically, from
            // the same argument types.
            let sig = lux_typeck::infer::resolve_call(local_types, call);
            out.push(MirInstruction::CallIntrinsic {
                intrinsic: sig.intrinsic,
                arg_count: call.args.len() as u8,
            });
        }
    }
}

/// There is no dedicated `Neg` opcode in this instruction set (see
/// `lux-bytecode`'s task brief, item 14): `-x` is desugared here into
/// `0 - x`, which `lux_typeck::rules` already guarantees is well-typed
/// whenever `Neg` on `x`'s type is (both are only ever valid for `Int`
/// and `Float`).
fn lower_unary(
    op: UnaryOp,
    operand: &HirExpr,
    local_types: &[Type],
    out: &mut Vec<MirInstruction>,
) {
    match op {
        UnaryOp::Neg => {
            let operand_ty = lux_typeck::expr_type(local_types, operand);
            let zero = match operand_ty {
                Type::Int => MirConstant::Int(0),
                Type::Float => MirConstant::Float(0.0),
                other => unreachable!(
                    "lower_unary: `Neg` on `{other}` should have been rejected by lux_typeck::check \
                     before MIR lowering ever runs"
                ),
            };
            out.push(MirInstruction::Const(zero));
            lower_expr(operand, local_types, out);
            out.push(MirInstruction::Sub);
        }
    }
}
