//! End-to-end `Vm` behavior, driven by hand-built `BytecodeModule`s so the
//! VM can be tested in isolation from `lux-compiler`. See
//! `crate::vm::arithmetic_tests` for direct, exhaustive arithmetic-table
//! tests.

use inception_core::{
    Clock, Duration as CoreDuration, LightingState, Timestamp, TransitionEngine, VirtualClock,
};
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
        target_count: 0,
        rig_contract: None,
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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_waiting());
    assert_eq!(vm.stack(), &[Value::Int(42)]);
}

#[test]
fn call_intrinsic_signal_constant_pushes_a_sampleable_signal() {
    // CONST Intensity(50%); CALL_INTRINSIC SignalConstantIntensity(1);
    // CONST Duration(1s); WAIT; POP; RETURN
    let module = module_with(
        vec![
            Constant::Intensity(32767),
            Constant::Duration(1_000_000_000),
        ],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting());

    let &[Value::Signal(elem, id)] = vm.stack() else {
        panic!(
            "expected exactly one Signal value on the stack, got {:?}",
            vm.stack()
        );
    };
    assert_eq!(elem, lux_bytecode::ScalarValueType::Intensity);

    for at in [
        Timestamp::ZERO,
        Timestamp::from_secs(1),
        Timestamp::from_secs(3600),
    ] {
        assert_eq!(vm.signals().sample(id, at), Ok(Value::Intensity(32767)));
    }
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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert_eq!(vm.state(), VmState::WaitingUntil(Timestamp::from_secs(1)));

    // Half the wait elapsed: still blocked, no partial progress.
    clock.advance(CoreDuration::from_millis(500));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting());
    assert_eq!(vm.state(), VmState::WaitingUntil(Timestamp::from_secs(1)));

    // The rest elapses: resumes exactly after WAIT, then hits RETURN.
    clock.advance(CoreDuration::from_millis(500));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting());

    // The runtime "polls late" by 5s when only 1s was needed.
    clock.advance(CoreDuration::from_secs(5));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    let err = vm
        .run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap_err();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    let err = vm
        .run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap_err();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

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
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert_eq!(vm.state(), VmState::Ready);
}

fn intensity_set_attribute_module() -> BytecodeModule {
    let mut module = module_with(
        vec![Constant::Intensity(32767)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::SetAttribute {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    module.target_count = 1;
    module
}

#[test]
fn set_attribute_propagates_to_every_fixture_the_target_resolves_to() {
    let mut vm = started(intensity_set_attribute_module());
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![inception_core::FixtureId(0), inception_core::FixtureId(1)],
        },
    );

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_finished());
    let expected = inception_core::Intensity::new(32767);
    assert_eq!(lighting.intensity(inception_core::FixtureId(0)), expected);
    assert_eq!(lighting.intensity(inception_core::FixtureId(1)), expected);
    // A fixture not part of the target is left untouched.
    assert_eq!(
        lighting.intensity(inception_core::FixtureId(2)),
        inception_core::Intensity::ZERO
    );
}

#[test]
fn set_attribute_to_an_unknown_target_is_a_structured_error() {
    let mut vm = started(intensity_set_attribute_module());
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    // No target defined at all: TargetId(0) is unknown to `lighting`,
    // even though it's in range per the module's own `target_count`.

    let err = vm
        .run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap_err();

    assert_eq!(
        err.kind,
        VmErrorKind::UnknownTarget(lux_bytecode::TargetId(0))
    );
}

// A `SetAttribute` whose popped value doesn't match its declared
// attribute (e.g. a `Duration` for `Intensity`) is already rejected by
// `lux_bytecode::verify` — so `Vm::new` refuses that module outright,
// the same way `malformed_hand_built_module_is_rejected_at_construction_not_at_runtime`
// shows for `LoadLocal`. There is therefore no verified module that
// reaches `Vm::exec_set_attribute`'s own defensive type check; that
// logic (`Value::into_attribute_value`) is unit-tested directly in
// `crate::value::tests` instead.

fn bind_intensity_signal_module(value: Constant, target: u32) -> BytecodeModule {
    let mut module = module_with(
        vec![value],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(target),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    module.target_count = target + 1;
    module
}

fn define_two_fixture_target(lighting: &mut LightingState) {
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![inception_core::FixtureId(0), inception_core::FixtureId(1)],
        },
    );
}

