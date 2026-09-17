//! Structured VM errors.
//!
//! `lux_bytecode::verify` already statically guarantees a verified
//! module can't produce these — but `Vm` must not simply trust that
//! every module it's handed was actually verified (see `Vm::new`'s
//! docs), so every execution-time bounds/type check still has a real
//! failure path here instead of a `panic!`/`unwrap()`/`unreachable!()`.

use lux_bytecode::{ConstantId, FunctionId, LocalId, TargetId, ValueType};

/// Failure to even construct a runnable [`crate::Vm`] — distinct from
/// [`VmError`] (a runtime failure *during* execution) because it isn't
/// tied to a function/instruction: there is no frame yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmInitError {
    /// The module failed static verification; see `lux_bytecode::verify`.
    Verification(Vec<lux_bytecode::VerificationError>),
    /// The module has no `entry` function to start execution from.
    NoEntryPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmError {
    pub function: FunctionId,
    pub instruction: usize,
    pub kind: VmErrorKind,
}

impl VmError {
    pub(crate) fn new(function: FunctionId, instruction: usize, kind: VmErrorKind) -> Self {
        Self {
            function,
            instruction,
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmErrorKind {
    /// A `Call`/entry `FunctionId` doesn't exist in the module.
    InvalidFunction(FunctionId),
    /// A `LoadLocal`/`StoreLocal` `LocalId` is out of range for the
    /// current frame.
    InvalidLocal(LocalId),
    /// A local was read (`LoadLocal`) before ever being written.
    UninitializedLocal(LocalId),
    /// A `Const` `ConstantId` is out of range for the module's constant
    /// pool.
    InvalidConstant(ConstantId),
    /// Popped from an empty operand stack.
    StackUnderflow,
    /// An instruction's operand(s) didn't have the type it expected —
    /// e.g. `Wait` popped a non-`Duration`, or an arithmetic op popped a
    /// combination it doesn't support.
    TypeMismatch {
        expected: ValueType,
        found: ValueType,
    },
    DivisionByZero,
    /// A result didn't fit in its value's representable range (e.g.
    /// `i64::MAX + 1`, or an `Intensity` sum past `65535`).
    ArithmeticOverflow,
    /// A result would be negative in a domain that can't represent one
    /// (e.g. `500ms - 1s` for `Duration`).
    ArithmeticUnderflow,
    /// The program counter ran off the end of the current function's
    /// code without hitting `Return`.
    InvalidProgramCounter,
    /// `Return` executed with no caller frame to return to, other than
    /// the entry frame (which instead finishes the VM) — reachable only
    /// through a malformed/hand-built module, since verified bytecode's
    /// call stack always matches its `Call`s.
    UnexpectedReturn,
    /// `SetAttribute` referenced a target the `LightingState` it was
    /// given doesn't know about (see `inception_core::LightingError`).
    /// `lux_bytecode::verify` only bounds-checks the `TargetId` against
    /// the module's declared `target_count` — it can't know whether the
    /// caller's `LightingState` actually defined that target, since that
    /// wiring happens outside the module entirely.
    UnknownTarget(TargetId),
    UnsupportedTransitionAttribute,
    InvalidTransitionValue,
    ClockOverflow,
    /// `BIND_SIGNAL`'s operand referenced a `SignalId` this `Vm`'s
    /// `SignalStore` doesn't know about — unreachable for a verified
    /// module (every `Signal` value on the stack was produced by this
    /// same `Vm`'s own `SignalConstant*` handling), kept only for the
    /// same defensive reason every other `VmErrorKind` exists.
    UnknownSignal(crate::signal::SignalId),
    /// An `Effects*` oscillator (`Sine`/`Triangle`/`Saw`/`Square`) was
    /// constructed with a zero `period`. A *constant* zero period is
    /// already rejected at compile time (see `lux-typeck`'s
    /// `check_literal_constant_misuse`); this is the runtime backstop for
    /// a non-constant `Duration` (e.g. a `let`-bound variable) that turns
    /// out to be zero when the oscillator is actually constructed —
    /// dividing by it would be a division by zero, so this is reported as
    /// a structured error instead.
    InvalidSignalPeriod,
    /// A `Phase` signal's source doesn't resolve to a base oscillator —
    /// unreachable from real Lux source (see
    /// `inception_vm::signal::SignalError::UnsupportedPhaseSource`'s
    /// docs), kept only for the same defensive reason every other
    /// `VmErrorKind` exists.
    UnsupportedPhaseSource(crate::signal::SignalId),
    /// A `Spread` signal's source doesn't resolve to a base oscillator
    /// (directly, or through one `Phase`) — unreachable from real Lux
    /// source (see
    /// `inception_vm::signal::SignalError::UnsupportedSpreadSource`'s
    /// docs), kept only for the same defensive reason every other
    /// `VmErrorKind` exists.
    UnsupportedSpreadSource(crate::signal::SignalId),
}
