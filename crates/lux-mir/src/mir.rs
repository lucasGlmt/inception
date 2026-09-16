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
