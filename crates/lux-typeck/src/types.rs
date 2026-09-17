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
    /// `Signal<T>` for one of the 5 element types `SignalElement` allows.
    /// Kept as a separate, non-recursive payload enum rather than
    /// `Box<Type>` specifically so `Type` itself can stay `Copy` — see
    /// [`SignalElement`]'s docs.
    Signal(SignalElement),
    /// `Sequence<T>` for one of the 5 element types `SequenceElement`
    /// allows — an immutable, ordered collection (see
    /// `docs/rfcs` and `crate::checker::Checker::check_call`'s
    /// `std.Sequence.of` handling). Same non-recursive-payload trick as
    /// `Signal(SignalElement)`, for the same reason: it keeps `Type`
    /// itself `Copy` and makes `Sequence<Sequence<T>>` structurally
    /// unrepresentable, which is correct — nesting sequences is out of
    /// scope for this milestone.
    Sequence(SequenceElement),
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

    /// The head type name — for `Signal(_)` this is just `"Signal"`, with
    /// no element type decoration (use `Display` for the full
    /// `Signal<Intensity>`-style rendering). Only the 9 flat base variants
    /// round-trip through [`Type::from_name`]/[`Type::ALL`]; `"Signal"`
    /// alone is never a valid standalone type name (see `from_name`'s
    /// docs), so this asymmetry is deliberate, not an oversight.
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
            Type::Signal(_) => "Signal",
            Type::Sequence(_) => "Sequence",
        }
    }

    /// Maps a *non-generic* type name as written in source (e.g. the
    /// `Duration` in `let x: Duration = ...`) to a builtin [`Type`].
    /// Returns `None` for any name that isn't a known builtin base type —
    /// notably including `"Signal"`, which only ever exists as
    /// `Type::Signal(_)`, never on its own; resolving a full,
    /// possibly-generic annotation is [`resolve_annotation`]'s job.
    pub fn from_name(name: &str) -> Option<Type> {
        Self::ALL.iter().copied().find(|ty| ty.name() == name)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Signal(elem) => write!(f, "Signal<{elem}>"),
            Type::Sequence(elem) => write!(f, "Sequence<{elem}>"),
            other => f.write_str(other.name()),
        }
    }
}

/// The element types `Signal<T>` may wrap in V1 — exactly the 5 types
/// `lux-stdlib`'s `ParamType` can represent (see `crate::stdlib_bridge`),
/// since `Signal.constant` is currently the only way to construct a
/// signal and it's built on that machinery. `Bool`/`Duration`/`Frequency`/
/// `Tempo` have no stdlib representation at all today (not just for
/// `Signal`), so leaving them out here is consistent, not an arbitrary
/// restriction — extending `Signal<T>` to them later is the same
/// mechanical step as giving `ParamType` more variants.
///
/// Kept as its own enum instead of reusing `Type` recursively so `Type`
/// can stay `Copy`: a `Type::Signal(Box<Type>)` payload would strip that
/// from every use of `Type`, not just `Signal`'s. As a side effect, this
/// also makes `Signal<Signal<T>>` structurally unrepresentable — appropriate
/// since signal composition is explicitly out of scope for this milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalElement {
    Int,
    Float,
    Angle,
    Intensity,
    Color,
}

impl SignalElement {
    pub const ALL: &'static [SignalElement] = &[
        SignalElement::Int,
        SignalElement::Float,
        SignalElement::Angle,
        SignalElement::Intensity,
        SignalElement::Color,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SignalElement::Int => "Int",
            SignalElement::Float => "Float",
            SignalElement::Angle => "Angle",
            SignalElement::Intensity => "Intensity",
            SignalElement::Color => "Color",
        }
    }

    pub fn as_type(self) -> Type {
        match self {
            SignalElement::Int => Type::Int,
            SignalElement::Float => Type::Float,
            SignalElement::Angle => Type::Angle,
            SignalElement::Intensity => Type::Intensity,
            SignalElement::Color => Type::Color,
        }
    }

    /// The inverse of [`SignalElement::as_type`]. `None` for `Bool`,
    /// `Duration`, `Frequency`, `Tempo` and `Signal(_)` itself — none of
    /// those are valid inside `Signal<...>` in V1.
    pub fn from_type(ty: Type) -> Option<SignalElement> {
        Self::ALL.iter().copied().find(|e| e.as_type() == ty)
    }
}

