//! Expected expression types exposed to IDE and other compiler clients.
//!
//! Syntax tooling identifies the construct surrounding a cursor; this module
//! answers the semantic question without duplicating Lux typing rules.

use crate::stdlib_bridge::from_param_type;
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

    /// The expected type of the `arg_index`-th argument of
    /// `module_path.name(...)`, for LSP argument-position hints. Returns
    /// `None` when the position is overloaded across candidates with
    /// different types (e.g. `Math.abs`'s single Int-or-Float argument):
    /// there's no single right answer to hint, so the LSP falls back to
    /// generic completion there rather than showing a misleading type.
    pub fn for_call_argument(module_path: &[&str], name: &str, arg_index: usize) -> Option<Self> {
        let candidates = lux_stdlib::candidates(module_path, name);
        if candidates.is_empty() {
            return None;
        }
        let mut types = candidates
            .iter()
            .filter_map(|sig| sig.params.get(arg_index))
            .map(|param| from_param_type(param.ty));
        let first = types.next()?;
        types.all(|ty| ty == first).then_some(Self(first))
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
