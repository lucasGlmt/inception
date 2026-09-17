//! Absolute-time lighting transitions.
//!
//! Sampling derives every value directly from `(starts_at, ends_at, now)`;
//! there is no tick counter and therefore no drift when frames are skipped.

use std::collections::HashMap;

use crate::{
    Attribute, AttributeValue, Duration, FixtureId, Intensity, LightingError, LightingState,
    TargetId, Timestamp,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveTransition {
    pub fixture: FixtureId,
    pub attribute: Attribute,
    pub from: AttributeValue,
    pub to: AttributeValue,
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    UnknownTarget(TargetId),
    UnsupportedAttribute(Attribute),
    InvalidTransitionValue,
    ClockOverflow,
}

impl From<LightingError> for TransitionError {
    fn from(value: LightingError) -> Self {
        match value {
            LightingError::UnknownTarget(target) => TransitionError::UnknownTarget(target),
        }
    }
}

/// At most one transition exists for a `(fixture, attribute)` key.
#[derive(Debug, Clone, Default)]
pub struct TransitionEngine {
    active: HashMap<(FixtureId, Attribute), ActiveTransition>,
}

impl TransitionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Applies an immediate set and cancels transitions for the addressed
    /// fixture/attribute pairs.
    pub fn set_target_attribute(
        &mut self,
        target: TargetId,
        value: AttributeValue,
        state: &mut LightingState,
    ) -> Result<(), TransitionError> {
        let fixtures = state.target_fixtures(target)?.to_vec();
        let attribute = value.attribute();
        for fixture in fixtures {
            self.active.remove(&(fixture, attribute));
            state.set_fixture_attribute(fixture, value);
        }
        Ok(())
    }

    /// Starts one independent transition per fixture. An existing transition
    /// on the same key is sampled at `now` before being replaced.
    pub fn start_transition(
        &mut self,
        now: Timestamp,
        target: TargetId,
        to: AttributeValue,
        duration: Duration,
        state: &mut LightingState,
    ) -> Result<(), TransitionError> {
        let attribute = to.attribute();
        if attribute != Attribute::Intensity {
            return Err(TransitionError::UnsupportedAttribute(attribute));
        }
        let fixtures = state.target_fixtures(target)?.to_vec();

        if duration == Duration::ZERO {
            for fixture in fixtures {
                self.active.remove(&(fixture, attribute));
                state.set_fixture_attribute(fixture, to);
            }
            return Ok(());
        }

        let ends_at = Timestamp(
            now.0
                .checked_add(duration.0)
                .ok_or(TransitionError::ClockOverflow)?,
        );
        for fixture in fixtures {
            let key = (fixture, attribute);
            if let Some(previous) = self.active.remove(&key) {
                let effective = sample_value(previous, now)?;
                state.set_fixture_attribute(fixture, effective);
            }
            let from = match attribute {
                Attribute::Intensity => AttributeValue::Intensity(state.intensity(fixture)),
                Attribute::Color | Attribute::Strobe => {
                    return Err(TransitionError::UnsupportedAttribute(attribute));
                }
            };
            self.active.insert(
                key,
                ActiveTransition {
                    fixture,
                    attribute,
                    from,
                    to,
                    starts_at: now,
                    ends_at,
                },
            );
        }
        Ok(())
    }

    /// Cancels any transition active on exactly this `(fixture, attribute)`
    /// key, first sampling it at `now` and writing that value into `state`
    /// so nothing is lost — a no-op if no transition is active there. Used
    /// when a signal binding (`<-`) is about to take over control of the
    /// attribute: the caller installs its own value into `state`
    /// immediately afterward, so the value written here is only ever
    /// momentarily visible, but this keeps the transition's own
    /// bookkeeping (its `active` entry) consistent rather than just
    /// discarding it silently.
    pub fn cancel(
        &mut self,
        now: Timestamp,
        fixture: FixtureId,
        attribute: Attribute,
        state: &mut LightingState,
    ) -> Result<(), TransitionError> {
        if let Some(transition) = self.active.remove(&(fixture, attribute)) {
            let value = sample_value(transition, now)?;
            state.set_fixture_attribute(fixture, value);
        }
        Ok(())
    }

    /// Resolves all active overlays at `now` into `state`. Completed entries
    /// write their exact target value and are removed in-place.
    pub fn sample(
        &mut self,
        now: Timestamp,
        state: &mut LightingState,
    ) -> Result<(), TransitionError> {
        let mut error = None;
        self.active.retain(|_, transition| {
            match sample_value(*transition, now) {
                Ok(value) => state.set_fixture_attribute(transition.fixture, value),
                Err(err) => error = Some(err),
            }
            now < transition.ends_at && error.is_none()
        });
        match error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

fn sample_value(
    transition: ActiveTransition,
    now: Timestamp,
) -> Result<AttributeValue, TransitionError> {
    if now <= transition.starts_at {
        return Ok(transition.from);
    }
    if now >= transition.ends_at {
        return Ok(transition.to);
    }

    match (transition.from, transition.to) {
        (AttributeValue::Intensity(from), AttributeValue::Intensity(to)) => {
            let elapsed = now.0 - transition.starts_at.0;
            let total = transition.ends_at.0 - transition.starts_at.0;
            Ok(AttributeValue::Intensity(interpolate_intensity(
                from, to, elapsed, total,
            )))
        }
        _ => Err(TransitionError::InvalidTransitionValue),
    }
}

fn interpolate_intensity(from: Intensity, to: Intensity, elapsed: u64, total: u64) -> Intensity {
    // `elapsed` and `total` may span the full u64 timestamp range, so the
    // multiplication needs u128 even though intensity itself is only u16.
    let from = from.raw() as u128;
    let to = to.raw() as u128;
    let distance = from.abs_diff(to);
    let elapsed = elapsed as u128;
    let total = total as u128;
    let travelled = (distance * elapsed + total / 2) / total;
    let raw = if to >= from {
        from + travelled
    } else {
        from - travelled
    };
    Intensity::new(raw as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResolvedTarget;

    fn state_with(fixtures: &[FixtureId]) -> LightingState {
        let mut state = LightingState::new();
        state.define_target(
            TargetId(0),
            ResolvedTarget {
                fixtures: fixtures.to_vec(),
            },
        );
        state
    }

    fn intensity(raw: u16) -> AttributeValue {
        AttributeValue::Intensity(Intensity::new(raw))
    }

    #[test]
    fn rising_and_falling_interpolation_use_absolute_time() {
        for (from, to) in [(0, u16::MAX), (u16::MAX, 0)] {
            let fixture = FixtureId(0);
            let mut state = state_with(&[fixture]);
            state.set_fixture_attribute(fixture, intensity(from));
            let mut engine = TransitionEngine::new();
            engine
                .start_transition(
                    Timestamp::ZERO,
                    TargetId(0),
                    intensity(to),
                    Duration::from_secs(2),
                    &mut state,
                )
                .unwrap();

            for millis in [0, 500, 1000, 1500, 2000] {
                engine
                    .sample(Timestamp::from_millis(millis), &mut state)
                    .unwrap();
                let expected = if from == 0 {
                    ((u16::MAX as u64 * millis + 1000) / 2000) as u16
                } else {
                    u16::MAX - ((u16::MAX as u64 * millis + 1000) / 2000) as u16
                };
                assert_eq!(state.intensity(fixture).raw(), expected);
            }
            assert_eq!(engine.active_count(), 0);
        }
    }

    #[test]
    fn zero_duration_is_an_immediate_set() {
        let fixture = FixtureId(0);
        let mut state = state_with(&[fixture]);
        state.set_fixture_attribute(fixture, intensity(32768));
        let mut engine = TransitionEngine::new();
        engine
            .start_transition(
                Timestamp::ZERO,
                TargetId(0),
                intensity(u16::MAX),
                Duration::ZERO,
                &mut state,
            )
            .unwrap();
        assert_eq!(state.intensity(fixture), Intensity::MAX);
        assert_eq!(engine.active_count(), 0);
    }

    #[test]
    fn replacement_starts_from_the_current_effective_value() {
        let fixture = FixtureId(0);
        let mut state = state_with(&[fixture]);
        let mut engine = TransitionEngine::new();
        engine
            .start_transition(
                Timestamp::ZERO,
                TargetId(0),
                intensity(u16::MAX),
                Duration::from_secs(10),
                &mut state,
            )
            .unwrap();
        engine
            .start_transition(
                Timestamp::from_secs(5),
                TargetId(0),
                intensity(0),
                Duration::from_secs(2),
                &mut state,
            )
            .unwrap();

        for (seconds, expected) in [(5, 32768), (6, 16384), (7, 0)] {
            engine
                .sample(Timestamp::from_secs(seconds), &mut state)
                .unwrap();
            assert_eq!(state.intensity(fixture).raw(), expected);
        }
    }

    #[test]
    fn fixtures_keep_independent_start_values() {
        let a = FixtureId(0);
        let b = FixtureId(1);
        let mut state = state_with(&[a, b]);
        state.set_fixture_attribute(b, intensity(32768));
        let mut engine = TransitionEngine::new();
        engine
            .start_transition(
                Timestamp::ZERO,
                TargetId(0),
                intensity(u16::MAX),
                Duration::from_secs(2),
                &mut state,
            )
            .unwrap();
        engine.sample(Timestamp::from_secs(1), &mut state).unwrap();
        assert_eq!(state.intensity(a).raw(), 32768);
        assert_eq!(state.intensity(b).raw(), 49152);
    }

    #[test]
    fn missed_frames_do_not_change_progress() {
        let fixture = FixtureId(0);
        let mut state = state_with(&[fixture]);
        let mut engine = TransitionEngine::new();
        engine
            .start_transition(
                Timestamp::ZERO,
                TargetId(0),
                intensity(u16::MAX),
                Duration::from_secs(2),
                &mut state,
            )
            .unwrap();
        engine
            .sample(Timestamp::from_millis(1700), &mut state)
            .unwrap();
        assert_eq!(state.intensity(fixture).raw(), 55705);
        engine
            .sample(Timestamp::from_millis(2500), &mut state)
            .unwrap();
        assert_eq!(state.intensity(fixture), Intensity::MAX);
        assert_eq!(engine.active_count(), 0);
    }

    #[test]
    fn interpolation_cannot_overflow_at_u64_time_scale() {
        assert_eq!(
            interpolate_intensity(Intensity::ZERO, Intensity::MAX, u64::MAX / 2, u64::MAX),
            Intensity::new(32767)
        );
    }
}
