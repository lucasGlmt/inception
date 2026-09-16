//! Static range checks for bounded value types.
//!
//! Only `Intensity` (`0..=100`) is bounded today, but new bounded types
//! (e.g. a future `0..=360` for `Angle`) only need a new match arm here —
//! callers just ask [`bound_for`] for a type's bound and don't need to
//! know which types happen to have one.

use crate::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    pub min: i64,
    pub max: i64,
}

impl Bound {
    pub fn contains(self, value: i64) -> bool {
        value >= self.min && value <= self.max
    }
}

pub fn bound_for(ty: Type) -> Option<Bound> {
    match ty {
        Type::Intensity => Some(Bound { min: 0, max: 100 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intensity_is_bounded_zero_to_hundred() {
        let bound = bound_for(Type::Intensity).unwrap();
        assert!(bound.contains(0));
        assert!(bound.contains(100));
        assert!(!bound.contains(101));
        assert!(!bound.contains(-1));
    }

    #[test]
    fn duration_has_no_bound() {
        assert_eq!(bound_for(Type::Duration), None);
    }
}