/// Item 55: after `Front.intensity <- Signal.constant(50%);` runs, every
/// fixture the target resolves to has the binding — and, once the
/// runtime's per-frame `sample_signal_bindings` follow-up call runs (as
/// `inception_runtime::LoadedProgram::advance` always does, same tick),
/// the sampled value lands in `LightingState` too.
#[test]
fn bind_signal_creates_a_binding_for_every_fixture_in_the_target() {
    let mut vm = started(bind_intensity_signal_module(Constant::Intensity(32767), 0));
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    define_two_fixture_target(&mut lighting);

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());

    assert_eq!(vm.bindings().active_count(), 2);
    let fixture_a = inception_core::FixtureId(0);
    let fixture_b = inception_core::FixtureId(1);
    let signal_a = vm
        .bindings()
        .get(fixture_a, inception_core::Attribute::Intensity)
        .expect("fixture 0 should be bound");
    let signal_b = vm
        .bindings()
        .get(fixture_b, inception_core::Attribute::Intensity)
        .expect("fixture 1 should be bound");
    assert_eq!(signal_a, signal_b, "both fixtures share the same signal");

    vm.sample_signal_bindings(clock.now(), &mut lighting)
        .unwrap();
    let expected = inception_core::Intensity::new(32767);
    assert_eq!(lighting.intensity(fixture_a), expected);
    assert_eq!(lighting.intensity(fixture_b), expected);
}

/// Item 56: `bind A; bind B;` on the same `(fixture, attribute)` leaves
/// only `B` active.
#[test]
fn a_second_bind_signal_replaces_the_first_on_the_same_key() {
    let module = module_with(
        vec![Constant::Intensity(30000), Constant::Intensity(10000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Const(ConstantId(1)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    let mut vm = started({
        let mut m = module;
        m.target_count = 1;
        m
    });
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![inception_core::FixtureId(0)],
        },
    );

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());
    assert_eq!(vm.bindings().active_count(), 1);

    vm.sample_signal_bindings(clock.now(), &mut lighting)
        .unwrap();
    assert_eq!(
        lighting.intensity(inception_core::FixtureId(0)),
        inception_core::Intensity::new(10000)
    );
}

/// Item 57: an immediate `=` on the same `(fixture, attribute)` detaches
/// the signal binding — after that, resampling bindings must not bring
/// the old value back.
#[test]
fn set_attribute_detaches_an_active_signal_binding() {
    let module = module_with(
        vec![Constant::Intensity(40000), Constant::Intensity(20000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Const(ConstantId(1)),
                Instruction::SetAttribute {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![],
            1,
        )],
    );
    let mut vm = started({
        let mut m = module;
        m.target_count = 1;
        m
    });
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let fixture = inception_core::FixtureId(0);
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![fixture],
        },
    );

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());

    assert_eq!(vm.bindings().active_count(), 0);
    assert_eq!(
        lighting.intensity(fixture),
        inception_core::Intensity::new(20000)
    );

    // Resampling bindings (as the runtime does every frame) must be a
    // no-op now: the old signal is no longer registered anywhere.
    vm.sample_signal_bindings(clock.now(), &mut lighting)
        .unwrap();
    assert_eq!(
        lighting.intensity(fixture),
        inception_core::Intensity::new(20000)
    );
}

/// Items 28/29/58: a `->` transition on the same `(fixture, attribute)`
/// as an active signal binding must detach the binding and start the
/// transition from the signal's own sampled value at `now` — never from
/// `LightingState`'s untouched default (`Intensity::ZERO`).
#[test]
fn transition_attribute_detaches_signal_and_starts_from_its_sampled_value() {
    let module = module_with(
        vec![
            Constant::Intensity(16384),
            Constant::Intensity(u16::MAX),
            Constant::Duration(2_000_000_000),
        ],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Const(ConstantId(1)),
                Instruction::Const(ConstantId(2)),
                Instruction::TransitionAttribute {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![],
            2,
        )],
    );
    let mut vm = started({
        let mut m = module;
        m.target_count = 1;
        m
    });
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let fixture = inception_core::FixtureId(0);
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![fixture],
        },
    );

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());

    assert_eq!(vm.bindings().active_count(), 0, "signal should be detached");
    assert_eq!(transitions.active_count(), 1);

    // `now` never advanced during this single run, so the transition's
    // `starts_at` equals `now`: sampling it right at that instant must
    // return exactly the signal's own value (16384), not `Intensity::ZERO`.
    transitions.sample(clock.now(), &mut lighting).unwrap();
    assert_eq!(
        lighting.intensity(fixture),
        inception_core::Intensity::new(16384)
    );

    transitions
        .sample(clock.now() + CoreDuration::from_secs(2), &mut lighting)
        .unwrap();
    assert_eq!(lighting.intensity(fixture), inception_core::Intensity::MAX);
}

