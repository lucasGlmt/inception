//! End-to-end tests: real Lux source, compiled by `lux-compiler`,
//! executed by `inception-vm`. `inception-vm`'s own library code never
//! depends on the compiler (see `crate::vm`'s docs) — this is a
//! dev-dependency, used only here to prove the whole pipeline sketched in
//! the task brief actually holds together:
//!
//! ```text
//! Lux source -> lux-compiler -> BytecodeModule -> lux_bytecode::verify
//!            -> Inception VM -> VirtualClock -> Finished
//! ```
//!
//! `lighting_pipeline.rs` in this same directory takes this one step
//! further, all the way through to a rendered DMX frame.

use inception_core::{
    Duration, FixtureId, LightingState, ResolvedTarget, Rgb, TargetId, Timestamp, TransitionEngine,
    VirtualClock,
};
use inception_vm::Vm;
use lux_compiler::TargetEnvironment;

#[test]
fn lux_program_waits_then_finishes() {
    let source = r#"
        scene main {
            let duration: Duration = 1s;
            wait duration;
        }
    "#;

    let module = lux_compiler::compile(source, &TargetEnvironment::new()).unwrap();

    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let mut vm = Vm::new(module).unwrap();

    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_waiting());

    clock.advance(Duration::from_secs(1));

    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_finished());
}

#[test]
fn compiler_task_brief_worked_example_runs_to_completion() {
    // The exact program from the compiler milestone's success criterion,
    // now taken all the way through to VM execution: `t=0` blocks until
    // 1.5s, `t=1.0s` is still too early, `t=1.5s` finishes.
    let source = r#"
        scene main {
            let first: Duration = 1s;
            let second: Duration = 500ms;
            let duration = first + second;

            wait duration;
        }
    "#;

    let module = lux_compiler::compile(source, &TargetEnvironment::new()).unwrap();
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let mut vm = Vm::new(module).unwrap();

    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting());
    assert_eq!(
        vm.state(),
        inception_vm::VmState::WaitingUntil(Timestamp::from_millis(1500))
    );

    clock.advance(Duration::from_secs(1));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting(), "1.0s elapsed but the wake-up time is 1.5s");

    clock.advance(Duration::from_millis(499));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_waiting(), "1.499s elapsed, still short of 1.5s");

    clock.advance(Duration::from_millis(1));
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished(), "1.500s elapsed, the wait is over");
}

#[test]
fn stdlib_module_worked_example_runs_to_completion() {
    // The module-system milestone's own end-to-end success criterion:
    // imports, a qualified `Math.sin` call, and a `Color.rgb` call feeding
    // a rig-contract-declared target, taken all the way through parsing,
    // module resolution, HIR, type checking, MIR, bytecode, the verifier
    // and VM execution.
    let source = r#"
        import std.Math;
        import std.Color;

        rig contract DemoRig {
            role Washes: Group<Color>;
        }

        scene main {
            let wave_point = Math.sin(90deg);
            let orange = Color.rgb(255, 120, 20);

            Washes.color = orange;
        }
    "#;

    let module = lux_compiler::compile_portable(source).expect("worked example should compile");

    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    lighting.define_target(
        TargetId(0),
        ResolvedTarget {
            fixtures: vec![FixtureId(0)],
        },
    );
    let mut transitions = TransitionEngine::new();
    let mut vm = Vm::new(module).unwrap();

    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_finished());
    assert_eq!(
        lighting.color(FixtureId(0)),
        Rgb {
            red: 255 * 257,
            green: 120 * 257,
            blue: 20 * 257,
        }
    );
}

#[test]
fn signal_constant_program_compiles_and_runs_to_completion() {
    // The Signal<T> milestone's own success criterion (see
    // docs/rfcs/0002-signal-type.md): a `Signal<Intensity>` local, built
    // from `Signal.constant`, taken all the way through parsing, HIR,
    // type checking, MIR, bytecode, the verifier and VM execution. This
    // milestone doesn't wire a signal into any lighting attribute yet, so
    // there's no DMX/attribute assertion to make here — see
    // `inception_vm::signal`'s unit tests for the deterministic-sampling
    // behavior itself, and `call_intrinsic_signal_constant_pushes_a_sampleable_signal`
    // in `inception-vm`'s own test suite for sampling driven through a
    // real `Vm`.
    let source = r#"
        import std.Signal;

        scene main {
            let level: Signal<Intensity> = Signal.constant(50%);
        }
    "#;

    let module = lux_compiler::compile_portable(source).expect("signal program should compile");

    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let mut vm = Vm::new(module).unwrap();

    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_finished());
}

#[test]
fn a_scene_with_no_wait_finishes_on_the_first_run() {
    let source = r#"
        scene main {
            let x = 1 + 2;
        }
    "#;

    let module = lux_compiler::compile(source, &TargetEnvironment::new()).unwrap();
    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let mut vm = Vm::new(module).unwrap();

    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    assert!(vm.is_finished());
}
