//! The bytecode verifier.
//!
//! This is the safety boundary between "a `BytecodeModule` value exists"
//! and "a future VM may safely execute it". A module can arrive here
//! either freshly compiled (and should always pass) or hand-built —
//! directly by a test, or in the future loaded from disk — and in that
//! second case nothing about its shape can be trusted. `verify` never
//! panics on any `BytecodeModule`, including malformed ones: every index
//! is bounds-checked before use.
//!
//! V1's instruction set has no jumps or branches, so a function's code is
//! a single straight-line sequence. That makes stack simulation exact and
//! total: a single linear pass over `code` is enough to know the operand
//! stack's contents at every point, with no control-flow merging to
//! reconcile. This will need to become a proper data-flow fixpoint once
//! `jump`/`branch` exist.

use crate::ids::{ConstantId, FunctionId, LocalId, TargetId};
use crate::instruction::Instruction;
use crate::module::{BytecodeModule, BytecodeVersion, Function};
use crate::value::ValueType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationErrorKind {
    UnsupportedVersion(BytecodeVersion),
    InvalidEntry(FunctionId),
    InvalidFunctionId(FunctionId),
    InvalidConstantId(ConstantId),
    InvalidLocalId(LocalId),
    /// A `SetAttribute` `TargetId` is `>= module.target_count`.
    InvalidTargetId(TargetId),
    InvalidRoleId(crate::RoleId),
    InvalidRoleTarget(TargetId),
    /// V1 only defines interpolation for intensity.
    UnsupportedTransitionAttribute(crate::Attribute),
    /// Popped a value from an empty operand stack.
    StackUnderflow,
    /// The function's code needs more stack depth than its declared
    /// `max_stack` allows for.
    StackOverflow,
    /// `StoreLocal` popped a value whose type doesn't match the local's
    /// declared type.
    TypeMismatch {
        expected: ValueType,
        found: ValueType,
    },
    /// An arithmetic instruction (`Add`/`Sub`/`Mul`/`Div`) was applied to
    /// a type combination with no defined result.
    InvalidArithmeticOperands {
        lhs: ValueType,
        rhs: ValueType,
    },
    /// `Wait` popped a value that isn't a `Duration`.
    InvalidWaitOperand(ValueType),
    /// Either the function's code doesn't end with `Return`, or it does
    /// but the operand stack isn't empty at that point.
    InvalidReturn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationError {
    /// `None` for module-level errors (bad version, bad entry point)
    /// that aren't tied to one function.
    pub function: Option<FunctionId>,
    /// `None` when the error isn't tied to one instruction.
    pub instruction: Option<usize>,
    pub kind: VerificationErrorKind,
}

impl VerificationError {
    fn module(kind: VerificationErrorKind) -> Self {
        Self {
            function: None,
            instruction: None,
            kind,
        }
    }

    fn in_function(function: FunctionId, kind: VerificationErrorKind) -> Self {
        Self {
            function: Some(function),
            instruction: None,
            kind,
        }
    }

    fn at(function: FunctionId, instruction: usize, kind: VerificationErrorKind) -> Self {
        Self {
            function: Some(function),
            instruction: Some(instruction),
            kind,
        }
    }
}

/// Verifies that `module` is internally consistent and safe to execute:
/// every ID it contains resolves to something that exists, every
/// instruction's operand types line up, and every function's operand
/// stack is used consistently. Returns every problem found, not just the
/// first.
pub fn verify(module: &BytecodeModule) -> Result<(), Vec<VerificationError>> {
    let mut errors = Vec::new();

    if module.version != BytecodeVersion::CURRENT {
        errors.push(VerificationError::module(
            VerificationErrorKind::UnsupportedVersion(module.version),
        ));
    }

    for (index, function) in module.functions.iter().enumerate() {
        if function.id != FunctionId(index as u32) {
            errors.push(VerificationError::module(
                VerificationErrorKind::InvalidFunctionId(function.id),
            ));
        }
    }

    if let Some(entry) = module.entry
        && entry.0 as usize >= module.functions.len()
    {
        errors.push(VerificationError::module(
            VerificationErrorKind::InvalidEntry(entry),
        ));
    }

    if let Some(contract) = &module.rig_contract {
        for (index, role) in contract.roles.iter().enumerate() {
            if role.id != crate::RoleId(index as u32) {
                errors.push(VerificationError::module(
                    VerificationErrorKind::InvalidRoleId(role.id),
                ));
            }
            if role.target.0 >= module.target_count {
                errors.push(VerificationError::module(
                    VerificationErrorKind::InvalidRoleTarget(role.target),
                ));
            }
        }
    }

    for function in &module.functions {
        verify_function(module, function, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn verify_function(
    module: &BytecodeModule,
    function: &Function,
    errors: &mut Vec<VerificationError>,
) {
    if function.code.last() != Some(&Instruction::Return) {
        errors.push(VerificationError::in_function(
            function.id,
            VerificationErrorKind::InvalidReturn,
        ));
    }

    let mut stack: Vec<ValueType> = Vec::new();
    let mut max_depth: usize = 0;

    for (index, instruction) in function.code.iter().enumerate() {
        verify_instruction(module, function, index, *instruction, &mut stack, errors);
        max_depth = max_depth.max(stack.len());
    }

    if max_depth > function.max_stack as usize {
        errors.push(VerificationError::in_function(
            function.id,
            VerificationErrorKind::StackOverflow,
        ));
    }
}

fn verify_instruction(
    module: &BytecodeModule,
    function: &Function,
    index: usize,
    instruction: Instruction,
    stack: &mut Vec<ValueType>,
    errors: &mut Vec<VerificationError>,
) {
    match instruction {
        Instruction::Const(id) => match module.constants.get(id.0 as usize) {
            Some(constant) => stack.push(constant.value_type()),
            None => errors.push(VerificationError::at(
                function.id,
                index,
                VerificationErrorKind::InvalidConstantId(id),
            )),
        },

        Instruction::LoadLocal(id) => match function.locals.get(id.0 as usize) {
            Some(&ty) => stack.push(ty),
            None => errors.push(VerificationError::at(
                function.id,
                index,
                VerificationErrorKind::InvalidLocalId(id),
            )),
        },

        Instruction::StoreLocal(id) => {
            let Some(&expected) = function.locals.get(id.0 as usize) else {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::InvalidLocalId(id),
                ));
                return;
            };
            match stack.pop() {
                None => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::StackUnderflow,
                )),
                Some(found) if found != expected => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::TypeMismatch { expected, found },
                )),
                Some(_) => {}
            }
        }

        Instruction::Add | Instruction::Sub | Instruction::Mul | Instruction::Div => {
            let rhs = stack.pop();
            let lhs = stack.pop();
            match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => match arithmetic_result_type(instruction, lhs, rhs) {
                    Some(result) => stack.push(result),
                    None => errors.push(VerificationError::at(
                        function.id,
                        index,
                        VerificationErrorKind::InvalidArithmeticOperands { lhs, rhs },
                    )),
                },
                _ => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::StackUnderflow,
                )),
            }
        }

        Instruction::Wait => match stack.pop() {
            Some(ValueType::Duration) => {}
            Some(found) => errors.push(VerificationError::at(
                function.id,
                index,
                VerificationErrorKind::InvalidWaitOperand(found),
            )),
            None => errors.push(VerificationError::at(
                function.id,
                index,
                VerificationErrorKind::StackUnderflow,
            )),
        },

        Instruction::Call(callee) => {
            if callee.0 as usize >= module.functions.len() {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::InvalidFunctionId(callee),
                ));
            }
            // V1 defines no parameters or return values for calls, so
            // there's no stack effect to check yet.
        }

        Instruction::Return => {
            if !stack.is_empty() {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::InvalidReturn,
                ));
            }
        }

        Instruction::Pop => {
            if stack.pop().is_none() {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::StackUnderflow,
                ));
            }
        }

        Instruction::SetAttribute { target, attribute } => {
            if target.0 >= module.target_count {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::InvalidTargetId(target),
                ));
            }
            let expected = attribute.value_type();
            match stack.pop() {
                None => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::StackUnderflow,
                )),
                Some(found) if found != expected => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::TypeMismatch { expected, found },
                )),
                Some(_) => {}
            }
        }
        Instruction::TransitionAttribute { target, attribute } => {
            if target.0 >= module.target_count {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::InvalidTargetId(target),
                ));
            }
            if attribute != crate::Attribute::Intensity {
                errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::UnsupportedTransitionAttribute(attribute),
                ));
            }

            // Codegen pushes value then duration, so duration is popped first.
            match stack.pop() {
                Some(ValueType::Duration) => {}
                Some(found) => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::TypeMismatch {
                        expected: ValueType::Duration,
                        found,
                    },
                )),
                None => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::StackUnderflow,
                )),
            }

            let expected = attribute.value_type();
            match stack.pop() {
                Some(found) if found != expected => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::TypeMismatch { expected, found },
                )),
                Some(_) => {}
                None => errors.push(VerificationError::at(
                    function.id,
                    index,
                    VerificationErrorKind::StackUnderflow,
                )),
            }
        }
    }
}

