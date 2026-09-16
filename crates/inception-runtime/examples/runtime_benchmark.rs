//! Lightweight, dependency-free runtime baseline. Run with:
//! `cargo run --release -p inception-runtime --example runtime_benchmark`.

use std::time::Instant;

use inception_core::{Timestamp, UniverseId};
use inception_driver_dmx::NullDmxOutput;
use inception_linker::{
    Capability, CapabilitySet, FixtureDefinition, FixtureLibrary, FixtureMappings, Patch,
    RigBinding, RoleBinding, link,
};
use inception_runtime::RuntimeEngine;

const SAMPLES: u64 = 500;

fn main() {
    for (universes, fixtures) in [(1, 100), (4, 500), (10, 1_000)] {
        run_case(universes, fixtures);
    }
}

fn run_case(universe_count: u16, fixture_count: usize) {
    let program = lux_compiler::compile_portable(
        r#"
        rig contract BenchmarkRig {
            role Lights: Group<Intensity>;
        }
        scene main {
            Lights.intensity -> 100% over 3600s;
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
    let mut patch = Patch::new("BenchmarkVenue");
    let mut fixture_names = Vec::with_capacity(fixture_count);
    for index in 0..fixture_count {
        let name = format!("fixture_{index}");
        let universe_index = index % usize::from(universe_count);
        let address = index / usize::from(universe_count) + 1;
        patch
            .add_fixture(
                &name,
                "Dimmer",
                UniverseId(universe_index as u16 + 1),
                address as u16,
            )
            .unwrap();
        fixture_names.push(name);
    }
    let rig = RigBinding {
        name: "BenchmarkVenueRig".into(),
        contract: "BenchmarkRig".into(),
        bindings: vec![RoleBinding {
            role: "Lights".into(),
            fixtures: fixture_names,
        }],
    };
    let image = link(&program, &library, &patch, &rig).unwrap();
    let mut engine = RuntimeEngine::new(image, NullDmxOutput).unwrap();
    engine.start(Timestamp::ZERO).unwrap();

    let started = Instant::now();
    for sample in 1..=SAMPLES {
        engine.tick(Timestamp::from_millis(sample * 25)).unwrap();
    }
    let elapsed = started.elapsed();
    println!(
        "{universe_count} universe(s), {fixture_count} fixtures: {:?}/frame ({:?} total, {SAMPLES} frames)",
        elapsed / SAMPLES as u32,
        elapsed,
    );
}
