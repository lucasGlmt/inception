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

use inception_core::{Duration, LightingState, Timestamp, TransitionEngine, VirtualClock};
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
