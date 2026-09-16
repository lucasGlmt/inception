//! The bytecode module format: the top-level structure a compiler
//! produces and a verifier/VM consumes.

use crate::ids::FunctionId;
use crate::instruction::Instruction;
use crate::value::{Constant, ValueType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytecodeVersion {
    pub major: u16,
    pub minor: u16,
}

impl BytecodeVersion {
    /// The only version this crate currently produces or accepts.
    pub const CURRENT: BytecodeVersion = BytecodeVersion { major: 0, minor: 1 };
}

#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeModule {
    pub version: BytecodeVersion,
    pub constants: Vec<Constant>,
    pub functions: Vec<Function>,
    /// The function to start execution from, if any. `None` is valid: a
    /// module with no `main` scene simply can't be run directly (nothing
    /// checks for this today since there is no VM yet).
    pub entry: Option<FunctionId>,
}

/// A single function's compiled code.
///
/// Deviates from a naive "just a `local_count: u16`" field: the verifier
/// needs to know each local's *type* (item 17 of the task brief), and
/// keeping a type per local is a strict superset of just a count —
/// `locals.len()` is the count. A separate count field that could
/// disagree with `locals.len()` would be an invalid state that's
/// possible to represent for no benefit, which `AGENTS.md`'s first
/// principle asks us to avoid.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub id: FunctionId,
    pub code: Vec<Instruction>,
    /// One entry per local slot, indexed by `LocalId`.
    pub locals: Vec<ValueType>,
    /// The maximum operand stack depth this function's code should ever
    /// reach. Declared by the producer, but never trusted blindly: the
    /// verifier independently simulates the stack and rejects the module
    /// if actual usage would exceed this (see `lux_bytecode::verify`).
    pub max_stack: u16,
    /// Kept for tests/tooling/disassembly only; the future VM must not
    /// depend on it.
    pub debug_name: Option<String>,
}