impl fmt::Display for SignalElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The element types `Sequence<T>` may wrap in V1 — the same 5 types as
/// [`SignalElement`] (see that type's docs for why this isn't just `Type`
/// recursively): `Sequence.of` is currently the only way to construct a
/// sequence, and it's built on the same `lux-stdlib` machinery
/// `Signal.constant` is (see `crate::stdlib_bridge`). Kept as its own enum
/// rather than reusing `SignalElement` directly so `Sequence<T>` and
/// `Signal<T>` stay independently extensible — nothing here assumes their
/// element universes must always match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceElement {
    Int,
    Float,
    Angle,
    Intensity,
    Color,
}

impl SequenceElement {
    pub const ALL: &'static [SequenceElement] = &[
        SequenceElement::Int,
        SequenceElement::Float,
        SequenceElement::Angle,
        SequenceElement::Intensity,
        SequenceElement::Color,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SequenceElement::Int => "Int",
            SequenceElement::Float => "Float",
            SequenceElement::Angle => "Angle",
            SequenceElement::Intensity => "Intensity",
            SequenceElement::Color => "Color",
        }
    }

    pub fn as_type(self) -> Type {
        match self {
            SequenceElement::Int => Type::Int,
            SequenceElement::Float => Type::Float,
            SequenceElement::Angle => Type::Angle,
            SequenceElement::Intensity => Type::Intensity,
            SequenceElement::Color => Type::Color,
        }
    }

    /// The inverse of [`SequenceElement::as_type`]. `None` for `Bool`,
    /// `Duration`, `Frequency`, `Tempo`, `Signal(_)` and `Sequence(_)`
    /// itself — none of those are valid inside `Sequence<...>` in V1.
    pub fn from_type(ty: Type) -> Option<SequenceElement> {
        Self::ALL.iter().copied().find(|e| e.as_type() == ty)
    }
}

