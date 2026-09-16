//! The VM itself.
//!
//! ## `WAIT` semantics
//!
//! `WAIT` never sleeps. It pops a `Duration`, computes an absolute
//! wake-up timestamp (`clock.now() + duration`), moves the VM to
//! [`VmState::WaitingUntil`], and returns control to the caller. There is
//! no busy loop, no `remaining -= tick` countdown, and no thread ever
//! blocks: [`Vm::run_until_blocked`] simply compares `clock.now()`
//! against the stored wake-up timestamp on each call, so a runtime that
//! polls late (e.g. by five seconds when only one was needed) resumes
//! immediately and correctly, without trying to "catch up" through
//! intermediate ticks.

use inception_core::{Clock, LightingState};
use lux_bytecode::{BytecodeModule, FunctionId, Instruction};

use crate::error::{VmError, VmErrorKind, VmInitError};
use crate::frame::Frame;
use crate::state::VmState;
use crate::value::Value;

/// What executing a single instruction did.
enum Step {
    Continue,
    Wait(inception_core::Timestamp),
    Finished,
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// A single-fiber Lux bytecode interpreter.
///
/// Knows nothing about Lux source, the AST, HIR or MIR — only
/// `lux_bytecode::BytecodeModule`. See `AGENTS.md` and this crate's
/// dependency on `lux-bytecode` (never the reverse).
#[derive(Debug)]
pub struct Vm {
    module: BytecodeModule,
    entry: FunctionId,
    state: VmState,
    /// A single operand stack shared by the whole call stack (see
    /// `Frame`'s docs for why frames don't each get their own).
    stack: Vec<Value>,
    /// The currently executing frame. `None` only before `start()`.
    frame: Option<Frame>,
    /// Saved caller frames, for `Call`/`Return`.
    call_stack: Vec<Frame>,
}

impl Vm {
    /// Verifies `module` (see `lux_bytecode::verify`) and prepares a VM
    /// in the [`VmState::Ready`] state. Does not start executing —
    /// nothing runs until [`Vm::start`] and [`Vm::run_until_blocked`] are
    /// called explicitly.
    ///
    /// Fails if the module doesn't pass verification, or has no `entry`
    /// function. A `Vm` can therefore never exist for a module that
    /// isn't statically known to be safe to execute — this is `inception-vm`'s
    /// half of the "never trust that bytecode came from the real
    /// compiler" rule; the other half is that every execution-time
    /// operation below still checks its own preconditions instead of
    /// assuming verification already did (see `error` module docs).
    pub fn new(module: BytecodeModule) -> Result<Self, VmInitError> {
        lux_bytecode::verify(&module).map_err(VmInitError::Verification)?;
        let entry = module.entry.ok_or(VmInitError::NoEntryPoint)?;

        Ok(Self {
            module,
            entry,
            state: VmState::Ready,
            stack: Vec::new(),
            frame: None,
            call_stack: Vec::new(),
        })
    }

    pub fn state(&self) -> VmState {
        self.state
    }

    pub fn is_waiting(&self) -> bool {
        self.state.is_waiting()
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_finished()
    }

    pub fn is_faulted(&self) -> bool {
        self.state.is_faulted()
    }

    /// The current operand stack, top last. For tests/debugging only.
    pub fn stack(&self) -> &[Value] {
        &self.stack
    }

    /// The function the VM is currently executing, if any (`None` before
    /// `start()`).
    pub fn current_function(&self) -> Option<FunctionId> {
        self.frame.as_ref().map(|f| f.function)
    }

    /// The current program counter, if any (`None` before `start()`).
    pub fn current_pc(&self) -> Option<usize> {
        self.frame.as_ref().map(|f| f.pc)
    }

    /// Loads the entry function, creates its frame, and moves the VM to
    /// [`VmState::Running`]. Must be called exactly once, before the
    /// first [`Vm::run_until_blocked`] — deliberately not done implicitly
    /// by `new`, so construction (which can fail on a bad module) stays
    /// separate from beginning execution (see item 14 of the task
    /// brief).
    pub fn start(&mut self) -> Result<(), VmError> {
        let local_count = self
            .function(self.entry)
            .map(|f| f.locals.len())
            .ok_or_else(|| VmError::new(self.entry, 0, VmErrorKind::InvalidFunction(self.entry)))?;
        self.frame = Some(Frame::new(self.entry, local_count));
        self.state = VmState::Running;
        Ok(())
    }

