use std::convert::Infallible;

use inception_core::{Clock, Duration, Timestamp, UniverseId, VirtualClock};
use inception_driver_dmx::{NullDmxOutput, RecordingDmxOutput};
use inception_linker::{
    Capability, CapabilitySet, FixtureDefinition, FixtureLibrary, FixtureMappings, Patch,
    RigBinding, RoleBinding, RuntimeImage, link,
};
use inception_runtime::{RuntimeConfig, RuntimeEngine, RuntimeLoop, Sleeper};

fn linked_image(fixtures: &[(String, UniverseId, u16)]) -> RuntimeImage {
    linked_image_for(
        r#"
        rig contract DemoRig {
            role Washes: Group<Intensity>;
        }
        scene main {
            Washes.intensity -> 100% over 2s;
        }
        "#,
        fixtures,
    )
}

fn linked_image_for(source: &str, fixtures: &[(String, UniverseId, u16)]) -> RuntimeImage {
    let program = lux_compiler::compile_portable(source).unwrap();

    let definition = FixtureDefinition::new(
        "Dimmer",
        1,
        CapabilitySet::from_capabilities([Capability::Intensity]),
        FixtureMappings {
            intensity: Some(0),
            color: None,
        },
    )
    .unwrap();
    let mut library = FixtureLibrary::new();
    library.insert(definition);
    let mut patch = Patch::new("Venue");
    for (name, universe, address) in fixtures {
        patch
            .add_fixture(name, "Dimmer", *universe, *address)
            .unwrap();
    }
    let rig = RigBinding {
        name: "VenueRig".into(),
        contract: "DemoRig".into(),
        bindings: vec![RoleBinding {
            role: "Washes".into(),
            fixtures: fixtures.iter().map(|(name, _, _)| name.clone()).collect(),
        }],
    };
    link(&program, &library, &patch, &rig).unwrap()
}

fn immediate_source(percent: u8) -> String {
    format!(
        r#"
        rig contract DemoRig {{
            role Washes: Group<Intensity>;
        }}
        scene main {{
            Washes.intensity = {percent}%;
        }}
        "#
    )
}

fn signal_source(percent: u8) -> String {
    format!(
        r#"
        import std.Signal;
        rig contract DemoRig {{
            role Washes: Group<Intensity>;
        }}
        scene main {{
            Washes.intensity <- Signal.constant({percent}%);
        }}
        "#
    )
}

fn linked_two_role_shared_signal_image() -> RuntimeImage {
    let source = r#"
        import std.Signal;
        rig contract DemoRig {
            role Front: Group<Intensity>;
            role Back: Group<Intensity>;
        }
        scene main {
            let level = Signal.constant(25%);
            Front.intensity <- level;
            Back.intensity <- level;
        }
    "#;
    let program = lux_compiler::compile_portable(source).unwrap();
    let definition = FixtureDefinition::new(
        "Dimmer",
        1,
        CapabilitySet::from_capabilities([Capability::Intensity]),
        FixtureMappings {
            intensity: Some(0),
            color: None,
        },
    )
    .unwrap();
    let mut library = FixtureLibrary::new();
    library.insert(definition);
    let mut patch = Patch::new("Venue");
    patch
        .add_fixture("front_wash", "Dimmer", UniverseId(1), 1)
        .unwrap();
    patch
        .add_fixture("back_wash", "Dimmer", UniverseId(1), 2)
        .unwrap();
    let rig = RigBinding {
        name: "VenueRig".into(),
        contract: "DemoRig".into(),
        bindings: vec![
            RoleBinding {
                role: "Front".into(),
                fixtures: vec!["front_wash".into()],
            },
            RoleBinding {
                role: "Back".into(),
                fixtures: vec!["back_wash".into()],
            },
        ],
    };
    link(&program, &library, &patch, &rig).unwrap()
}

