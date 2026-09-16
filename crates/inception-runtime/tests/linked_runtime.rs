use std::convert::Infallible;

use inception_core::{Clock, Duration, Timestamp, UniverseId, VirtualClock};
use inception_driver_dmx::{NullDmxOutput, RecordingDmxOutput};
use inception_linker::{
    Capability, CapabilitySet, FixtureDefinition, FixtureLibrary, FixtureMappings, Patch,
    RigBinding, RoleBinding, RuntimeImage, link,
};
use inception_runtime::{RuntimeConfig, RuntimeEngine, RuntimeLoop, Sleeper};

fn linked_image(fixtures: &[(String, UniverseId, u16)]) -> RuntimeImage {
    let program = lux_compiler::compile_portable(
        r#"
        rig contract DemoRig {
            role Washes: Group<Intensity>;
        }
        scene main {
            Washes.intensity -> 100% over 2s;
        }
        "#,
    )
    .unwrap();

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
