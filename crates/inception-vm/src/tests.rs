//! End-to-end `Vm` behavior, driven by hand-built `BytecodeModule`s so the
//! VM can be tested in isolation from `lux-compiler`. See
//! `crate::vm::arithmetic_tests` for direct, exhaustive arithmetic-table
//! tests.

use inception_core::{Duration as CoreDuration, Timestamp, VirtualClock};
use lux_bytecode::{
    BytecodeModule, BytecodeVersion, ColorValue, Constant, ConstantId, Function, FunctionId,
    Instruction, LocalId, ValueType,
};

use crate::error::{VmErrorKind, VmInitError};
use crate::state::VmState;
use crate::value::Value;
use crate::vm::Vm;

fn module_with(constants: Vec<Constant>, functions: Vec<Function>) -> BytecodeModule {
    BytecodeModule {
        version: BytecodeVersion::CURRENT,
        constants,
        functions,
        entry: Some(FunctionId(0)),
    }
}

fn function(id: u32, code: Vec<Instruction>, locals: Vec<ValueType>, max_stack: u16) -> Function {
    Function {
        id: FunctionId(id),
        code,
        locals,
        max_stack,
        debug_name: None,
    }
}

fn started(module: BytecodeModule) -> Vm {
    let mut vm = Vm::new(module).expect("module should verify");
    vm.start().expect("entry function should be valid");
    vm
}