    /// Executes instructions until the program blocks on a `WAIT` that
    /// hasn't elapsed yet, finishes, or faults.
    ///
    /// `lighting` is where `SET_ATTRIBUTE` commands land (see
    /// `AGENTS.md`'s and this milestone's semantic-state/DMX separation:
    /// the VM only ever calls `LightingState::set_target_attribute`,
    /// never touches a DMX buffer). A program that never assigns an
    /// attribute can just be given a fresh, empty `LightingState`.
    ///
    /// Calling this while [`VmState::Ready`] (before `start()`),
    /// [`VmState::Finished`] or [`VmState::Faulted`] is a harmless no-op.
    /// Calling it while [`VmState::WaitingUntil`] with `clock.now()`
    /// still short of the wake-up time is also a no-op — the VM stays
    /// blocked; see the module docs for why this never tries to "catch
    /// up" through intermediate steps.
    pub fn run_until_blocked<C: Clock>(
        &mut self,
        clock: &C,
        lighting: &mut LightingState,
    ) -> Result<(), VmError> {
        match self.state {
            VmState::Ready | VmState::Finished | VmState::Faulted(_) => return Ok(()),
            VmState::WaitingUntil(wake_at) => {
                if clock.now() < wake_at {
                    return Ok(());
                }
                self.state = VmState::Running;
            }
            VmState::Running => {}
        }

        loop {
            match self.execute_one(clock, lighting) {
                Ok(Step::Continue) => {}
                Ok(Step::Wait(wake_at)) => {
                    self.state = VmState::WaitingUntil(wake_at);
                    return Ok(());
                }
                Ok(Step::Finished) => {
                    self.state = VmState::Finished;
                    return Ok(());
                }
                Err(err) => {
                    self.state = VmState::Faulted(err);
                    return Err(err);
                }
            }
        }
    }

    fn function(&self, id: FunctionId) -> Option<&lux_bytecode::Function> {
        self.module
            .functions
            .get(id.0 as usize)
            .filter(|f| f.id == id)
    }

    /// # Panics
    ///
    /// Only if called with no active frame. `execute_one` is only ever
    /// reached from `run_until_blocked` while `state == Running`, and
    /// `Running` is only ever entered once `start()` has set `frame`
    /// to `Some` — no code path clears it afterwards — so this
    /// invariant can't be violated by any bytecode, valid or not.
    fn frame_mut(&mut self) -> &mut Frame {
        self.frame.as_mut().expect(
            "execute_one requires an active frame, guaranteed by run_until_blocked's state check",
        )
    }

    fn execute_one<C: Clock>(
        &mut self,
        clock: &C,
        lighting: &mut LightingState,
    ) -> Result<Step, VmError> {
        let function_id = self.frame_mut().function;
        let pc = self.frame_mut().pc;

        let instruction = *self
            .function(function_id)
            .and_then(|f| f.code.get(pc))
            .ok_or_else(|| VmError::new(function_id, pc, VmErrorKind::InvalidProgramCounter))?;

        self.frame_mut().pc = pc + 1;

        match instruction {
            Instruction::Const(id) => self.exec_const(function_id, pc, id),
            Instruction::LoadLocal(id) => self.exec_load_local(function_id, pc, id),
            Instruction::StoreLocal(id) => self.exec_store_local(function_id, pc, id),
            Instruction::Add => self.exec_binary(function_id, pc, BinOp::Add),
            Instruction::Sub => self.exec_binary(function_id, pc, BinOp::Sub),
            Instruction::Mul => self.exec_binary(function_id, pc, BinOp::Mul),
            Instruction::Div => self.exec_binary(function_id, pc, BinOp::Div),
            Instruction::Wait => self.exec_wait(function_id, pc, clock),
            Instruction::Call(target) => self.exec_call(function_id, pc, target),
            Instruction::Return => Ok(self.exec_return()),
            Instruction::Pop => self.exec_pop(function_id, pc),
            Instruction::SetAttribute { target, attribute } => {
                self.exec_set_attribute(function_id, pc, target, attribute, lighting)
            }
        }
    }

    fn pop(&mut self, function: FunctionId, pc: usize) -> Result<Value, VmError> {
        self.stack
            .pop()
            .ok_or_else(|| VmError::new(function, pc, VmErrorKind::StackUnderflow))
    }

    fn exec_const(
        &mut self,
        function: FunctionId,
        pc: usize,
        id: lux_bytecode::ConstantId,
    ) -> Result<Step, VmError> {
        let constant = self
            .module
            .constants
            .get(id.0 as usize)
            .ok_or_else(|| VmError::new(function, pc, VmErrorKind::InvalidConstant(id)))?;
        self.stack.push(Value::from_constant(constant));
        Ok(Step::Continue)
    }

