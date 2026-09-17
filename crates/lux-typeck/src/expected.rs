//! Expected expression types exposed to IDE and other compiler clients.
//!
//! Syntax tooling identifies the construct surrounding a cursor; this module
//! answers the semantic question without duplicating Lux typing rules.

use crate::stdlib_bridge::from_param_type;
use crate::types::SignalElement;
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

    /// The expected type on the right of `<-` for `<target>.<name> <- `,
    /// i.e. `Signal<T>` where `T` is the same type [`Self::for_attribute`]
    /// would report for `name` — every attribute's value type is a valid
    /// `Signal` element (see `SignalElement`'s docs), so this never fails
    /// for a name `for_attribute` itself accepts.
    pub fn for_signal_binding(name: &str) -> Option<Self> {
        let attribute = Attribute::from_name(name)?;
        let element = SignalElement::from_type(attribute.value_type())?;
        Some(Self(Type::Signal(element)))
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
        Self::from_candidates(lux_stdlib::candidates(module_path, name), arg_index)
    }

    /// The method-call counterpart to [`Self::for_call_argument`] — the
    /// expected type of the `arg_index`-th argument of
    /// `<Signal<Float> receiver>.name(...)`. This is exactly what makes
    /// `wave.range($0` hint `Intensity` for both arguments once the first
    /// one is: every `range` overload requires both bounds to share the
    /// same type (see `lux_stdlib::methods`'s docs), so once the first
    /// argument narrows which overload applies, the second is no longer
    /// ambiguous either.
    pub fn for_signal_float_method_argument(name: &str, arg_index: usize) -> Option<Self> {
        Self::from_candidates(lux_stdlib::signal_float_method_candidates(name), arg_index)
    }

    fn from_candidates(candidates: &[lux_stdlib::Signature], arg_index: usize) -> Option<Self> {
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
