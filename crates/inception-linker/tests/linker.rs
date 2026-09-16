use inception_core::{FixtureId, UniverseId};
use inception_linker::*;

fn capabilities(values: &[Capability]) -> CapabilitySet {
    CapabilitySet::from_capabilities(values.iter().copied())
}

fn rgb_par() -> FixtureDefinition {
    FixtureDefinition::new(
        "GenericRgbPar",
        4,
        capabilities(&[Capability::Intensity, Capability::Color]),
        FixtureMappings {
            intensity: Some(0),
            color: Some(RgbOffsets {
                red: 1,
                green: 2,
                blue: 3,
            }),
        },
    )
    .unwrap()
}

fn dimmer() -> FixtureDefinition {
    FixtureDefinition::new(
        "SimpleDimmer",
        1,
        capabilities(&[Capability::Intensity]),
        FixtureMappings {
            intensity: Some(0),
            color: None,
        },
    )
    .unwrap()
}

fn library() -> FixtureLibrary {
    let mut library = FixtureLibrary::new();
    library.insert(rgb_par());
    library.insert(dimmer());
    library
}

fn patch_at(second_address: u16) -> Patch {
    let mut patch = Patch::new("DemoVenue");
    patch
        .add_fixture("wash_left", "GenericRgbPar", UniverseId(1), 1)
        .unwrap();
    patch
        .add_fixture("wash_right", "GenericRgbPar", UniverseId(1), second_address)
        .unwrap();
    patch
}

fn source(roles: &str) -> String {
    format!("rig contract DemoRig {{ {roles} }} scene main {{ Washes.intensity -> 100% over 2s; }}")
}

fn washes_rig(fixtures: &[&str]) -> RigBinding {
    RigBinding {
        name: "DemoVenueRig".into(),
        contract: "DemoRig".into(),
        bindings: vec![RoleBinding {
            role: "Washes".into(),
            fixtures: fixtures.iter().map(|name| (*name).to_string()).collect(),
        }],
    }
}

#[test]
fn fixture_definition_validates_capabilities_offsets_and_footprint() {
    assert_eq!(rgb_par().footprint(), 4);
    let errors = FixtureDefinition::new(
        "Broken",
        2,
        capabilities(&[Capability::Color]),
        FixtureMappings {
            intensity: None,
            color: Some(RgbOffsets {
                red: 0,
                green: 1,
                blue: 2,
            }),
        },
    )
    .unwrap_err();
    assert!(
        errors.contains(&FixtureDefinitionError::OffsetOutsideFootprint {
            offset: 2,
            footprint: 2,
        })
    );
}

#[test]
fn phase_a_resolves_exact_rgb_par_channels() {
    let resolved = resolve_patch(&library(), &patch_at(5)).unwrap();
    assert_eq!(resolved.fixtures.len(), 2);
    for (index, expected_base) in [(0, 1), (1, 5)] {
        let fixture = &resolved.fixtures[index];
        assert_eq!(fixture.id, FixtureId(index as u32));
        let intensity = fixture.mapping.intensity.unwrap();
        let color = fixture.mapping.color.unwrap();
        assert_eq!(intensity.universe, UniverseId(1));
        assert_eq!(intensity.channel.get(), expected_base);
        assert_eq!(color.red.get(), expected_base + 1);
        assert_eq!(color.green.get(), expected_base + 2);
        assert_eq!(color.blue.get(), expected_base + 3);
    }
}

#[test]
fn invalid_addresses_are_rejected_at_patch_construction() {
    let mut patch = Patch::new("bad");
    for address in [0, 513] {
        assert!(matches!(
            patch.add_fixture("bad", "GenericRgbPar", UniverseId(1), address),
            Err(LinkError::InvalidDmxAddress { address: found, .. }) if found == address
        ));
    }
}

#[test]
fn patch_rejects_collision_with_actionable_ranges() {
    let errors = resolve_patch(&library(), &patch_at(4)).unwrap_err();
    assert!(errors.iter().any(|error| matches!(
        error,
        LinkError::DmxCollision {
            universe: UniverseId(1),
            first_range: (1, 4),
            second_range: (4, 7),
            overlap: (4, 4),
            ..
        }
    )));
}

#[test]
fn patch_rejects_fixture_that_exceeds_universe() {
    let definition = FixtureDefinition::new(
        "EightChannel",
        8,
        CapabilitySet::empty(),
        FixtureMappings::default(),
    )
    .unwrap();
    let mut library = FixtureLibrary::new();
    library.insert(definition);
    let mut patch = Patch::new("bad");
    patch
        .add_fixture("too_late", "EightChannel", UniverseId(1), 508)
        .unwrap();
    assert!(matches!(
        resolve_patch(&library, &patch).unwrap_err().as_slice(),
        [LinkError::FixtureExceedsUniverse { end: 515, .. }]
    ));
}

