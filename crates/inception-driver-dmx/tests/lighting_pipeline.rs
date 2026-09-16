//! The full virtual lighting pipeline, end to end:
//!
//! ```text
//! Lux source -> lux-compiler -> BytecodeModule -> lux_bytecode::verify
//!            -> Inception VM -> LightingState -> inception-renderer
//!            -> RecordingDmxOutput
//! ```
//!
//! No hardware, no real DMX, no real time: this is the milestone's major
//! success criterion (item 44 of the task brief) — a Lux program setting
//! a semantic attribute must end up as concrete DMX bytes.

use std::collections::HashMap;

use inception_core::{
    FixtureId, LightingState, ResolvedTarget, TargetId, UniverseId, VirtualClock,
};
use inception_driver_dmx::{DmxOutput, RecordingDmxOutput};
use inception_renderer::{DmxChannel, DmxChannelMapping, ResolvedFixture, ResolvedRig};
use inception_vm::Vm;
use lux_compiler::TargetEnvironment;

/// The test environment from item 34 of the task brief:
///
/// ```text
/// Washes -> Target #0
/// Target #0: Fixture #0, Fixture #1
/// Fixture #0: intensity -> U1 channel 1
/// Fixture #1: intensity -> U1 channel 5
/// ```
///
/// Built by hand here, standing in for the rig/patch/linker this
/// milestone doesn't have yet — see `TargetEnvironment`'s and
/// `ResolvedRig`'s docs for why that's an intentional, temporary
/// simplification rather than an oversight.
struct TestRig {
    targets: TargetEnvironment,
    lighting: LightingState,
    rig: ResolvedRig,
}

fn washes_two_fixtures_one_universe() -> TestRig {
    let mut targets = TargetEnvironment::new();
    let washes = targets.insert("Washes");
    assert_eq!(washes, lux_compiler::TargetId(0));

    let mut lighting = LightingState::new();
    lighting.define_target(
        TargetId(0),
        ResolvedTarget {
            fixtures: vec![FixtureId(0), FixtureId(1)],
        },
    );

    let rig = ResolvedRig {
        fixtures: vec![
            ResolvedFixture {
                id: FixtureId(0),
                intensity: Some(DmxChannelMapping {
                    universe: UniverseId(1),
                    channel: DmxChannel::new(1).unwrap(),
                }),
                color: None,
            },
            ResolvedFixture {
                id: FixtureId(1),
                intensity: Some(DmxChannelMapping {
                    universe: UniverseId(1),
                    channel: DmxChannel::new(5).unwrap(),
                }),
                color: None,
            },
        ],
    };

    TestRig {
        targets,
        lighting,
        rig,
    }
}

#[test]
fn lux_attribute_assignment_produces_the_expected_dmx_frame() {
    let source = r#"
        scene main {
            Washes.intensity = 50%;
        }
    "#;

    let mut env = washes_two_fixtures_one_universe();

    let module = lux_compiler::compile(source, &env.targets).expect("program should compile");
    lux_bytecode::verify(&module).expect("compiler must generate valid bytecode");

    let clock = VirtualClock::new();
    let mut vm = Vm::new(module).expect("module should be valid");
    vm.start().expect("entry function should exist");
    vm.run_until_blocked(&clock, &mut env.lighting)
        .expect("program has no WAIT, should run to completion");
    assert!(vm.is_finished());

    let mut frames = HashMap::new();
    inception_renderer::render(&env.lighting, &env.rig, &mut frames);

    let mut output = RecordingDmxOutput::new();
    for (universe, frame) in &frames {
        output.send(*universe, frame).unwrap();
    }

    let expected =
        inception_renderer::intensity_to_dmx8(inception_core::Intensity::from_percent(50).unwrap());
    let frame = output
        .last_frame(UniverseId(1))
        .expect("universe 1 should have been rendered");
    assert_eq!(frame[0], expected, "channel 1 (fixture 0)");
    assert_eq!(frame[4], expected, "channel 5 (fixture 1)");

    // Every unrelated channel stays at 0.
    for (index, &slot) in frame.as_slice().iter().enumerate() {
        if index != 0 && index != 4 {
            assert_eq!(slot, 0, "channel {} should be untouched", index + 1);
        }
    }
}

#[test]
fn attribute_type_error_is_caught_by_the_compiler_not_the_runtime() {
    // Item 35 of the task brief: `Washes.intensity = red;` must fail at
    // type checking. This asserts it never even reaches the VM/renderer —
    // there is no bytecode to run at all.
    let source = r#"
        scene main {
            Washes.intensity = red;
        }
    "#;

    let env = washes_two_fixtures_one_universe();
    let diagnostics =
        lux_compiler::compile(source, &env.targets).expect_err("should fail type checking");

    assert!(
        diagnostics
            .iter()
            .all(|d| d.stage == lux_compiler::Stage::Type)
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("expected `Intensity`, found `Color`"))
    );
}

#[test]
fn malformed_hand_built_set_attribute_is_rejected_before_execution() {
    // Item 36 of the task brief: a `Duration` pushed for an `Intensity`
    // attribute must be rejected by the verifier, never reach the VM.
    use lux_bytecode::{
        BytecodeModule, BytecodeVersion, Constant, ConstantId, Function, FunctionId, Instruction,
    };

    let module = BytecodeModule {
        version: BytecodeVersion::CURRENT,
        constants: vec![Constant::Duration(1_000_000_000)],
        functions: vec![Function {
            id: FunctionId(0),
            code: vec![
                Instruction::Const(ConstantId(0)),
                Instruction::SetAttribute {
                    target: lux_bytecode::TargetId(0),
                    attribute: lux_bytecode::Attribute::Intensity,
                },
                Instruction::Return,
            ],
            locals: vec![],
            max_stack: 1,
            debug_name: None,
        }],
        entry: Some(FunctionId(0)),
        target_count: 1,
    };

    assert!(lux_bytecode::verify(&module).is_err());
    assert!(matches!(
        Vm::new(module),
        Err(inception_vm::VmInitError::Verification(_))
    ));
}
