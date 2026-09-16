//! The V1 instruction set: a small, stack-based opcode list.
//!
//! Kept intentionally minimal (see `AGENTS.md`'s "hors scope" list) —
//! nothing DMX-, fixture- or scheduling-related exists yet. Represented
//! as a plain Rust enum rather than a packed binary encoding: the
//! priority for this milestone is correctness and verifiability, not a
//! final on-disk format (see item 15 of the task brief).

use crate::ids::{ConstantId, FunctionId, LocalId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    /// Pushes a constant pool entry onto the stack.
    Const(ConstantId),

    /// Pushes a local's current value onto the stack.
    LoadLocal(LocalId),
    /// Pops the top of the stack into a local.
    StoreLocal(LocalId),

    Add,
    Sub,
    Mul,
    Div,

    /// Pops a `Duration` and suspends execution for that long.
    Wait,

    /// Calls another function. V1 defines no parameters or return
    /// values: this has no stack effect yet, and nothing currently
    /// generates it (see `lux-mir`) — it exists so the opcode doesn't
    /// need to be redesigned once user-defined functions exist.
    Call(FunctionId),

    /// Ends the current function. The stack must be empty at this point
    /// (Lux scenes don't return a value in this milestone).
    Return,

    /// Discards the top of the stack, e.g. for a bare expression
    /// statement whose value is unused.
    Pop,
}