impl fmt::Display for SequenceElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Resolves a full (possibly-generic) type annotation to a [`Type`] —
/// the generic-aware counterpart to [`Type::from_name`], which only
/// handles flat base names. This is the single place that decides
/// `Signal`/`Sequence` are the generic type names Lux currently has, and
/// which element types each may wrap.
pub fn resolve_annotation(ann: &lux_hir::TypeAnnotation) -> Result<Type, crate::error::TypeError> {
    use crate::error::TypeError;

    if ann.name == "Signal" {
        let [arg] = ann.type_args.as_slice() else {
            return Err(TypeError::new(
                format!(
                    "`Signal` expects exactly one type argument, found {}",
                    ann.type_args.len()
                ),
                ann.span,
            ));
        };
        if !arg.type_args.is_empty() {
            return Err(TypeError::new(
                "`Signal<Signal<...>>` is not supported — signals cannot wrap other signals",
                arg.span,
            ));
        }
        let inner = resolve_annotation(arg)?;
        return SignalElement::from_type(inner)
            .map(Type::Signal)
            .ok_or_else(|| {
                TypeError::new(format!("`Signal<{inner}>` is not supported"), arg.span)
                    .with_help("`Signal` may wrap `Int`, `Float`, `Angle`, `Intensity` or `Color`")
            });
    }

    if ann.name == "Sequence" {
        let [arg] = ann.type_args.as_slice() else {
            return Err(TypeError::new(
                format!(
                    "`Sequence` expects exactly one type argument, found {}",
                    ann.type_args.len()
                ),
                ann.span,
            ));
        };
        if !arg.type_args.is_empty() {
            return Err(TypeError::new(
                "`Sequence<Sequence<...>>` is not supported — sequences cannot wrap other sequences",
                arg.span,
            ));
        }
        let inner = resolve_annotation(arg)?;
        return SequenceElement::from_type(inner)
            .map(Type::Sequence)
            .ok_or_else(|| {
                TypeError::new(format!("`Sequence<{inner}>` is not supported"), arg.span).with_help(
                    "`Sequence` may wrap `Int`, `Float`, `Angle`, `Intensity` or `Color`",
                )
            });
    }

    if !ann.type_args.is_empty() {
        return Err(TypeError::new(
            format!("`{}` does not take type arguments", ann.name),
            ann.span,
        ));
    }

    Type::from_name(&ann.name)
        .ok_or_else(|| TypeError::new(format!("unknown type `{}`", ann.name), ann.span))
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

    #[test]
    fn signal_element_round_trips() {
        for &elem in SignalElement::ALL {
            assert_eq!(SignalElement::from_type(elem.as_type()), Some(elem));
        }
    }

    #[test]
    fn non_signal_element_types_are_not_signal_elements() {
        for ty in [Type::Bool, Type::Duration, Type::Frequency, Type::Tempo] {
            assert_eq!(SignalElement::from_type(ty), None);
        }
        assert_eq!(
            SignalElement::from_type(Type::Signal(SignalElement::Int)),
            None
        );
    }

    #[test]
    fn signal_type_displays_with_element() {
        assert_eq!(
            Type::Signal(SignalElement::Intensity).to_string(),
            "Signal<Intensity>"
        );
        assert_eq!(
            Type::Signal(SignalElement::Color).to_string(),
            "Signal<Color>"
        );
    }

    #[test]
    fn sequence_element_round_trips() {
        for &elem in SequenceElement::ALL {
            assert_eq!(SequenceElement::from_type(elem.as_type()), Some(elem));
        }
    }

    #[test]
    fn non_sequence_element_types_are_not_sequence_elements() {
        for ty in [Type::Bool, Type::Duration, Type::Frequency, Type::Tempo] {
            assert_eq!(SequenceElement::from_type(ty), None);
        }
        assert_eq!(
            SequenceElement::from_type(Type::Sequence(SequenceElement::Int)),
            None
        );
        assert_eq!(
            SequenceElement::from_type(Type::Signal(SignalElement::Int)),
            None
        );
    }

    #[test]
    fn sequence_type_displays_with_element() {
        assert_eq!(
            Type::Sequence(SequenceElement::Color).to_string(),
            "Sequence<Color>"
        );
        assert_eq!(
            Type::Sequence(SequenceElement::Intensity).to_string(),
            "Sequence<Intensity>"
        );
    }

    #[test]
    fn resolve_sequence_annotation() {
        let ann = lux_hir::TypeAnnotation {
            name: "Sequence".into(),
            span: lux_syntax::Span::new(0, 0),
            type_args: vec![lux_hir::TypeAnnotation {
                name: "Color".into(),
                span: lux_syntax::Span::new(0, 0),
                type_args: vec![],
            }],
        };
        assert_eq!(
            resolve_annotation(&ann),
            Ok(Type::Sequence(SequenceElement::Color))
        );
    }

    #[test]
    fn resolve_nested_sequence_annotation_is_rejected() {
        let ann = lux_hir::TypeAnnotation {
            name: "Sequence".into(),
            span: lux_syntax::Span::new(0, 0),
            type_args: vec![lux_hir::TypeAnnotation {
                name: "Sequence".into(),
                span: lux_syntax::Span::new(0, 0),
                type_args: vec![lux_hir::TypeAnnotation {
                    name: "Color".into(),
                    span: lux_syntax::Span::new(0, 0),
                    type_args: vec![],
                }],
            }],
        };
        assert!(resolve_annotation(&ann).is_err());
    }
}
