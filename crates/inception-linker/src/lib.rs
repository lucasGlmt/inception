//! Build-time linking from portable Lux roles and physical configuration to
//! numeric, runtime-ready fixture and target mappings.

use std::collections::{BTreeMap, BTreeSet};

use inception_core::{FixtureId, LightingState, ResolvedTarget, TargetId, UniverseId};
use inception_renderer::{
    DmxChannel, DmxChannelMapping, ResolvedFixture, ResolvedRig, RgbChannelMapping,
};
use lux_bytecode::{BytecodeModule, PortableRole};
pub use lux_bytecode::{Capability, CapabilitySet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FixtureDefinitionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmxAddress(u16);

impl DmxAddress {
    pub fn new(fixture: &str, value: u16) -> Result<Self, LinkError> {
        if !(1..=512).contains(&value) {
            return Err(LinkError::InvalidDmxAddress {
                fixture: fixture.to_string(),
                address: value,
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbOffsets {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixtureMappings {
    /// Zero-based offset relative to the fixture's one-based DMX address.
    pub intensity: Option<u16>,
    pub color: Option<RgbOffsets>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureDefinition {
    name: String,
    footprint: u16,
    capabilities: CapabilitySet,
    mappings: FixtureMappings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureDefinitionError {
    InvalidFootprint(u16),
    CapabilityMappingMismatch(Capability),
    OffsetOutsideFootprint { offset: u16, footprint: u16 },
    DuplicateChannelOffset(u16),
}

impl FixtureDefinition {
    pub fn new(
        name: impl Into<String>,
        footprint: u16,
        capabilities: CapabilitySet,
        mappings: FixtureMappings,
    ) -> Result<Self, Vec<FixtureDefinitionError>> {
        let mut errors = Vec::new();
        if !(1..=512).contains(&footprint) {
            errors.push(FixtureDefinitionError::InvalidFootprint(footprint));
        }
        if capabilities.contains(Capability::Intensity) != mappings.intensity.is_some() {
            errors.push(FixtureDefinitionError::CapabilityMappingMismatch(
                Capability::Intensity,
            ));
        }
        if capabilities.contains(Capability::Color) != mappings.color.is_some() {
            errors.push(FixtureDefinitionError::CapabilityMappingMismatch(
                Capability::Color,
            ));
        }
        let mut offsets = Vec::new();
        if let Some(offset) = mappings.intensity {
            offsets.push(offset);
        }
        if let Some(rgb) = mappings.color {
            offsets.extend([rgb.red, rgb.green, rgb.blue]);
        }
        for &offset in &offsets {
            if offset >= footprint {
                errors.push(FixtureDefinitionError::OffsetOutsideFootprint { offset, footprint });
            }
        }
        let mut seen = BTreeSet::new();
        for offset in offsets {
            if !seen.insert(offset) {
                errors.push(FixtureDefinitionError::DuplicateChannelOffset(offset));
            }
        }
        if errors.is_empty() {
            Ok(Self {
                name: name.into(),
                footprint,
                capabilities,
                mappings,
            })
        } else {
            Err(errors)
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn footprint(&self) -> u16 {
        self.footprint
    }

    pub const fn capabilities(&self) -> CapabilitySet {
        self.capabilities
    }
}

#[derive(Debug, Clone, Default)]
pub struct FixtureLibrary {
    definitions: Vec<FixtureDefinition>,
}

impl FixtureLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, definition: FixtureDefinition) -> FixtureDefinitionId {
        let id = FixtureDefinitionId(self.definitions.len() as u32);
        self.definitions.push(definition);
        id
    }

    fn resolve(&self, name: &str) -> Option<(FixtureDefinitionId, &FixtureDefinition)> {
        self.definitions
            .iter()
            .position(|definition| definition.name == name)
            .map(|index| (FixtureDefinitionId(index as u32), &self.definitions[index]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFixture {
    pub name: String,
    pub definition: String,
    pub universe: UniverseId,
    pub address: DmxAddress,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    pub name: String,
    pub fixtures: Vec<PatchFixture>,
}

impl Patch {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fixtures: Vec::new(),
        }
    }

    pub fn add_fixture(
        &mut self,
        name: impl Into<String>,
        definition: impl Into<String>,
        universe: UniverseId,
        address: u16,
    ) -> Result<(), LinkError> {
        let name = name.into();
        self.fixtures.push(PatchFixture {
            address: DmxAddress::new(&name, address)?,
            name,
            definition: definition.into(),
            universe,
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPhysicalFixture {
    pub id: FixtureId,
    pub definition: FixtureDefinitionId,
    pub debug_name: String,
    pub capabilities: CapabilitySet,
    pub mapping: ResolvedFixture,
    pub universe: UniverseId,
    pub first_channel: DmxChannel,
    pub last_channel: DmxChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPatch {
    pub fixtures: Vec<ResolvedPhysicalFixture>,
}

impl ResolvedPatch {
    pub fn resolved_rig(&self) -> ResolvedRig {
        ResolvedRig {
            fixtures: self
                .fixtures
                .iter()
                .map(|fixture| fixture.mapping)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleBinding {
    pub role: String,
    pub fixtures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigBinding {
    pub name: String,
    pub contract: String,
    pub bindings: Vec<RoleBinding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeImage {
    pub bytecode: BytecodeModule,
    pub targets: Vec<ResolvedTarget>,
    pub rig: ResolvedRig,
}

impl RuntimeImage {
    pub fn lighting_state(&self) -> LightingState {
        let mut state = LightingState::new();
        for (index, target) in self.targets.iter().enumerate() {
            state.define_target(TargetId(index as u32), target.clone());
        }
        state
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    InvalidDmxAddress {
        fixture: String,
        address: u16,
    },
    UnknownFixtureDefinition {
        fixture: String,
        definition: String,
    },
    DuplicateFixture {
        fixture: String,
    },
    FixtureExceedsUniverse {
        fixture: String,
        universe: UniverseId,
        start: u16,
        end: u32,
        footprint: u16,
    },
    DmxCollision {
        universe: UniverseId,
        first_fixture: String,
        first_range: (u16, u16),
        second_fixture: String,
        second_range: (u16, u16),
        overlap: (u16, u16),
    },
    MissingRigContract,
    ContractMismatch {
        expected: String,
        found: String,
    },
    UnknownRole {
        role: String,
    },
    MissingRoleBinding {
        role: String,
    },
    DuplicateRoleBinding {
        role: String,
    },
    UnknownFixture {
        role: String,
        fixture: String,
    },
    MissingCapability {
        role: String,
        fixture: String,
        missing: Vec<Capability>,
    },
    EmptyBinding {
        role: String,
    },
    InvalidProgram,
}

/// Phase A: validates and resolves fixture models, instances, addresses, and
/// semantic attribute mappings without involving any role.
pub fn resolve_patch(
    library: &FixtureLibrary,
    patch: &Patch,
) -> Result<ResolvedPatch, Vec<LinkError>> {
    let mut errors = Vec::new();
    let mut names = BTreeSet::new();
    let mut fixtures = Vec::new();

    for (index, instance) in patch.fixtures.iter().enumerate() {
        if !names.insert(instance.name.clone()) {
            errors.push(LinkError::DuplicateFixture {
                fixture: instance.name.clone(),
            });
            continue;
        }
        let Some((definition_id, definition)) = library.resolve(&instance.definition) else {
            errors.push(LinkError::UnknownFixtureDefinition {
                fixture: instance.name.clone(),
                definition: instance.definition.clone(),
            });
            continue;
        };
        let start = instance.address.get();
        let end = start as u32 + definition.footprint as u32 - 1;
        if end > 512 {
            errors.push(LinkError::FixtureExceedsUniverse {
                fixture: instance.name.clone(),
                universe: instance.universe,
                start,
                end,
                footprint: definition.footprint,
            });
            continue;
        }
        let channel = |offset: u16| {
            DmxChannel::new(start + offset)
                .expect("validated fixture footprint guarantees an in-range channel")
        };
        let intensity = definition
            .mappings
            .intensity
            .map(|offset| DmxChannelMapping {
                universe: instance.universe,
                channel: channel(offset),
            });
        let color = definition.mappings.color.map(|offsets| RgbChannelMapping {
            universe: instance.universe,
            red: channel(offsets.red),
            green: channel(offsets.green),
            blue: channel(offsets.blue),
        });
        let id = FixtureId(index as u32);
        fixtures.push(ResolvedPhysicalFixture {
            id,
            definition: definition_id,
            debug_name: instance.name.clone(),
            capabilities: definition.capabilities,
            mapping: ResolvedFixture {
                id,
                intensity,
                color,
            },
            universe: instance.universe,
            first_channel: channel(0),
            last_channel: channel(definition.footprint - 1),
        });
    }

    for (left_index, left) in fixtures.iter().enumerate() {
        for right in fixtures.iter().skip(left_index + 1) {
            if left.universe != right.universe {
                continue;
            }
            let overlap_start = left.first_channel.get().max(right.first_channel.get());
            let overlap_end = left.last_channel.get().min(right.last_channel.get());
            if overlap_start <= overlap_end {
                errors.push(LinkError::DmxCollision {
                    universe: left.universe,
                    first_fixture: left.debug_name.clone(),
                    first_range: (left.first_channel.get(), left.last_channel.get()),
                    second_fixture: right.debug_name.clone(),
                    second_range: (right.first_channel.get(), right.last_channel.get()),
                    overlap: (overlap_start, overlap_end),
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(ResolvedPatch { fixtures })
    } else {
        Err(errors)
    }
}

/// Phase B: links portable roles to the already validated physical patch.
pub fn link(
    program: &BytecodeModule,
    library: &FixtureLibrary,
    patch: &Patch,
    rig: &RigBinding,
) -> Result<RuntimeImage, Vec<LinkError>> {
    if lux_bytecode::verify(program).is_err() {
        return Err(vec![LinkError::InvalidProgram]);
    }
    let resolved_patch = resolve_patch(library, patch)?;
    let Some(contract) = &program.rig_contract else {
        return Err(vec![LinkError::MissingRigContract]);
    };
    let mut errors = Vec::new();
    if rig.contract != contract.name {
        errors.push(LinkError::ContractMismatch {
            expected: contract.name.clone(),
            found: rig.contract.clone(),
        });
    }

    let fixtures_by_name: BTreeMap<_, _> = resolved_patch
        .fixtures
        .iter()
        .map(|fixture| (fixture.debug_name.as_str(), fixture))
        .collect();
    let roles_by_name: BTreeMap<_, _> = contract
        .roles
        .iter()
        .map(|role| (role.name.as_str(), role))
        .collect();
    let mut bindings_by_role: BTreeMap<&str, &RoleBinding> = BTreeMap::new();
    for binding in &rig.bindings {
        if !roles_by_name.contains_key(binding.role.as_str()) {
            errors.push(LinkError::UnknownRole {
                role: binding.role.clone(),
            });
            continue;
        }
        if bindings_by_role
            .insert(binding.role.as_str(), binding)
            .is_some()
        {
            errors.push(LinkError::DuplicateRoleBinding {
                role: binding.role.clone(),
            });
        }
    }

    let mut targets = vec![ResolvedTarget::default(); program.target_count as usize];
    for role in &contract.roles {
        let Some(binding) = bindings_by_role.get(role.name.as_str()) else {
            errors.push(LinkError::MissingRoleBinding {
                role: role.name.clone(),
            });
            continue;
        };
        bind_role(role, binding, &fixtures_by_name, &mut targets, &mut errors);
    }

    if errors.is_empty() {
        Ok(RuntimeImage {
            bytecode: program.clone(),
            targets,
            rig: resolved_patch.resolved_rig(),
        })
    } else {
        Err(errors)
    }
}

fn bind_role(
    role: &PortableRole,
    binding: &RoleBinding,
    fixtures_by_name: &BTreeMap<&str, &ResolvedPhysicalFixture>,
    targets: &mut [ResolvedTarget],
    errors: &mut Vec<LinkError>,
) {
    if binding.fixtures.is_empty() {
        errors.push(LinkError::EmptyBinding {
            role: role.name.clone(),
        });
        return;
    }
    let mut fixture_ids = Vec::new();
    for fixture_name in &binding.fixtures {
        let Some(fixture) = fixtures_by_name.get(fixture_name.as_str()) else {
            errors.push(LinkError::UnknownFixture {
                role: role.name.clone(),
                fixture: fixture_name.clone(),
            });
            continue;
        };
        if !fixture.capabilities.is_superset(role.required_capabilities) {
            let missing = role
                .required_capabilities
                .iter()
                .filter(|capability| !fixture.capabilities.contains(*capability))
                .collect();
            errors.push(LinkError::MissingCapability {
                role: role.name.clone(),
                fixture: fixture_name.clone(),
                missing,
            });
            continue;
        }
        fixture_ids.push(fixture.id);
    }
    if let Some(target) = targets.get_mut(role.target.0 as usize) {
        target.fixtures = fixture_ids;
    } else {
        errors.push(LinkError::InvalidProgram);
    }
}