    fn exec_load_local(
        &mut self,
        function: FunctionId,
        pc: usize,
        id: lux_bytecode::LocalId,
    ) -> Result<Step, VmError> {
        let slot = self
            .frame_mut()
            .locals
            .get(id.0 as usize)
            .ok_or_else(|| VmError::new(function, pc, VmErrorKind::InvalidLocal(id)))?;
        let value =
            slot.ok_or_else(|| VmError::new(function, pc, VmErrorKind::UninitializedLocal(id)))?;
        self.stack.push(value);
        Ok(Step::Continue)
    }

    fn exec_store_local(
        &mut self,
        function: FunctionId,
        pc: usize,
        id: lux_bytecode::LocalId,
    ) -> Result<Step, VmError> {
        let value = self.pop(function, pc)?;

        let declared_ty = self
            .function(function)
            .and_then(|f| f.locals.get(id.0 as usize))
            .copied()
            .ok_or_else(|| VmError::new(function, pc, VmErrorKind::InvalidLocal(id)))?;

        if value.value_type() != declared_ty {
            return Err(VmError::new(
                function,
                pc,
                VmErrorKind::TypeMismatch {
                    expected: declared_ty,
                    found: value.value_type(),
                },
            ));
        }

        let slot = self
            .frame_mut()
            .locals
            .get_mut(id.0 as usize)
            .ok_or_else(|| VmError::new(function, pc, VmErrorKind::InvalidLocal(id)))?;
        *slot = Some(value);

        Ok(Step::Continue)
    }

    fn exec_binary(&mut self, function: FunctionId, pc: usize, op: BinOp) -> Result<Step, VmError> {
        let rhs = self.pop(function, pc)?;
        let lhs = self.pop(function, pc)?;
        let result = apply_binary(function, pc, op, lhs, rhs)?;
        self.stack.push(result);
        Ok(Step::Continue)
    }

    fn exec_wait<C: Clock>(
        &mut self,
        function: FunctionId,
        pc: usize,
        clock: &C,
    ) -> Result<Step, VmError> {
        let value = self.pop(function, pc)?;
        let Value::Duration(duration) = value else {
            return Err(VmError::new(
                function,
                pc,
                VmErrorKind::TypeMismatch {
                    expected: lux_bytecode::ValueType::Duration,
                    found: value.value_type(),
                },
            ));
        };
        Ok(Step::Wait(clock.now() + duration))
    }

    /// Pops a value, converts it to the `AttributeValue` `attribute`
    /// declares, and applies it in `lighting` — never touching DMX (see
    /// this module's docs). This is the only place the VM talks to the
    /// lighting world at all.
    fn exec_set_attribute(
        &mut self,
        function: FunctionId,
        pc: usize,
        target: lux_bytecode::TargetId,
        attribute: lux_bytecode::Attribute,
        lighting: &mut LightingState,
    ) -> Result<Step, VmError> {
        let value = self.pop(function, pc)?;
        let found = value.value_type();
        let attribute_value = value.into_attribute_value(attribute).ok_or_else(|| {
            VmError::new(
                function,
                pc,
                VmErrorKind::TypeMismatch {
                    expected: attribute.value_type(),
                    found,
                },
            )
        })?;

        let core_target = inception_core::TargetId(target.0);
        lighting
            .set_target_attribute(core_target, attribute_value)
            .map_err(|err| match err {
                inception_core::LightingError::UnknownTarget(_) => {
                    VmError::new(function, pc, VmErrorKind::UnknownTarget(target))
                }
            })?;

        Ok(Step::Continue)
    }

    fn exec_call(
        &mut self,
        function: FunctionId,
        pc: usize,
        target: FunctionId,
    ) -> Result<Step, VmError> {
        let local_count = self
            .function(target)
            .map(|f| f.locals.len())
            .ok_or_else(|| VmError::new(function, pc, VmErrorKind::InvalidFunction(target)))?;
        let caller = std::mem::replace(self.frame_mut(), Frame::new(target, local_count));
        self.call_stack.push(caller);
        Ok(Step::Continue)
    }

    /// Popping the call stack to `None` unambiguously means "the entry
    /// frame just returned": `call_stack` only ever grows on `Call` and
    /// shrinks on `Return`, and the VM starts with it empty and the
    /// entry frame active, so "empty after a pop" and "the entry frame
    /// was the one returning" are the same fact by construction. There's
    /// therefore no reachable "unexpected return" case to report here.
    fn exec_return(&mut self) -> Step {
        match self.call_stack.pop() {
            Some(caller) => {
                self.frame = Some(caller);
                Step::Continue
            }
            None => Step::Finished,
        }
    }

