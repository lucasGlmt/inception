//! The builtin lighting attributes.
//!
//! Mirrors [`crate::types::Type`]'s design: `lux-hir` only carries an
//! attribute's raw source name (`HirAssign::attribute_name`), and mapping
//! that name to a real [`Attribute`] — and deciding whether that mapping
//! even succeeds — is entirely this crate's job, kept in one place per
//! `AGENTS.md`'s "single source of truth" rule.

use std::fmt;

use crate::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    Intensity,
    Color,
    Strobe,
}

impl Attribute {
    pub const ALL: &'static [Attribute] =
        &[Attribute::Intensity, Attribute::Color, Attribute::Strobe];

    pub fn name(self) -> &'static str {
        match self {
            Attribute::Intensity => "intensity",
            Attribute::Color => "color",
            Attribute::Strobe => "strobe",
        }
    }

    /// Maps an attribute name as written in source (e.g. the `intensity`
    /// in `Washes.intensity = 50%;`) to a builtin [`Attribute`]. Returns
    /// `None` for any name that isn't a known attribute.
    pub fn from_name(name: &str) -> Option<Attribute> {
        Self::ALL.iter().copied().find(|attr| attr.name() == name)
    }

    /// The type an assignment to this attribute must produce.
    ///
    /// `Strobe` reuses `Intensity` (`0%..=100%`, no strobe rate/frequency):
    /// a strobe's actual flash rate is fixture-specific DMX behavior that
    /// varies per projector model, so V1 exposes it as a percentage-driven
    /// speed/duty knob rather than a `Frequency`/`Hz` value it can't map
    /// consistently across hardware — see `AGENTS.md`'s "hardware-independent
    /// show code" principle.
    pub fn value_type(self) -> Type {
        match self {
            Attribute::Intensity => Type::Intensity,
            Attribute::Color => Type::Color,
            Attribute::Strobe => Type::Intensity,
        }
    }

    /// Capability required to read or write this attribute. Tooling uses
    /// this API instead of maintaining a second attribute/capability table.
    pub fn required_capability(self) -> lux_hir::Capability {
        match self {
            Attribute::Intensity => lux_hir::Capability::Intensity,
            Attribute::Color => lux_hir::Capability::Color,
            Attribute::Strobe => lux_hir::Capability::Strobe,
        }
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_names() {
        for &attr in Attribute::ALL {
            assert_eq!(Attribute::from_name(attr.name()), Some(attr));
        }
    }

    #[test]
    fn unknown_name_is_none() {
        assert_eq!(Attribute::from_name("pan"), None);
    }

    #[test]
    fn value_types_match_the_attribute() {
        assert_eq!(Attribute::Intensity.value_type(), Type::Intensity);
        assert_eq!(Attribute::Color.value_type(), Type::Color);
        assert_eq!(Attribute::Strobe.value_type(), Type::Intensity);
    }
}
