use std::collections::HashMap;

use inception_core::{Clock, Duration, UniverseId, VirtualClock};
use inception_driver_dmx::{DmxOutput, RecordingDmxOutput};
use inception_linker::{
    Capability, CapabilitySet, FixtureDefinition, FixtureLibrary, FixtureMappings, Patch,
    RgbOffsets, RigBinding, RoleBinding, link,
};
use inception_runtime::Runtime;

#[test]
fn runtime_consumes_linked_image_and_renders_virtual_dmx() {
    let program = lux_compiler::compile_portable(
        r#"
        rig contract DemoRig {
            role Washes: Group<Color + Intensity>;
        }
        scene main {
            Washes.intensity -> 100% over 2s;
        }
        "#,
    )
    .unwrap();

    let capabilities = CapabilitySet::from_capabilities([Capability::Intensity, Capability::Color]);
    let definition = FixtureDefinition::new(
        "GenericRgbPar",
        4,
        capabilities,
        FixtureMappings {
            intensity: Some(0),
            color: Some(RgbOffsets {
                red: 1,
                green: 2,
                blue: 3,
            }),
        },
    )
    .unwrap();
    let mut library = FixtureLibrary::new();
    library.insert(definition);
    let mut patch = Patch::new("DemoVenue");
    patch
        .add_fixture("wash_left", "GenericRgbPar", UniverseId(1), 1)
        .unwrap();
    patch
        .add_fixture("wash_right", "GenericRgbPar", UniverseId(1), 5)
        .unwrap();
    let rig = RigBinding {
        name: "DemoVenueRig".into(),
        contract: "DemoRig".into(),
        bindings: vec![RoleBinding {
            role: "Washes".into(),
            fixtures: vec!["wash_left".into(), "wash_right".into()],
        }],
    };

    let image = link(&program, &library, &patch, &rig).unwrap();
    let mut runtime = Runtime::new(image).unwrap();
    let clock = VirtualClock::new();
    runtime.start().unwrap();
    runtime.run_until_blocked(&clock).unwrap();

    let mut output = RecordingDmxOutput::new();
    for (advance_ms, expected) in [(0, 0), (1000, 128), (1000, 255)] {
        clock.advance(Duration::from_millis(advance_ms));
        let mut frames = HashMap::new();
        runtime.render_at(clock.now(), &mut frames).unwrap();
        for (universe, frame) in frames {
            output.send(universe, &frame).unwrap();
        }
        let frame = output.last_frame(UniverseId(1)).unwrap();
        assert_eq!(frame[0], expected);
        assert_eq!(frame[4], expected);
    }
    assert_eq!(runtime.active_transition_count(), 0);
}
