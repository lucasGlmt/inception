//! The semantic lighting state: what's currently commanded, in domain
//! terms — never DMX. See `AGENTS.md`'s and this milestone's separation:
//!
//! ```text
//! Lux / VM -> semantic lighting state -> renderer -> resolved physical
//! mapping -> DMX
//! ```
//!
//! `LightingState` only knows [`FixtureId`]s and [`TargetId`]s; it has no
//! notion of a DMX channel, universe or protocol.

use std::collections::HashMap;

use crate::attribute::AttributeValue;
use crate::color::Rgb;
use crate::ids::{FixtureId, TargetId};
use crate::intensity::Intensity;

/// A target already resolved to the fixtures it addresses. This
/// `inception-linker` produces these as part of a venue-specific
/// `RuntimeImage`; lower-level unit tests may still construct them directly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedTarget {
    pub fixtures: Vec<FixtureId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingError {
    UnknownTarget(TargetId),
}

/// The semantic lighting state, keyed by [`FixtureId`].
///
/// A fixture that has never received a value for some attribute reads
/// back as that attribute's explicit default (`Intensity::ZERO`,
/// `Rgb::BLACK`) rather than as "unknown" — there is no uninitialized
/// state to observe (item 37 of the task brief). Backed by `HashMap`s,
/// but that's safe for determinism (item 38): nothing here ever iterates
/// them in a way whose result depends on iteration order — every read is
/// a keyed lookup by a specific `FixtureId`/`TargetId`, driven by
/// whatever external, already-ordered structure (e.g. the renderer's
/// `ResolvedRig`) is walking fixtures.
#[derive(Debug, Clone, Default)]
pub struct LightingState {
    targets: HashMap<TargetId, ResolvedTarget>,
    intensities: HashMap<FixtureId, Intensity>,
    colors: HashMap<FixtureId, Rgb>,
}

impl LightingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares what a target resolves to. Called once during setup (by
    /// `RuntimeImage` setup) — not part of the per-frame command path.
    pub fn define_target(&mut self, id: TargetId, target: ResolvedTarget) {
        self.targets.insert(id, target);
    }

    /// Returns the already-resolved fixture IDs for a target. Transition
    /// creation uses this once per command; sampling remains fixture-ID based.
    pub fn target_fixtures(&self, target: TargetId) -> Result<&[FixtureId], LightingError> {
        self.targets
            .get(&target)
            .map(|resolved| resolved.fixtures.as_slice())
            .ok_or(LightingError::UnknownTarget(target))
    }

    /// Applies `value` to every fixture `target` resolves to.
    pub fn set_target_attribute(
        &mut self,
        target: TargetId,
        value: AttributeValue,
    ) -> Result<(), LightingError> {
        let resolved = self
            .targets
            .get(&target)
            .ok_or(LightingError::UnknownTarget(target))?;
        for &fixture in &resolved.fixtures {
            match value {
                AttributeValue::Intensity(i) => {
                    self.intensities.insert(fixture, i);
                }
                AttributeValue::Color(c) => {
                    self.colors.insert(fixture, c);
                }
            }
        }
        Ok(())
    }

    /// Applies `value` to exactly one fixture, bypassing target
    /// resolution. Mainly useful for tests that want to check a single
    /// fixture's propagation directly.
    pub fn set_fixture_attribute(&mut self, fixture: FixtureId, value: AttributeValue) {
        match value {
            AttributeValue::Intensity(i) => {
                self.intensities.insert(fixture, i);
            }
            AttributeValue::Color(c) => {
                self.colors.insert(fixture, c);
            }
        }
    }

    /// `Intensity::ZERO` if `fixture` was never set — see this struct's
    /// docs on why that's the explicit default rather than an error.
    pub fn intensity(&self, fixture: FixtureId) -> Intensity {
        self.intensities
            .get(&fixture)
            .copied()
            .unwrap_or(Intensity::ZERO)
    }

    /// `Rgb::BLACK` if `fixture` was never set.
    pub fn color(&self, fixture: FixtureId) -> Rgb {
        self.colors.get(&fixture).copied().unwrap_or(Rgb::BLACK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::Attribute;

    #[test]
    fn target_attribute_propagates_to_every_resolved_fixture() {
        let mut state = LightingState::new();
        state.define_target(
            TargetId(0),
            ResolvedTarget {
                fixtures: vec![FixtureId(0), FixtureId(1)],
            },
        );

        state
            .set_target_attribute(
                TargetId(0),
                AttributeValue::Intensity(Intensity::from_percent(50).unwrap()),
            )
            .expect("target should be known");

        assert_eq!(
            state.intensity(FixtureId(0)),
            Intensity::from_percent(50).unwrap()
        );
        assert_eq!(
            state.intensity(FixtureId(1)),
            Intensity::from_percent(50).unwrap()
        );
    }

    #[test]
    fn unknown_target_is_an_error() {
        let mut state = LightingState::new();
        let err = state
            .set_target_attribute(TargetId(0), AttributeValue::Intensity(Intensity::ZERO))
            .unwrap_err();
        assert_eq!(err, LightingError::UnknownTarget(TargetId(0)));
    }

    #[test]
    fn never_set_fixture_defaults_to_zero_intensity_and_black() {
        let state = LightingState::new();
        assert_eq!(state.intensity(FixtureId(0)), Intensity::ZERO);
        assert_eq!(state.color(FixtureId(0)), Rgb::BLACK);
    }

    #[test]
    fn setting_one_fixture_does_not_affect_another() {
        let mut state = LightingState::new();
        state.define_target(
            TargetId(0),
            ResolvedTarget {
                fixtures: vec![FixtureId(0), FixtureId(1), FixtureId(2)],
            },
        );
        state.set_fixture_attribute(FixtureId(1), AttributeValue::Intensity(Intensity::MAX));

        assert_eq!(state.intensity(FixtureId(0)), Intensity::ZERO);
        assert_eq!(state.intensity(FixtureId(1)), Intensity::MAX);
        assert_eq!(state.intensity(FixtureId(2)), Intensity::ZERO);
    }

    #[test]
    fn attribute_value_reports_its_own_attribute() {
        assert_eq!(
            AttributeValue::Intensity(Intensity::ZERO).attribute(),
            Attribute::Intensity
        );
        assert_eq!(
            AttributeValue::Color(Rgb::BLACK).attribute(),
            Attribute::Color
        );
    }
}
