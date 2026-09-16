//! A single call frame.

use lux_bytecode::FunctionId;

use crate::value::Value;

/// One activation record. Note there is deliberately no per-frame
/// operand stack here — matching the task brief's sketch — because V1's
/// `Call` defines no parameters or return values (see
/// `lux_bytecode::Instruction::Call`'s docs), so there's no need to
/// isolate one frame's operands from another's; [`crate::Vm`] keeps a
/// single operand stack shared across the whole call stack.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frame {
    pub function: FunctionId,
    pub pc: usize,
    /// One slot per local, indexed by `LocalId`. `None` means declared
    /// but never written — reading it is a [`crate::VmErrorKind::UninitializedLocal`],
    /// not a silently-wrong default value.
    pub locals: Vec<Option<Value>>,
}

impl Frame {
    pub fn new(function: FunctionId, local_count: usize) -> Self {
        Self {
            function,
            pc: 0,
            locals: vec![None; local_count],
        }
    }
}