#[test]
fn patch_rejects_unknown_definition_and_duplicate_instance() {
    let mut unknown = Patch::new("bad");
    unknown
        .add_fixture("mystery", "Missing", UniverseId(1), 1)
        .unwrap();
    assert!(matches!(
        resolve_patch(&library(), &unknown).unwrap_err().as_slice(),
        [LinkError::UnknownFixtureDefinition { .. }]
    ));

    let mut duplicate = Patch::new("bad");
    duplicate
        .add_fixture("same", "GenericRgbPar", UniverseId(1), 1)
        .unwrap();
    duplicate
        .add_fixture("same", "GenericRgbPar", UniverseId(1), 5)
        .unwrap();
    assert!(resolve_patch(&library(), &duplicate)
        .unwrap_err()
        .iter()
        .any(|error| matches!(error, LinkError::DuplicateFixture { fixture } if fixture == "same")));
}

#[test]
fn binding_rejects_missing_capability() {
    let program =
        lux_compiler::compile_portable(&source("role Washes: Group<Color + Intensity>;")).unwrap();
    let mut patch = Patch::new("venue");
    patch
        .add_fixture("dimmer_1", "SimpleDimmer", UniverseId(1), 1)
        .unwrap();
    let errors = link(&program, &library(), &patch, &washes_rig(&["dimmer_1"])).unwrap_err();
    assert!(errors.iter().any(|error| matches!(
        error,
        LinkError::MissingCapability { missing, .. } if missing == &vec![Capability::Color]
    )));
}

#[test]
fn binding_validation_reports_missing_duplicate_unknown_and_empty_roles() {
    let program = lux_compiler::compile_portable(&source(
        "role Washes: Group<Color + Intensity>; role Movers: Group<Intensity>;",
    ))
    .unwrap();
    let patch = patch_at(5);
    let rig = RigBinding {
        name: "bad".into(),
        contract: "DemoRig".into(),
        bindings: vec![
            RoleBinding {
                role: "Washes".into(),
                fixtures: vec![],
            },
            RoleBinding {
                role: "Washes".into(),
                fixtures: vec!["missing".into()],
            },
            RoleBinding {
                role: "Unknown".into(),
                fixtures: vec!["wash_left".into()],
            },
        ],
    };
    let errors = link(&program, &library(), &patch, &rig).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, LinkError::DuplicateRoleBinding { .. }))
    );
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, LinkError::UnknownRole { .. }))
    );
    assert!(
        errors.iter().any(
            |error| matches!(error, LinkError::MissingRoleBinding { role } if role == "Movers")
        )
    );
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, LinkError::UnknownFixture { .. }))
    );
}

#[test]
fn empty_binding_is_rejected_and_fixture_may_be_shared_across_roles() {
    let one_role =
        lux_compiler::compile_portable(&source("role Washes: Group<Color + Intensity>;")).unwrap();
    let errors = link(&one_role, &library(), &patch_at(5), &washes_rig(&[])).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, LinkError::EmptyBinding { role } if role == "Washes"))
    );

    let shared = lux_compiler::compile_portable(&source(
        "role Washes: Group<Color + Intensity>; role All: Group<Color + Intensity>;",
    ))
    .unwrap();
    let rig = RigBinding {
        name: "shared".into(),
        contract: "DemoRig".into(),
        bindings: vec![
            RoleBinding {
                role: "Washes".into(),
                fixtures: vec!["wash_left".into()],
            },
            RoleBinding {
                role: "All".into(),
                fixtures: vec!["wash_left".into(), "wash_right".into()],
            },
        ],
    };
    let image = link(&shared, &library(), &patch_at(5), &rig).unwrap();
    assert_eq!(image.targets[0].fixtures, vec![FixtureId(0)]);
    assert_eq!(image.targets[1].fixtures, vec![FixtureId(0), FixtureId(1)]);
}

#[test]
fn linker_is_deterministic() {
    let program =
        lux_compiler::compile_portable(&source("role Washes: Group<Color + Intensity>;")).unwrap();
    let first = link(
        &program,
        &library(),
        &patch_at(5),
        &washes_rig(&["wash_left", "wash_right"]),
    )
    .unwrap();
    let second = link(
        &program,
        &library(),
        &patch_at(5),
        &washes_rig(&["wash_left", "wash_right"]),
    )
    .unwrap();
    assert_eq!(first, second);
}