#[test]
fn immediate_return_finishes() {
    let module = module_with(
        vec![],
        vec![function(0, vec![Instruction::Return], vec![], 0)],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert!(vm.is_finished());
}

#[test]
fn const_and_local_round_trip_the_correct_value() {
    // CONST 42; STORE_LOCAL 0; LOAD_LOCAL 0; CONST Duration(1s); WAIT; POP; RETURN
    //
    // Blocking on a *valid* WAIT afterwards, with the loaded value still
    // sitting below the popped duration, lets us inspect the exact value
    // that made it through STORE_LOCAL/LOAD_LOCAL via `Vm::stack()` —
    // without faulting (a fault would itself have already popped the
    // value being checked). The trailing POP/RETURN exist only so the
    // module still satisfies the bytecode verifier's "empty stack at
    // RETURN" rule; this test never reaches them.
    let module = module_with(
        vec![Constant::Int(42), Constant::Duration(1_000_000_000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::StoreLocal(LocalId(0)),
                Instruction::LoadLocal(LocalId(0)),
                Instruction::Const(ConstantId(1)),
                Instruction::Wait,
                Instruction::Pop,
                Instruction::Return,
            ],
            vec![ValueType::Int],
            2,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert!(vm.is_waiting());
    assert_eq!(vm.stack(), &[Value::Int(42)]);
}

#[test]
fn arithmetic_result_feeds_correctly_into_a_later_instruction() {
    // CONST 1; CONST 2; ADD; STORE_LOCAL 0; LOAD_LOCAL 0; CONST Duration(1s); WAIT; POP; RETURN
    let module = module_with(
        vec![
            Constant::Int(1),
            Constant::Int(2),
            Constant::Duration(1_000_000_000),
        ],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Const(ConstantId(1)),
                Instruction::Add,
                Instruction::StoreLocal(LocalId(0)),
                Instruction::LoadLocal(LocalId(0)),
                Instruction::Const(ConstantId(2)),
                Instruction::Wait,
                Instruction::Pop,
                Instruction::Return,
            ],
            vec![ValueType::Int],
            2,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert!(vm.is_waiting());
    assert_eq!(vm.stack(), &[Value::Int(3)]);
}

#[test]
fn call_then_return_finishes_normally() {
    let main = function(
        0,
        vec![Instruction::Call(FunctionId(1)), Instruction::Return],
        vec![],
        0,
    );
    let callee = function(1, vec![Instruction::Return], vec![], 0);

    let module = module_with(vec![], vec![main, callee]);
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert!(vm.is_finished());
}

#[test]
fn wait_blocks_then_resumes_exactly_when_time_passes() {
    let module = module_with(
        vec![Constant::Duration(1_000_000_000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Wait,
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();
    assert_eq!(vm.state(), VmState::WaitingUntil(Timestamp::from_secs(1)));

    // Half the wait elapsed: still blocked, no partial progress.
    clock.advance(CoreDuration::from_millis(500));
    vm.run_until_blocked(&clock).unwrap();
    assert!(vm.is_waiting());
    assert_eq!(vm.state(), VmState::WaitingUntil(Timestamp::from_secs(1)));

    // The rest elapses: resumes exactly after WAIT, then hits RETURN.
    clock.advance(CoreDuration::from_millis(500));
    vm.run_until_blocked(&clock).unwrap();
    assert!(vm.is_finished());
}

#[test]
fn a_late_runtime_does_not_try_to_catch_up() {
    let module = module_with(
        vec![Constant::Duration(1_000_000_000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Wait,
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();
    assert!(vm.is_waiting());

    // The runtime "polls late" by 5s when only 1s was needed.
    clock.advance(CoreDuration::from_secs(5));
    vm.run_until_blocked(&clock).unwrap();

    // Resumes once and finishes directly — no intermediate ticks, no
    // attempt to simulate the 4 "missed" seconds.
    assert!(vm.is_finished());
}

#[test]
fn division_by_zero_is_a_structured_error_not_a_panic() {
    let module = module_with(
        vec![Constant::Int(1), Constant::Int(0)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Const(ConstantId(1)),
                Instruction::Div,
                Instruction::Pop,
                Instruction::Return,
            ],
            vec![],
            2,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    let err = vm.run_until_blocked(&clock).unwrap_err();

    assert_eq!(err.kind, VmErrorKind::DivisionByZero);
    assert!(vm.is_faulted());
    assert_eq!(vm.state(), VmState::Faulted(err));
}

#[test]
fn reading_an_uninitialized_local_is_a_structured_error() {
    let module = module_with(
        vec![],
        vec![function(
            0,
            vec![
                Instruction::LoadLocal(LocalId(0)),
                Instruction::Pop,
                Instruction::Return,
            ],
            vec![ValueType::Int],
            1,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    let err = vm.run_until_blocked(&clock).unwrap_err();

    assert_eq!(err.kind, VmErrorKind::UninitializedLocal(LocalId(0)));
    assert_eq!(err.function, FunctionId(0));
    assert_eq!(err.instruction, 0);
}

#[test]
fn malformed_hand_built_module_is_rejected_at_construction_not_at_runtime() {
    // LOAD_LOCAL 0 with zero declared locals: caught by
    // `lux_bytecode::verify` inside `Vm::new`, before the VM ever runs a
    // single instruction.
    let module = module_with(
        vec![],
        vec![function(
            0,
            vec![Instruction::LoadLocal(LocalId(0)), Instruction::Return],
            vec![],
            1,
        )],
    );

    let err = Vm::new(module).unwrap_err();

    assert!(matches!(err, VmInitError::Verification(_)));
}

#[test]
fn module_without_entry_point_is_rejected() {
    let mut module = module_with(
        vec![],
        vec![function(0, vec![Instruction::Return], vec![], 0)],
    );
    module.entry = None;

    let err = Vm::new(module).unwrap_err();

    assert_eq!(err, VmInitError::NoEntryPoint);
}

#[test]
fn colors_round_trip_through_the_stack() {
    let module = module_with(
        vec![
            Constant::Color(ColorValue { r: 255, g: 0, b: 0 }),
            Constant::Duration(1_000_000_000),
        ],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Const(ConstantId(1)),
                Instruction::Wait,
                Instruction::Pop,
                Instruction::Return,
            ],
            vec![],
            2,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert!(vm.is_waiting());
    assert_eq!(
        vm.stack(),
        &[Value::Color(ColorValue { r: 255, g: 0, b: 0 })]
    );
}

#[test]
fn debug_inspection_reflects_the_resume_point_after_wait() {
    let module = module_with(
        vec![Constant::Duration(1_000_000_000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Wait,
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    let mut vm = started(module);
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert_eq!(vm.stack(), &[] as &[Value]);
    assert_eq!(vm.current_function(), Some(FunctionId(0)));
    // WAIT is instruction index 1; the frame's PC already advanced past
    // it before blocking, so it resumes at RETURN (index 2).
    assert_eq!(vm.current_pc(), Some(2));
}

#[test]
fn never_started_vm_run_is_a_harmless_no_op() {
    let module = module_with(
        vec![],
        vec![function(0, vec![Instruction::Return], vec![], 0)],
    );
    let mut vm = Vm::new(module).unwrap();
    let clock = VirtualClock::new();

    vm.run_until_blocked(&clock).unwrap();

    assert_eq!(vm.state(), VmState::Ready);
}