/// Item 60/61: `Front.intensity <- Signal.constant(50%);`, driven through
/// the real `RuntimeEngine` (compile -> link -> VM -> signal binding
/// engine -> renderer -> DMX), not just the `SignalBindingStore` in
/// isolation.
#[test]
fn signal_constant_binding_produces_stable_dmx_at_any_timestamp() {
    let image = linked_image_for(&signal_source(50), &[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();

    engine.start(Timestamp::ZERO).unwrap();
    let expected = engine.output().last_frame(UniverseId(1)).unwrap()[0];
    assert!(expected > 0, "50% should not render as blackout");

    // Item 35/64: the same value at every timestamp, including ones far
    // apart — this is what proves the binding samples `now` rather than
    // depending on how many frames were rendered in between.
    for at in [
        Timestamp::from_millis(25),
        Timestamp::from_secs(1),
        Timestamp::from_secs(10),
        Timestamp::from_secs(3600),
    ] {
        engine.tick(at).unwrap();
        assert_eq!(
            engine.output().last_frame(UniverseId(1)).unwrap()[0],
            expected,
            "mismatch at {at:?}"
        );
    }
}

/// Item 62: a target with several fixtures gets the same signal-driven
/// value on all of them.
#[test]
fn signal_binding_drives_every_fixture_of_a_multi_fixture_target() {
    let image = linked_image_for(
        &signal_source(25),
        &[
            ("a".into(), UniverseId(1), 1),
            ("b".into(), UniverseId(1), 2),
            ("c".into(), UniverseId(1), 3),
        ],
    );
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    assert!(frame[0] > 0);
    assert_eq!(frame[0], frame[1]);
    assert_eq!(frame[1], frame[2]);
}

/// Item 63: two different targets bound to the same `let`-bound signal
/// both render its value, end to end.
#[test]
fn a_shared_signal_drives_two_independent_targets_consistently() {
    let image = linked_two_role_shared_signal_image();
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    assert!(frame[0] > 0);
    assert_eq!(frame[0], frame[1]);
}

/// Item 36: a scene consisting only of `<-` (no `wait`) already finishes
/// the VM inside `start()` — the binding must keep driving DMX long
/// after that, since it isn't tied to any VM instruction still "in
/// flight".
#[test]
fn signal_binding_keeps_driving_dmx_long_after_the_scene_finishes() {
    let image = linked_image_for(&signal_source(50), &[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    let expected = engine.output().last_frame(UniverseId(1)).unwrap()[0];

    engine.tick(Timestamp::from_secs(3600)).unwrap();

    assert_eq!(
        engine.output().last_frame(UniverseId(1)).unwrap()[0],
        expected
    );
}

/// Item 33/57: a direct `=` after a signal binding detaches it — the
/// assigned value must stay stable afterward, not get overwritten by the
/// old signal on a later frame.
#[test]
fn direct_assignment_after_a_signal_binding_stays_stable() {
    let source = r#"
        import std.Signal;
        rig contract DemoRig {
            role Washes: Group<Intensity>;
        }
        scene main {
            let level = Signal.constant(50%);
            Washes.intensity <- level;
            wait 2s;
            Washes.intensity = 25%;
        }
    "#;
    let image = linked_image_for(source, &[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    engine.tick(Timestamp::from_secs(2)).unwrap();
    let after_assign = engine.output().last_frame(UniverseId(1)).unwrap()[0];
    assert_eq!(after_assign, 64); // 25% -> 16384 -> DMX 64

    engine.tick(Timestamp::from_secs(10)).unwrap();
    assert_eq!(
        engine.output().last_frame(UniverseId(1)).unwrap()[0],
        after_assign
    );
}

fn passive_source() -> &'static str {
    r#"
    rig contract DemoRig {
        role Washes: Group<Intensity>;
    }
    scene main {
    }
    "#
}

#[test]
fn runtime_engine_drives_linked_transition_to_recording_output() {
    let image = linked_image(&[
        ("wash_left".into(), UniverseId(1), 1),
        ("wash_right".into(), UniverseId(1), 5),
    ]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();

    engine.start(Timestamp::ZERO).unwrap();
    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    assert_eq!((frame[0], frame[4]), (0, 0));

    engine.tick(Timestamp::from_secs(1)).unwrap();
    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    assert_eq!((frame[0], frame[4]), (128, 128));

    engine.tick(Timestamp::from_secs(2)).unwrap();
    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    assert_eq!((frame[0], frame[4]), (255, 255));
    assert_eq!(engine.active_transition_count(), 0);
}

#[test]
fn missed_frames_sample_transition_directly_at_actual_time() {
    let image = linked_image(&[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    engine.tick(Timestamp::from_millis(25)).unwrap();
    engine.tick(Timestamp::from_millis(50)).unwrap();
    engine.tick(Timestamp::from_millis(1_100)).unwrap();

    assert_eq!(engine.output().last_frame(UniverseId(1)).unwrap()[0], 141);
    assert_eq!(engine.output().history().len(), 4);
}

#[test]
fn sends_multiple_universes_in_ascending_order_and_blackouts_on_stop() {
    let image = linked_image(&[
        ("second".into(), UniverseId(2), 1),
        ("first".into(), UniverseId(1), 1),
    ]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    engine.tick(Timestamp::from_secs(2)).unwrap();
    engine.stop().unwrap();

    let history = engine.output().history();
    let order: Vec<_> = history.iter().map(|(universe, _)| *universe).collect();
    assert_eq!(
        order,
        vec![
            UniverseId(1),
            UniverseId(2),
            UniverseId(1),
            UniverseId(2),
            UniverseId(1),
            UniverseId(2),
        ]
    );
    assert_eq!(history[2].1[0], 255);
    assert_eq!(history[3].1[0], 255);
    assert_eq!(history[4].1[0], 0);
    assert_eq!(history[5].1[0], 0);
}

#[derive(Debug, Default)]
struct VirtualSleeper;

impl Sleeper<VirtualClock> for VirtualSleeper {
    type Error = Infallible;

    fn sleep_until(
        &mut self,
        clock: &VirtualClock,
        deadline: Timestamp,
    ) -> Result<(), Self::Error> {
        let now = clock.now();
        if deadline > now {
            clock.advance(Duration::from_nanos(deadline.0 - now.0));
        }
        Ok(())
    }
}

#[test]
fn runtime_loop_uses_virtual_absolute_deadlines() {
    let image = linked_image(&[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    let mut runtime_loop = RuntimeLoop::new(
        RuntimeConfig::default(),
        VirtualClock::new(),
        VirtualSleeper,
    );
    runtime_loop.start(&mut engine).unwrap();
    for expected in [25, 50, 75, 100] {
        runtime_loop.run_next_frame(&mut engine).unwrap();
        assert_eq!(runtime_loop.clock().now(), Timestamp::from_millis(expected));
    }
    let stats = runtime_loop.timing_stats().unwrap();
    assert_eq!(stats.frames, 5);
    assert_eq!(stats.late_frames, 0);
}

#[test]
fn late_runtime_loop_emits_only_one_frame_and_resumes_at_future_deadline() {
    let image = linked_image(&[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    let mut runtime_loop = RuntimeLoop::new(
        RuntimeConfig::default(),
        VirtualClock::new(),
        VirtualSleeper,
    );
    runtime_loop.start(&mut engine).unwrap();
    runtime_loop.run_next_frame(&mut engine).unwrap();
    runtime_loop.clock().advance(Duration::from_millis(62));
    runtime_loop.run_next_frame(&mut engine).unwrap();

    assert_eq!(runtime_loop.clock().now(), Timestamp::from_millis(87));
    assert_eq!(
        runtime_loop.next_deadline(),
        Some(Timestamp::from_millis(100))
    );
    assert_eq!(runtime_loop.timing_stats().unwrap().late_frames, 1);
    assert_eq!(engine.output().history().len(), 3);
}

#[test]
fn one_virtual_hour_finishes_transitions_without_recording_growth() {
    let image = linked_image(&[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, NullDmxOutput).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    for frame in 1..=144_000_u64 {
        engine.tick(Timestamp::from_millis(frame * 25)).unwrap();
    }

    assert_eq!(engine.frames_sent(), 144_001);
    assert_eq!(engine.active_transition_count(), 0);
}

#[test]
fn successful_reload_reuses_output_and_replaces_program() {
    let fixtures = [("wash".into(), UniverseId(1), 1)];
    let image_a = linked_image_for(&immediate_source(20), &fixtures);
    let image_b = linked_image_for(&immediate_source(80), &fixtures);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();
    let sends_before = host.output().history().len();

    host.reload(
        inception_runtime::LoadedProgram::new(image_b).unwrap(),
        Timestamp::from_millis(25),
    )
    .unwrap();
    host.tick(Timestamp::from_millis(25)).unwrap();

    assert_eq!(sends_before, 1);
    assert_eq!(host.output().history().len(), 2);
    assert_eq!(host.output().last_frame(UniverseId(1)).unwrap()[0], 205);
}

#[test]
fn failed_compile_never_replaces_the_running_program() {
    let fixtures = [("wash".into(), UniverseId(1), 1)];
    let image_a = linked_image_for(&immediate_source(20), &fixtures);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();

    let invalid = r#"
        rig contract DemoRig { role Washes: Group<Intensity>; }
        scene main { Washes.intensity = red; }
    "#;
    assert!(lux_compiler::compile_portable(invalid).is_err());
    host.tick(Timestamp::from_millis(25)).unwrap();

    assert_eq!(host.output().last_frame(UniverseId(1)).unwrap()[0], 51);
    assert_eq!(host.output().history().len(), 2);
}

#[test]
fn failed_link_never_replaces_the_running_program() {
    let fixtures = [("wash".into(), UniverseId(1), 1)];
    let image_a = linked_image_for(&immediate_source(20), &fixtures);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();

    let program = lux_compiler::compile_portable(&immediate_source(80)).unwrap();
    let definition = FixtureDefinition::new(
        "Dimmer",
        1,
        CapabilitySet::from_capabilities([Capability::Intensity]),
        FixtureMappings {
            intensity: Some(0),
            color: None,
        },
    )
    .unwrap();
    let mut library = FixtureLibrary::new();
    library.insert(definition);
    let mut patch = Patch::new("Venue");
    patch
        .add_fixture("wash", "Dimmer", UniverseId(1), 1)
        .unwrap();
    let invalid_rig = RigBinding {
        name: "VenueRig".into(),
        contract: "DemoRig".into(),
        bindings: Vec::new(),
    };
    assert!(link(&program, &library, &patch, &invalid_rig).is_err());
    host.tick(Timestamp::from_millis(25)).unwrap();

    assert_eq!(host.output().last_frame(UniverseId(1)).unwrap()[0], 51);
    assert_eq!(host.output().history().len(), 2);
}

#[test]
fn reload_samples_transition_and_clears_it_while_preserving_effective_value() {
    let fixtures = [("wash".into(), UniverseId(1), 1)];
    let transitioning = r#"
        rig contract DemoRig { role Washes: Group<Intensity>; }
        scene main { Washes.intensity -> 100% over 10s; }
    "#;
    let image_a = linked_image_for(transitioning, &fixtures);
    let image_b = linked_image_for(passive_source(), &fixtures);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();
    host.tick(Timestamp::from_secs(4)).unwrap();

    let report = host
        .reload(
            inception_runtime::LoadedProgram::new(image_b).unwrap(),
            Timestamp::from_secs(4),
        )
        .unwrap();
    host.tick(Timestamp::from_secs(4)).unwrap();

    assert_eq!(report.preserved_fixtures, 1);
    assert_eq!(host.active_transition_count(), 0);
    assert_eq!(host.output().last_frame(UniverseId(1)).unwrap()[0], 102);
}

/// Item 38: hot reload samples the old program's active signal bindings
/// at `now` (like it already does for transitions) before discarding
/// them, so a compatible fixture in the new program starts from the
/// signal's last effective value rather than losing it.
#[test]
fn reload_samples_signal_binding_and_preserves_effective_value() {
    let fixtures = [("wash".into(), UniverseId(1), 1)];
    let image_a = linked_image_for(&signal_source(40), &fixtures);
    let image_b = linked_image_for(passive_source(), &fixtures);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();
    host.tick(Timestamp::from_secs(4)).unwrap();
    let before_reload = host.output().last_frame(UniverseId(1)).unwrap()[0];

    let report = host
        .reload(
            inception_runtime::LoadedProgram::new(image_b).unwrap(),
            Timestamp::from_secs(4),
        )
        .unwrap();
    host.tick(Timestamp::from_secs(4)).unwrap();

    assert_eq!(report.preserved_fixtures, 1);
    assert_eq!(host.active_binding_count(), 0);
    assert_eq!(
        host.output().last_frame(UniverseId(1)).unwrap()[0],
        before_reload
    );
}

#[test]
fn state_preservation_uses_stable_names_not_reassigned_fixture_ids() {
    let fixtures_a = [
        ("stable".into(), UniverseId(1), 1),
        ("removed".into(), UniverseId(1), 2),
    ];
    let fixtures_b = [
        ("new".into(), UniverseId(1), 3),
        ("stable".into(), UniverseId(1), 1),
    ];
    let image_a = linked_image_for(&immediate_source(73), &fixtures_a);
    let image_b = linked_image_for(passive_source(), &fixtures_b);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();
    host.reload(
        inception_runtime::LoadedProgram::new(image_b).unwrap(),
        Timestamp::from_millis(25),
    )
    .unwrap();
    host.tick(Timestamp::from_millis(25)).unwrap();

    let frame = host.output().last_frame(UniverseId(1)).unwrap();
    assert_eq!(frame[0], 187); // stable fixture retained 73%
    assert_eq!(frame[1], 0); // removed fixture was blacked
    assert_eq!(frame[2], 0); // new fixture starts at its initial state
}
