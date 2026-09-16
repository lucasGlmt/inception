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
}

impl Attribute {
    pub const ALL: &'static [Attribute] = &[Attribute::Intensity, Attribute::Color];

    pub fn name(self) -> &'static str {
        match self {
            Attribute::Intensity => "intensity",
            Attribute::Color => "color",
        }
    }

    /// Maps an attribute name as written in source (e.g. the `intensity`
    /// in `Washes.intensity = 50%;`) to a builtin [`Attribute`]. Returns
    /// `None` for any name that isn't a known attribute.
    pub fn from_name(name: &str) -> Option<Attribute> {
        Self::ALL.iter().copied().find(|attr| attr.name() == name)
    }

    /// The type an assignment to this attribute must produce.
    pub fn value_type(self) -> Type {
        match self {
            Attribute::Intensity => Type::Intensity,
            Attribute::Color => Type::Color,
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
    }
}
