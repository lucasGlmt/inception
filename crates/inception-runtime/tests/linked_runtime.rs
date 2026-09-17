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

// --- Signal composition: .range()/.phase() ------------------------------

/// Mirrors `inception_renderer::convert::u16_to_dmx8` exactly (`(raw +
/// 128) >> 8`, clamped to 255) — used to derive expected DMX values from
/// hand-computed raw `Intensity` values in these tests, instead of
/// guessing at a rounding rule.
fn u16_to_dmx8(raw: f64) -> u8 {
    (((raw.round() as u32) + 128) >> 8).min(255) as u8
}

fn breathing_source() -> &'static str {
    r#"
    import std.Effects;
    rig contract DemoRig {
        role Washes: Group<Intensity>;
    }
    scene main {
        Washes.intensity <-
            Effects.sine(2s).range(20%, 100%);
    }
    "#
}

/// Items 63/64/78: `Front.intensity <- Effects.sine(2s).range(20%, 100%);`
/// driven through the real `RuntimeEngine` (compile -> link -> VM ->
/// signal graph -> binding engine -> renderer -> `RecordingDmxOutput`),
/// matching the task brief's documented DMX table exactly.
#[test]
fn composed_signal_binding_produces_the_documented_dmx_table() {
    let image = linked_image_for(breathing_source(), &[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    for (millis, expected) in [(0, 154), (500, 255), (1000, 154), (1500, 51), (2000, 154)] {
        engine.tick(Timestamp::from_millis(millis)).unwrap();
        assert_eq!(
            engine.output().last_frame(UniverseId(1)).unwrap()[0],
            expected,
            "mismatch at {millis}ms"
        );
    }
}

/// Item 65: rendering only at `0s, 730ms, 1810ms, 4440ms` (never the
/// frames in between) must still produce values matching the graph
/// sampled directly at those exact timestamps — no dependency on how
/// many frames were rendered before.
#[test]
fn composed_signal_binding_survives_missed_frames() {
    let image = linked_image_for(breathing_source(), &[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    for millis in [730, 1810, 4440] {
        engine.tick(Timestamp::from_millis(millis)).unwrap();
        let phase = (millis % 2000) as f64 / 2000.0;
        let source = 0.5 + 0.5 * (std::f64::consts::TAU * phase).sin();
        let expected_intensity = 13107.0 + source * (65535.0 - 13107.0);
        let expected_dmx = u16_to_dmx8(expected_intensity);
        let actual = engine.output().last_frame(UniverseId(1)).unwrap()[0];
        assert!(
            actual.abs_diff(expected_dmx) <= 1,
            "at {millis}ms: expected ~{expected_dmx}, got {actual}"
        );
    }
}

/// Item 79: `.phase()` composed before `.range()` also reaches DMX.
#[test]
fn phase_then_range_binding_reaches_dmx() {
    let source = r#"
        import std.Effects;
        rig contract DemoRig {
            role Washes: Group<Intensity>;
        }
        scene main {
            Washes.intensity <-
                Effects.sine(2s)
                    .phase(90deg)
                    .range(10%, 80%);
        }
    "#;
    let image = linked_image_for(source, &[("wash".into(), UniverseId(1), 1)]);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    // phase(90deg) at t=0 reads what the unshifted sine reads at t=500ms
    // (1.0) -> top of the 10%..80% range.
    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    let min = 6553.0; // 10%
    let max = 52428.0; // 80%
    let expected_dmx = u16_to_dmx8(min + 1.0 * (max - min));
    assert_eq!(frame[0], expected_dmx);
}

/// Item 66/67: modifying `.range(20%, 100%)` to `.range(5%, 40%)` and hot
/// reloading rebuilds the signal graph normally, with no special-cased
/// graph migration.
#[test]
fn hot_reload_rebuilds_the_signal_graph() {
    let fixtures = [("wash".into(), UniverseId(1), 1)];
    let image_a = linked_image_for(breathing_source(), &fixtures);
    let new_source = r#"
        import std.Effects;
        rig contract DemoRig {
            role Washes: Group<Intensity>;
        }
        scene main {
            Washes.intensity <-
                Effects.sine(2s).range(5%, 40%);
        }
    "#;
    let image_b = linked_image_for(new_source, &fixtures);
    let mut host = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    host.start(Timestamp::ZERO).unwrap();
    host.tick(Timestamp::from_millis(500)).unwrap(); // source=1.0 -> 100%
    assert_eq!(host.output().last_frame(UniverseId(1)).unwrap()[0], 255);

    host.reload(
        inception_runtime::LoadedProgram::new(image_b).unwrap(),
        Timestamp::from_millis(500),
    )
    .unwrap();
    host.tick(Timestamp::from_millis(500)).unwrap();

    // The new program's own oscillator restarts at t=500ms (its own
    // construction time), so its phase is 0 there -> source=0.5 ->
    // midpoint of 5%..40%.
    let min = 3276.0; // 5%
    let max = 26214.0; // 40%
    let expected_dmx = u16_to_dmx8(min + 0.5 * (max - min));
    let actual = host.output().last_frame(UniverseId(1)).unwrap()[0];
    assert!(
        actual.abs_diff(expected_dmx) <= 1,
        "expected ~{expected_dmx}, got {actual}"
    );
}

// --- Signal composition: `.spread()` -------------------------------------

/// Like `linked_image_for`, but binds a role named "Front" (`linked_image_for`
/// always binds "Washes") to however many fixtures `fixtures` lists — used
/// by every `.spread()` test below, most of them with 4 fixtures, each on
/// its own DMX channel/address (in written order — see `bind_role` in
/// `inception-linker`, which resolves a role's fixtures in exactly the
/// order `RoleBinding::fixtures` lists them, never a `HashMap`'s), so
/// `RecordingDmxOutput`'s frame lets each fixture's own spread offset be
/// checked independently.
fn linked_front_image(source: &str, fixtures: &[(String, UniverseId, u16)]) -> RuntimeImage {
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
            role: "Front".into(),
            fixtures: fixtures.iter().map(|(name, _, _)| name.clone()).collect(),
        }],
    };
    link(&program, &library, &patch, &rig).unwrap()
}

fn four_fixture_front_image(source: &str) -> RuntimeImage {
    linked_front_image(
        source,
        &[
            ("front_0".into(), UniverseId(1), 1),
            ("front_1".into(), UniverseId(1), 2),
            ("front_2".into(), UniverseId(1), 3),
            ("front_3".into(), UniverseId(1), 4),
        ],
    )
}

fn spread_source(spread_deg: u32, min_percent: u8, max_percent: u8) -> String {
    format!(
        r#"
        import std.Effects;
        rig contract DemoRig {{
            role Front: Group<Intensity>;
        }}
        scene main {{
            Front.intensity <-
                Effects.sine(2s)
                    .spread({spread_deg}deg)
                    .range({min_percent}%, {max_percent}%);
        }}
        "#
    )
}

/// Item 11: `Front.intensity <- Effects.sine(2s).spread(360deg).range(0%, 100%);`
/// over 4 fixtures, driven through the real `RuntimeEngine`. At `t = 0`
/// fixture `i` reads the unshifted sine at phase `i * 90deg` — `0°` and
/// `180°` both land on sine's `0.5` midpoint (`~50%`), `90°` on its peak
/// (`100%`), `270°` on its trough (`0%`) — producing a genuine spatial
/// wave across the group, not one shared value.
#[test]
fn spread_360_over_four_fixtures_produces_the_documented_dmx_table() {
    let image = four_fixture_front_image(&spread_source(360, 0, 100));
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    engine.tick(Timestamp::ZERO).unwrap();

    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    let expected_dmx = |offset_deg: f64| {
        let phase = offset_deg / 360.0;
        let source = 0.5 + 0.5 * (std::f64::consts::TAU * phase).sin();
        u16_to_dmx8(source * 65535.0)
    };
    for (index, offset_deg) in [(0, 0.0), (1, 90.0), (2, 180.0), (3, 270.0)] {
        let expected = expected_dmx(offset_deg);
        let actual = frame[index];
        assert!(
            actual.abs_diff(expected) <= 1,
            "fixture {index} (offset {offset_deg}deg): expected ~{expected}, got {actual}"
        );
    }
    // A genuine wave means the four fixtures are not all equal.
    assert_ne!(frame[0], frame[1]);
    assert_ne!(frame[1], frame[3]);
}

/// Item 12: `.spread(180deg)` over 4 fixtures distributes phase as `0°,
/// 45°, 90°, 135°` — `i * amount / n`, never `i * amount / (n - 1)`.
#[test]
fn spread_180_over_four_fixtures_produces_the_documented_offsets() {
    let image = four_fixture_front_image(&spread_source(180, 0, 100));
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    engine.tick(Timestamp::ZERO).unwrap();

    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    let expected_dmx = |offset_deg: f64| {
        let phase = offset_deg / 360.0;
        let source = 0.5 + 0.5 * (std::f64::consts::TAU * phase).sin();
        u16_to_dmx8(source * 65535.0)
    };
    for (index, offset_deg) in [(0, 0.0), (1, 45.0), (2, 90.0), (3, 135.0)] {
        let expected = expected_dmx(offset_deg);
        let actual = frame[index];
        assert!(
            actual.abs_diff(expected) <= 1,
            "fixture {index} (offset {offset_deg}deg): expected ~{expected}, got {actual}"
        );
    }
}

/// Item 13: linking the exact same rig source twice (two independent
/// `RuntimeImage`s) produces byte-for-byte identical DMX output — the
/// fixture order (and so every fixture's spread offset) never depends on
/// anything nondeterministic like a `HashMap`'s iteration order.
#[test]
fn spread_fixture_order_is_deterministic_across_separate_links() {
    let source = spread_source(360, 0, 100);
    let image_a = four_fixture_front_image(&source);
    let image_b = four_fixture_front_image(&source);

    let mut engine_a = RuntimeEngine::new(image_a, RecordingDmxOutput::new()).unwrap();
    let mut engine_b = RuntimeEngine::new(image_b, RecordingDmxOutput::new()).unwrap();
    engine_a.start(Timestamp::ZERO).unwrap();
    engine_b.start(Timestamp::ZERO).unwrap();

    for millis in [0, 500, 1234, 1999] {
        engine_a.tick(Timestamp::from_millis(millis)).unwrap();
        engine_b.tick(Timestamp::from_millis(millis)).unwrap();
        assert_eq!(
            engine_a.output().last_frame(UniverseId(1)).unwrap(),
            engine_b.output().last_frame(UniverseId(1)).unwrap(),
            "mismatch at {millis}ms"
        );
    }
}

/// Item 14: rendering only at irregular timestamps (never the frames in
/// between) must still produce, for every fixture, exactly the value a
/// direct `(now, fixture_index, fixture_count)` computation gives — no
/// dependency on how many previous frames were rendered, for a spread
/// binding same as for a plain one.
#[test]
fn spread_survives_missed_frames() {
    let image = four_fixture_front_image(&spread_source(360, 0, 100));
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    for millis in [730, 1810, 4440] {
        engine.tick(Timestamp::from_millis(millis)).unwrap();
        let frame = engine.output().last_frame(UniverseId(1)).unwrap();
        for (index, offset_deg) in [(0, 0.0), (1, 90.0), (2, 180.0), (3, 270.0)] {
            let phase = ((millis % 2000) as f64 / 2000.0 + offset_deg / 360.0).rem_euclid(1.0);
            let source = 0.5 + 0.5 * (std::f64::consts::TAU * phase).sin();
            let expected = u16_to_dmx8(source * 65535.0);
            let actual = frame[index];
            assert!(
                actual.abs_diff(expected) <= 1,
                "at {millis}ms, fixture {index}: expected ~{expected}, got {actual}"
            );
        }
    }
}

/// Item 9: with a single-fixture target, `.spread()` is a no-op — the
/// bound fixture reads exactly what the unshifted (`.range()`-wrapped)
/// source would.
#[test]
fn spread_with_a_single_fixture_target_is_a_no_op() {
    let spread_image = linked_front_image(
        &spread_source(360, 0, 100),
        &[("only".into(), UniverseId(1), 1)],
    );
    let plain_source = r#"
        import std.Effects;
        rig contract DemoRig {
            role Front: Group<Intensity>;
        }
        scene main {
            Front.intensity <-
                Effects.sine(2s).range(0%, 100%);
        }
    "#;
    let plain_image = linked_front_image(plain_source, &[("only".into(), UniverseId(1), 1)]);

    let mut spread_engine = RuntimeEngine::new(spread_image, RecordingDmxOutput::new()).unwrap();
    let mut plain_engine = RuntimeEngine::new(plain_image, RecordingDmxOutput::new()).unwrap();
    spread_engine.start(Timestamp::ZERO).unwrap();
    plain_engine.start(Timestamp::ZERO).unwrap();

    for millis in [0, 500, 1000, 1500] {
        spread_engine.tick(Timestamp::from_millis(millis)).unwrap();
        plain_engine.tick(Timestamp::from_millis(millis)).unwrap();
        assert_eq!(
            spread_engine.output().last_frame(UniverseId(1)).unwrap()[0],
            plain_engine.output().last_frame(UniverseId(1)).unwrap()[0],
            "mismatch at {millis}ms"
        );
    }
}

/// Item 6: `.phase(45deg).spread(360deg)` composes — the static phase
/// applies to every fixture, on top of each fixture's own spread offset.
#[test]
fn spread_composes_with_a_preceding_phase() {
    let source = r#"
        import std.Effects;
        rig contract DemoRig {
            role Front: Group<Intensity>;
        }
        scene main {
            Front.intensity <-
                Effects.sine(2s)
                    .phase(45deg)
                    .spread(360deg)
                    .range(0%, 100%);
        }
    "#;
    let image = four_fixture_front_image(source);
    let mut engine = RuntimeEngine::new(image, RecordingDmxOutput::new()).unwrap();
    engine.start(Timestamp::ZERO).unwrap();
    engine.tick(Timestamp::ZERO).unwrap();

    let frame = engine.output().last_frame(UniverseId(1)).unwrap();
    let offsets: [(usize, f64); 4] = [(0, 0.0), (1, 90.0), (2, 180.0), (3, 270.0)];
    for (index, spread_offset_deg) in offsets {
        let phase = ((45.0 + spread_offset_deg) / 360.0).rem_euclid(1.0);
        let source = 0.5 + 0.5 * (std::f64::consts::TAU * phase).sin();
        let expected = u16_to_dmx8(source * 65535.0);
        let actual = frame[index];
        assert!(
            actual.abs_diff(expected) <= 1,
            "fixture {index}: expected ~{expected}, got {actual}"
        );
    }
}
