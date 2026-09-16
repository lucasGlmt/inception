//! The builtin Lux types.
//!
//! `lux-typeck` is the single source of truth for what a "type" is in
//! Lux. `lux-syntax` and `lux-hir` only carry type *names* (raw
//! identifiers written in source); mapping a name to an actual [`Type`],
//! and deciding whether that mapping succeeds, is entirely this crate's
//! responsibility.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Bool,
    Int,
    Float,
    Duration,
    Intensity,
    Color,
    Angle,
    Frequency,
    Tempo,
}

impl Type {
    pub const ALL: &'static [Type] = &[
        Type::Bool,
        Type::Int,
        Type::Float,
        Type::Duration,
        Type::Intensity,
        Type::Color,
        Type::Angle,
        Type::Frequency,
        Type::Tempo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Type::Bool => "Bool",
            Type::Int => "Int",
            Type::Float => "Float",
            Type::Duration => "Duration",
            Type::Intensity => "Intensity",
            Type::Color => "Color",
            Type::Angle => "Angle",
            Type::Frequency => "Frequency",
            Type::Tempo => "Tempo",
        }
    }

    /// Maps a type name as written in source (e.g. the `Duration` in
    /// `let x: Duration = ...`) to a builtin [`Type`]. Returns `None` for
    /// any name that isn't a known builtin type.
    pub fn from_name(name: &str) -> Option<Type> {
        Self::ALL.iter().copied().find(|ty| ty.name() == name)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_names() {
        for &ty in Type::ALL {
            assert_eq!(Type::from_name(ty.name()), Some(ty));
        }
    }

    #[test]
    fn unknown_name_is_none() {
        assert_eq!(Type::from_name("Fixture"), None);
    }
}
