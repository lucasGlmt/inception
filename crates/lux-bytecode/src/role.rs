//! Portable rig requirements embedded in compiled Lux bytecode.

use crate::TargetId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Intensity,
    Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilitySet(u8);

impl CapabilitySet {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b11)
    }

    pub const fn contains(self, capability: Capability) -> bool {
        let bit = match capability {
            Capability::Intensity => 1,
            Capability::Color => 2,
        };
        self.0 & bit != 0
    }

    pub fn insert(&mut self, capability: Capability) {
        self.0 |= match capability {
            Capability::Intensity => 1,
            Capability::Color => 2,
        };
    }

    pub fn from_capabilities(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        let mut set = Self::empty();
        for capability in capabilities {
            set.insert(capability);
        }
        set
    }

    pub const fn is_superset(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    pub fn iter(self) -> impl Iterator<Item = Capability> {
        [Capability::Intensity, Capability::Color]
            .into_iter()
            .filter(move |capability| self.contains(*capability))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableRole {
    pub id: RoleId,
    pub target: TargetId,
    /// Link-time/debug metadata only; never read by the VM hot path.
    pub name: String,
    pub required_capabilities: CapabilitySet,
    pub cardinality: RoleCardinality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleCardinality {
    GroupNonEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableRigContract {
    pub name: String,
    pub roles: Vec<PortableRole>,
}
