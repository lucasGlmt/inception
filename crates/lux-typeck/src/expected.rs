//! Expected expression types exposed to IDE and other compiler clients.
//!
//! Syntax tooling identifies the construct surrounding a cursor; this module
//! answers the semantic question without duplicating Lux typing rules.

use crate::{Attribute, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedType(Type);

impl ExpectedType {
    pub const fn exact(ty: Type) -> Self {
        Self(ty)
    }

    pub const fn ty(self) -> Type {
        self.0
    }

    pub fn for_attribute(name: &str) -> Option<Self> {
        Attribute::from_name(name).map(|attribute| Self(attribute.value_type()))
    }

    pub const fn transition_duration() -> Self {
        Self(Type::Duration)
    }

    pub const fn wait_value() -> Self {
        Self(Type::Duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_types_reuse_attribute_rules() {
        assert_eq!(
            ExpectedType::for_attribute("intensity").unwrap().ty(),
            Type::Intensity
        );
        assert_eq!(
            ExpectedType::for_attribute("color").unwrap().ty(),
            Type::Color
        );
        assert_eq!(ExpectedType::transition_duration().ty(), Type::Duration);
    }
}
