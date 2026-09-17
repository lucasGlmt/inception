use std::collections::HashMap;

use inception_core::{Clock, Duration, TransitionEngine, UniverseId, VirtualClock};
use inception_driver_dmx::{DmxOutput, RecordingDmxOutput};
use inception_linker::*;
use inception_vm::Vm;

const SHOW: &str = r#"
rig contract DemoRig {
    role Washes: Group<Color + Intensity>;
}

scene main {
    Washes.intensity -> 100% over 2s;
}
"#;

fn rgb_library() -> FixtureLibrary {
    let mut capabilities = CapabilitySet::empty();
    capabilities.insert(Capability::Intensity);
    capabilities.insert(Capability::Color);
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
            strobe: None,
        },
    )
    .unwrap();
    let mut library = FixtureLibrary::new();
    library.insert(definition);
    library
}

fn configuration(universe: u16, addresses: &[u16]) -> (Patch, RigBinding) {
    let mut patch = Patch::new("venue");
    let mut names = Vec::new();
    for (index, &address) in addresses.iter().enumerate() {
        let name = format!("wash_{index}");
        patch
            .add_fixture(&name, "GenericRgbPar", UniverseId(universe), address)
            .unwrap();
        names.push(name);
    }
    let rig = RigBinding {
        name: "VenueRig".into(),
        contract: "DemoRig".into(),
        bindings: vec![RoleBinding {
            role: "Washes".into(),
            fixtures: names,
        }],
    };
    (patch, rig)
}

fn run_at_half_time(image: RuntimeImage) -> RecordingDmxOutput {
    let mut lighting = image.lighting_state();
    let mut vm = Vm::new(image.bytecode).unwrap();
    let clock = VirtualClock::new();
    let mut transitions = TransitionEngine::new();
    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();
    clock.advance(Duration::from_secs(1));
    transitions.sample(clock.now(), &mut lighting).unwrap();
    let mut frames = HashMap::new();
    inception_renderer::render(&lighting, &image.rig, &mut frames);
    let mut output = RecordingDmxOutput::new();
    for (universe, frame) in frames {
        output.send(universe, &frame).unwrap();
    }
    output
}

#[test]
fn full_lux_linked_runtime_reaches_virtual_dmx() {
    let program = lux_compiler::compile_portable(SHOW).unwrap();
    let (patch, rig) = configuration(1, &[1, 5]);
    let image = link(&program, &rgb_library(), &patch, &rig).unwrap();

    let mut lighting = image.lighting_state();
    let mut vm = Vm::new(image.bytecode.clone()).unwrap();
    let clock = VirtualClock::new();
    let mut transitions = TransitionEngine::new();
    vm.start().unwrap();
    vm.run_until_blocked(&clock, &mut lighting, &mut transitions)
        .unwrap();

    for (advance, expected) in [(0, 0), (1000, 128), (1000, 255)] {
        clock.advance(Duration::from_millis(advance));
        transitions.sample(clock.now(), &mut lighting).unwrap();
        let mut frames = HashMap::new();
        inception_renderer::render(&lighting, &image.rig, &mut frames);
        let frame = &frames[&UniverseId(1)];
        assert_eq!(frame[0], expected);
        assert_eq!(frame[4], expected);
    }
}

#[test]
fn same_compiled_show_links_to_two_physical_rigs() {
    let program = lux_compiler::compile_portable(SHOW).unwrap();
    let (patch_a, rig_a) = configuration(1, &[1, 5]);
    let (patch_b, rig_b) = configuration(2, &[20, 24, 28]);
    let image_a = link(&program, &rgb_library(), &patch_a, &rig_a).unwrap();
    let image_b = link(&program, &rgb_library(), &patch_b, &rig_b).unwrap();
    assert_eq!(image_a.bytecode, image_b.bytecode);
    assert_ne!(image_a.rig, image_b.rig);

    let output_a = run_at_half_time(image_a);
    let output_b = run_at_half_time(image_b);
    let frame_a = output_a.last_frame(UniverseId(1)).unwrap();
    assert_eq!(frame_a[0], 128);
    assert_eq!(frame_a[4], 128);
    let frame_b = output_b.last_frame(UniverseId(2)).unwrap();
    assert_eq!(frame_b[19], 128);
    assert_eq!(frame_b[23], 128);
    assert_eq!(frame_b[27], 128);
}