/// The result type of an arithmetic instruction applied to `lhs`/`rhs`,
/// or `None` if that combination isn't supported. Deliberately a
/// standalone copy of `lux_typeck::rules::binary_result_type`'s table
/// rather than a shared call: `lux-bytecode` must not depend on the
/// compiler frontend (see `AGENTS.md`), since it needs to be usable by
/// `inception-vm` on its own. The two tables are expected to stay in
/// sync; `lux-compiler`'s end-to-end tests catch a drift indirectly, by
/// verifying every module the real compiler produces.
fn arithmetic_result_type(
    instruction: Instruction,
    lhs: ValueType,
    rhs: ValueType,
) -> Option<ValueType> {
    match instruction {
        Instruction::Add | Instruction::Sub => match (lhs, rhs) {
            (ValueType::Int, ValueType::Int) => Some(ValueType::Int),
            (ValueType::Float, ValueType::Float) => Some(ValueType::Float),
            (ValueType::Duration, ValueType::Duration) => Some(ValueType::Duration),
            (ValueType::Intensity, ValueType::Intensity) => Some(ValueType::Intensity),
            _ => None,
        },
        Instruction::Mul | Instruction::Div => match (lhs, rhs) {
            (ValueType::Int, ValueType::Int) => Some(ValueType::Int),
            (ValueType::Float, ValueType::Float) => Some(ValueType::Float),
            _ => None,
        },
        _ => None,
    }
}