/// Items 30/31/59: binding a signal onto a fixture with an active
/// transition detaches the transition (sampled at `now` first, per
/// `TransitionEngine::cancel`) and, once the runtime's per-frame
/// `sample_signal_bindings` follow-up runs, the signal's value wins
/// immediately — there is no crossfade between the stabilized transition
/// value and the new signal.
#[test]
fn bind_signal_detaches_an_active_transition_and_wins_immediately() {
    let module = module_with(
        vec![
            Constant::Intensity(u16::MAX),
            Constant::Duration(10_000_000_000),
            Constant::Duration(5_000_000_000),
            Constant::Intensity(20000),
        ],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::Const(ConstantId(1)),
                Instruction::TransitionAttribute {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Const(ConstantId(2)),
                Instruction::Wait,
                Instruction::Const(ConstantId(3)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![],
            2,
        )],
    );
    let mut vm = started({
        let mut m = module;
        m.target_count = 1;
        m
    });
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let fixture = inception_core::FixtureId(0);
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![fixture],
        },
    );

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting(), "should block on the 5s WAIT");
    assert_eq!(transitions.active_count(), 1);

    clock.advance(CoreDuration::from_secs(5));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());

    assert_eq!(transitions.active_count(), 0, "transition should be gone");
    assert_eq!(vm.bindings().active_count(), 1);
    // Roughly halfway through 0..MAX over 10s at t=5s — the exact
    // stabilized transition value, before the signal takes over.
    let halfway = lighting.intensity(fixture).raw();
    assert!(
        (30000..35000).contains(&halfway),
        "expected an interpolated halfway value, got {halfway}"
    );

    // The runtime's same-tick follow-up: this is what makes the signal
    // "win immediately" (item 31) in `inception_runtime::LoadedProgram::advance`.
    vm.sample_signal_bindings(clock.now(), &mut lighting)
        .unwrap();
    assert_eq!(
        lighting.intensity(fixture),
        inception_core::Intensity::new(20000)
    );
}

/// Item 63: two targets bound to the same signal share the `SignalId`
/// rather than each getting their own copy of the definition.
#[test]
fn a_signal_can_be_shared_across_multiple_targets() {
    let module = module_with(
        vec![Constant::Intensity(25000)],
        vec![function(
            0,
            vec![
                Instruction::Const(ConstantId(0)),
                Instruction::CallIntrinsic {
                    intrinsic: lux_bytecode::IntrinsicId::SignalConstantIntensity,
                    arg_count: 1,
                },
                Instruction::StoreLocal(LocalId(0)),
                Instruction::LoadLocal(LocalId(0)),
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::LoadLocal(LocalId(0)),
                Instruction::BindSignal {
                    target: lux_bytecode::TargetId(1),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            vec![ValueType::Signal(lux_bytecode::ScalarValueType::Intensity)],
            2,
        )],
    );
    let mut vm = started({
        let mut m = module;
        m.target_count = 2;
        m
    });
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let front = inception_core::FixtureId(0);
    let back = inception_core::FixtureId(1);
    lighting.define_target(
        inception_core::TargetId(0),
        inception_core::ResolvedTarget {
            fixtures: vec![front],
        },
    );
    lighting.define_target(
        inception_core::TargetId(1),
        inception_core::ResolvedTarget {
            fixtures: vec![back],
        },
    );

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());

    let signal_front = vm
        .bindings()
        .get(front, inception_core::Attribute::Intensity)
        .unwrap();
    let signal_back = vm
        .bindings()
        .get(back, inception_core::Attribute::Intensity)
        .unwrap();
    assert_eq!(signal_front, signal_back);

    vm.sample_signal_bindings(clock.now(), &mut lighting)
        .unwrap();
    let expected = inception_core::Intensity::new(25000);
    assert_eq!(lighting.intensity(front), expected);
    assert_eq!(lighting.intensity(back), expected);
}
