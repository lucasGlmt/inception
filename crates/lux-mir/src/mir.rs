//! MIR: explicit, typed, stack-oriented operations after type checking.
//!
//! MIR intentionally drops syntactic abstractions the language no longer
//! needs past this point (see `AGENTS.md`, item 2 of the task brief):
//! there's no expression tree, no statement kinds — just a flat sequence
//! of [`MirInstruction`]s per block, operating on an implicit operand
//! stack, plus explicit local slots. A `let` and a `wait` are lowered
//! identically to "evaluate this expression, then do something with the
//! top of the stack".
//!
//! ## Why `BasicBlock`/`Terminator` for a linear V1
//!
//! Every function currently lowers to exactly one [`BasicBlock`] ending
//! in [`Terminator::Return`] — there's no `jump`/`branch`/`loop` yet. The
//! block/terminator split exists anyway so that adding those later is an
//! additive change to `Terminator` (new variants) and to how
//! `MirFunction::blocks` gets built, not a restructuring of the type
//! itself.

use lux_typeck::Type;

use crate::ids::{BlockId, FunctionId, LocalId};

#[derive(Debug, Clone, PartialEq)]
pub struct MirModule {
    pub functions: Vec<MirFunction>,
    /// The function to start execution from — by convention, the scene
    /// named `main`, if the program has one. See `lux-mir::lower`.
    pub entry: Option<FunctionId>,
    /// How many distinct lighting targets exist, per the
    /// `TargetEnvironment` `lower` was given — copied straight through to
    /// `lux_bytecode::BytecodeModule::target_count` by codegen.
    pub target_count: u32,
    pub rig_contract: Option<MirRigContract>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirRigContract {
    pub name: String,
    pub roles: Vec<MirRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirRole {
    pub id: lux_hir::RoleId,
    pub target: lux_hir::TargetId,
    pub name: String,
    pub capabilities: lux_hir::CapabilitySet,
    pub cardinality: lux_hir::RoleCardinality,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirFunction {
    pub id: FunctionId,
    /// Kept for debugging/tooling only; not used to resolve anything.
    pub name: String,
    /// Every local declared in this function, in declaration order —
    /// matches the source `HirScene::locals` order, so a `LocalId` means
    /// the same thing in both.
    pub locals: Vec<MirLocal>,
    pub blocks: Vec<BasicBlock>,
}

impl MirFunction {
    /// V1 functions have exactly one block: execution always starts —
    /// and, since there's no `jump`/`branch` yet, always stays — here.
    pub fn entry_block(&self) -> &BasicBlock {
        &self.blocks[0]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirLocal {
    pub id: LocalId,
    /// Kept for debugging/tooling only.
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<MirInstruction>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// Ends the function. Scenes don't return a value in this milestone,
    /// so unlike a real "return expression", this carries none — by the
    /// time codegen reaches it, the operand stack is expected to be
    /// empty (checked by `lux_bytecode::verify`, not here).
    Return,
}

/// A single MIR operation. Stack-based, mirroring the eventual bytecode
/// instruction set closely (see `lux-bytecode`) — MIR exists as a
/// separate stage mainly to keep constants un-pooled and types attached
/// to locals, not because its operations differ from bytecode's.
#[derive(Debug, Clone, PartialEq)]
pub enum MirInstruction {
    /// Pushes a constant value. Unlike bytecode's `Const(ConstantId)`,
    /// MIR keeps the value inline rather than pooled — interning into a
    /// module-wide constant pool is a bytecode-codegen concern (see
    /// `crate::codegen`), not something MIR itself needs to model.
    Const(MirConstant),
    LoadLocal(LocalId),
    StoreLocal(LocalId),
    Add,
    Sub,
    Mul,
    Div,
    Wait,
    /// Discards the top of the stack (e.g. a bare expression statement).
    Pop,
    /// Pops a value and applies it to `attribute` on `target`, in the
    /// semantic `LightingState` — see `lux_bytecode::Instruction::SetAttribute`,
    /// which this lowers to directly. `TargetId` is reused from
    /// `lux-hir` and `Attribute` from `lux-typeck`: MIR already depends
    /// on both, so redefining either here would be pure duplication (see
    /// `crate::ids`'s docs on why `LocalId` is reused the same way).
    SetAttribute {
        target: lux_hir::TargetId,
        attribute: lux_typeck::Attribute,
    },
    /// Pops `duration` first, then the target value, and starts a
    /// non-blocking absolute-time transition.
    TransitionAttribute {
        target: lux_hir::TargetId,
        attribute: lux_typeck::Attribute,
    },
    /// Pops a `Signal<T>` value and installs it as the continuously-sampled
    /// controller for `attribute` on `target`, replacing whatever
    /// transition or signal binding previously controlled it — see
    /// `lux_bytecode::Instruction::BindSignal`, which this lowers to
    /// directly. Deliberately *not* lowered to "sample once + SetAttribute":
    /// the binding must stay alive in the runtime past this instruction.
    BindSignal {
        target: lux_hir::TargetId,
        attribute: lux_typeck::Attribute,
    },
    /// Calls one builtin stdlib intrinsic (`std.Math`, `std.Color`, ...).
    /// `intrinsic` is `lux-stdlib`'s compiler-side id, reused directly
    /// (MIR already depends on `lux-stdlib`, so redefining it here would
    /// just be duplication, unlike the frontend/runtime boundary crossed
    /// by `crate::codegen::to_bytecode_intrinsic`). Arguments are already
    /// on the stack, evaluated left to right; net stack effect is
    /// `1 - arg_count`.
    CallIntrinsic {
        intrinsic: lux_stdlib::IntrinsicId,
        arg_count: u8,
    },
}

/// A color value, normalized to RGB. `lux-hir`'s `ColorLiteral` can still
/// be a named color (`red`, `blue`, ...); by MIR, that syntactic
/// convenience has already been resolved to a concrete value, per this
/// crate's job of dropping abstractions the language no longer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorValue {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A constant's runtime value, in the domain-appropriate representation
/// described in the task brief (e.g. durations in nanoseconds). See
/// `crate::values` for the conversions from `lux_syntax::ast::Literal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MirConstant {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Nanoseconds.
    Duration(u64),
    /// `0..=65535`, linearly mapped from the source's `0..=100%`.
    Intensity(u16),
    Color(ColorValue),
    /// Millidegrees.
    Angle(i32),
    /// Hertz.
    Frequency(u32),
    /// Beats per minute.
    Tempo(u32),
}
