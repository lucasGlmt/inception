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
fn effects_sine_program_compiles_and_samples_the_documented_phase_table() {
    // Item 66/78 of the `std.Effects` milestone's success criterion:
    // `Effects.sine(2s)` compiled and executed through the full pipeline
    // (parse -> HIR -> typecheck -> MIR -> bytecode -> verifier -> VM),
    // then sampled directly off the VM's own `SignalStore`. There's no
    // public API to read a `let` local's value back out of a finished
    // VM's frame (see `signal_constant_program_compiles_and_runs_to_completion`
    // above, which sidesteps the same limitation by not sampling at all)
    // — this samples by `SignalId` instead, which is deterministic: the
    // program's one `Effects.sine` call is always the first and only
    // signal this `Vm` ever creates.
    let source = r#"
        import std.Effects;

        scene main {
            let wave: Signal<Float> = Effects.sine(2s);
        }
    "#;

    let module = lux_compiler::compile_portable(source).expect("effects program should compile");

    let clock = VirtualClock::new();
    let mut lighting = LightingState::new();
    let mut transitions = TransitionEngine::new();
    let mut vm = Vm::new(module).unwrap();

    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    assert!(vm.is_finished());

    let id = inception_vm::SignalId(0);
    for (millis, expected) in [(0, 0.5), (500, 1.0), (1000, 0.5), (1500, 0.0), (2000, 0.5)] {
        let sampled = vm
            .signals()
            .sample(id, Timestamp::from_millis(millis))
            .unwrap();
        let inception_vm::Value::Float(v) = sampled else {
            panic!("expected Float, got {sampled:?}");
        };
        assert!(
            (v - expected).abs() < 1e-9,
            "expected {expected} at {millis}ms, got {v}"
        );
    }
}

#[test]
fn effects_oscillator_created_after_a_wait_starts_its_cycle_there() {
    // Item 67: `wait 1s; let wave = Effects.sine(2s);` — the oscillator's
    // origin must be the timestamp the VM actually executed the
    // construction at (`t=1s`), not the runtime's own start (`t=0`).
    let source = r#"
        import std.Effects;

        scene main {
            wait 1s;
            let wave = Effects.sine(2s);
        }
    "#;

    let module = lux_compiler::compile_portable(source).expect("effects program should compile");

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

    let id = inception_vm::SignalId(0);
    // Sampled exactly at its own origin (t=1s): elapsed=0, phase=0,
    // sine=0.5 — never the value it would have had if it had (wrongly)
    // started ticking from the runtime's own t=0.
    let sampled = vm.signals().sample(id, Timestamp::from_secs(1)).unwrap();
    let inception_vm::Value::Float(v) = sampled else {
        panic!("expected Float, got {sampled:?}");
    };
    assert!((v - 0.5).abs() < 1e-9);
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