    fn exec_pop(&mut self, function: FunctionId, pc: usize) -> Result<Step, VmError> {
        self.pop(function, pc)?;
        Ok(Step::Continue)
    }
}

/// Executes a binary arithmetic op over two runtime values.
///
/// This mirrors `lux_bytecode::verify`'s arithmetic compatibility table
/// (which itself mirrors `lux_typeck::rules::binary_result_type`) —
/// intentionally re-expressed here rather than shared, since
/// `inception-vm` must not depend on the compiler frontend and
/// `lux-bytecode`'s verifier only checks *types*, not concrete values, so
/// it can't also do the overflow/division-by-zero checks this function
/// needs. All three tables are expected to stay in sync; an end-to-end
/// compiler test (`lux_program_waits_then_finishes` and friends) would
/// catch a real-world drift between them.
fn apply_binary(
    function: FunctionId,
    pc: usize,
    op: BinOp,
    lhs: Value,
    rhs: Value,
) -> Result<Value, VmError> {
    let overflow = || VmError::new(function, pc, VmErrorKind::ArithmeticOverflow);
    let underflow = || VmError::new(function, pc, VmErrorKind::ArithmeticUnderflow);
    let division_by_zero = || VmError::new(function, pc, VmErrorKind::DivisionByZero);
    let type_mismatch = |lhs: Value, rhs: Value| {
        VmError::new(
            function,
            pc,
            VmErrorKind::TypeMismatch {
                expected: lhs.value_type(),
                found: rhs.value_type(),
            },
        )
    };

    match (op, lhs, rhs) {
        (BinOp::Add, Value::Int(a), Value::Int(b)) => {
            a.checked_add(b).map(Value::Int).ok_or_else(overflow)
        }
        (BinOp::Sub, Value::Int(a), Value::Int(b)) => {
            a.checked_sub(b).map(Value::Int).ok_or_else(overflow)
        }
        (BinOp::Mul, Value::Int(a), Value::Int(b)) => {
            a.checked_mul(b).map(Value::Int).ok_or_else(overflow)
        }
        (BinOp::Div, Value::Int(a), Value::Int(b)) => {
            if b == 0 {
                return Err(division_by_zero());
            }
            a.checked_div(b).map(Value::Int).ok_or_else(overflow)
        }

        (BinOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (BinOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
        (BinOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        (BinOp::Div, Value::Float(a), Value::Float(b)) => {
            if b == 0.0 {
                return Err(division_by_zero());
            }
            Ok(Value::Float(a / b))
        }

        (BinOp::Add, Value::Duration(a), Value::Duration(b)) => {
            a.checked_add(b).map(Value::Duration).ok_or_else(overflow)
        }
        (BinOp::Sub, Value::Duration(a), Value::Duration(b)) => {
            a.checked_sub(b).map(Value::Duration).ok_or_else(underflow)
        }

        (BinOp::Add, Value::Intensity(a), Value::Intensity(b)) => {
            a.checked_add(b).map(Value::Intensity).ok_or_else(overflow)
        }
        (BinOp::Sub, Value::Intensity(a), Value::Intensity(b)) => {
            a.checked_sub(b).map(Value::Intensity).ok_or_else(underflow)
        }

        (_, lhs, rhs) => Err(type_mismatch(lhs, rhs)),
    }
}

/// Direct unit tests for `apply_binary`'s arithmetic table — faster and
/// more exhaustive than driving a whole `Vm` per case. End-to-end
/// behavior (`WAIT`, `Call`/`Return`, state transitions, faults on
/// malformed bytecode) is covered separately in `crate::tests`.
#[cfg(test)]
mod arithmetic_tests {
    use inception_core::Duration;
    use lux_bytecode::ValueType;

    use super::*;

    const HERE: (FunctionId, usize) = (FunctionId(0), 0);

    fn apply(op: BinOp, lhs: Value, rhs: Value) -> Result<Value, VmError> {
        apply_binary(HERE.0, HERE.1, op, lhs, rhs)
    }

    #[test]
    fn int_arithmetic() {
        assert_eq!(
            apply(BinOp::Add, Value::Int(1), Value::Int(2)),
            Ok(Value::Int(3))
        );
        assert_eq!(
            apply(BinOp::Sub, Value::Int(5), Value::Int(2)),
            Ok(Value::Int(3))
        );
        assert_eq!(
            apply(BinOp::Mul, Value::Int(3), Value::Int(4)),
            Ok(Value::Int(12))
        );
        assert_eq!(
            apply(BinOp::Div, Value::Int(10), Value::Int(4)),
            Ok(Value::Int(2))
        );
    }

    #[test]
    fn int_division_by_zero_errors() {
        assert_eq!(
            apply(BinOp::Div, Value::Int(1), Value::Int(0))
                .unwrap_err()
                .kind,
            VmErrorKind::DivisionByZero
        );
    }

    #[test]
    fn int_overflow_errors_on_every_op() {
        assert_eq!(
            apply(BinOp::Add, Value::Int(i64::MAX), Value::Int(1))
                .unwrap_err()
                .kind,
            VmErrorKind::ArithmeticOverflow
        );
        assert_eq!(
            apply(BinOp::Sub, Value::Int(i64::MIN), Value::Int(1))
                .unwrap_err()
                .kind,
            VmErrorKind::ArithmeticOverflow
        );
        assert_eq!(
            apply(BinOp::Mul, Value::Int(i64::MAX), Value::Int(2))
                .unwrap_err()
                .kind,
            VmErrorKind::ArithmeticOverflow
        );
        assert_eq!(
            apply(BinOp::Div, Value::Int(i64::MIN), Value::Int(-1))
                .unwrap_err()
                .kind,
            VmErrorKind::ArithmeticOverflow
        );
    }

    #[test]
    fn float_arithmetic() {
        assert_eq!(
            apply(BinOp::Add, Value::Float(1.5), Value::Float(2.5)),
            Ok(Value::Float(4.0))
        );
        assert_eq!(
            apply(BinOp::Div, Value::Float(1.0), Value::Float(4.0)),
            Ok(Value::Float(0.25))
        );
    }

    #[test]
    fn float_division_by_zero_errors_instead_of_producing_infinity() {
        assert_eq!(
            apply(BinOp::Div, Value::Float(1.0), Value::Float(0.0))
                .unwrap_err()
                .kind,
            VmErrorKind::DivisionByZero
        );
    }

    #[test]
    fn duration_add_and_sub() {
        assert_eq!(
            apply(
                BinOp::Add,
                Value::Duration(Duration::from_secs(1)),
                Value::Duration(Duration::from_millis(500))
            ),
            Ok(Value::Duration(Duration::from_millis(1500)))
        );
        assert_eq!(
            apply(
                BinOp::Sub,
                Value::Duration(Duration::from_secs(1)),
                Value::Duration(Duration::from_millis(500))
            ),
            Ok(Value::Duration(Duration::from_millis(500)))
        );
    }

    #[test]
    fn duration_subtraction_below_zero_underflows() {
        let err = apply(
            BinOp::Sub,
            Value::Duration(Duration::from_millis(500)),
            Value::Duration(Duration::from_secs(1)),
        )
        .unwrap_err();
        assert_eq!(err.kind, VmErrorKind::ArithmeticUnderflow);
    }

    #[test]
    fn duration_multiplication_is_not_defined() {
        let err = apply(
            BinOp::Mul,
            Value::Duration(Duration::from_secs(1)),
            Value::Duration(Duration::from_secs(1)),
        )
        .unwrap_err();
        assert_eq!(
            err.kind,
            VmErrorKind::TypeMismatch {
                expected: ValueType::Duration,
                found: ValueType::Duration
            }
        );
    }

    #[test]
    fn intensity_add_and_sub() {
        assert_eq!(
            apply(BinOp::Add, Value::Intensity(100), Value::Intensity(200)),
            Ok(Value::Intensity(300))
        );
        assert_eq!(
            apply(BinOp::Sub, Value::Intensity(200), Value::Intensity(100)),
            Ok(Value::Intensity(100))
        );
    }

    #[test]
    fn intensity_overflow_and_underflow_error() {
        assert_eq!(
            apply(BinOp::Add, Value::Intensity(u16::MAX), Value::Intensity(1))
                .unwrap_err()
                .kind,
            VmErrorKind::ArithmeticOverflow
        );
        assert_eq!(
            apply(BinOp::Sub, Value::Intensity(0), Value::Intensity(1))
                .unwrap_err()
                .kind,
            VmErrorKind::ArithmeticUnderflow
        );
    }

    #[test]
    fn cross_type_arithmetic_is_a_type_mismatch() {
        let err = apply(
            BinOp::Add,
            Value::Duration(Duration::ZERO),
            Value::Intensity(0),
        )
        .unwrap_err();
        assert_eq!(
            err.kind,
            VmErrorKind::TypeMismatch {
                expected: ValueType::Duration,
                found: ValueType::Intensity
            }
        );
    }
}
