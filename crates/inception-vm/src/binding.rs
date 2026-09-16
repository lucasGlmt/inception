//! Active signal bindings: `<target>.<attribute> <- <signal>;`.
//!
//! Mirrors `inception_core::TransitionEngine`'s shape deliberately — same
//! "at most one controller per `(fixture, attribute)`" invariant, same
//! `HashMap`-backed store — but kept in `inception-vm` rather than
//! `inception-core`, because sampling a binding needs a [`SignalStore`]
//! (owned by the executing [`crate::Vm`]) to turn a [`SignalId`] into a
//! [`Value`], and `inception-core` must never depend on this crate (see
//! `AGENTS.md`'s dependency-direction rule). This is the only place that
//! knows both "what a signal is" and "what a `(fixture, attribute)` binding
//! is" — `inception_core::LightingState`/`TransitionEngine` know neither,
//! and `inception_renderer` knows neither either, matching the layering:
//!
//! ```text
//! Signals (SignalStore)
//!     ↓
//! SignalBindingStore (this module)
//!     ↓
//! LightingState (inception-core, signal-agnostic)
//!     ↓
//! Renderer (signal-agnostic)
//! ```
//!
//! Only `SignalStore::sample(id, timestamp)` is ever called — never a
//! `match` on `SignalKind` — so adding a new signal kind (e.g. a future
//! `Sine`) never touches this module (item 77 of the signal-binding task
//! brief).

use std::collections::HashMap;

use inception_core::{Attribute, FixtureId, LightingState, Timestamp};

use crate::signal::{SignalError, SignalId, SignalStore};
use crate::value::Value;

/// One fixture's active signal binding, as handed out by
/// [`SignalBindingStore::iter`]. Not how bindings are stored internally
/// (see [`SignalBindingStore`]'s docs) — just a read-only view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveSignalBinding {
    pub fixture: FixtureId,
    pub attribute: Attribute,
    pub signal: SignalId,
}

/// At most one signal binding exists for a `(fixture, attribute)` key —
/// the same invariant `TransitionEngine` enforces for transitions, and for
/// the same V1 reason (item 26/43 of the task brief: one active controller
/// per fixture/attribute, no simultaneous transition + signal binding).
///
/// Backed by a `HashMap`, same as `TransitionEngine::active` and
/// `LightingState`'s fields: every write is independent per key (there is
/// no interaction between two bindings), so iteration order never affects
/// the result — determinism (item 42) doesn't require an ordered map here,
/// only that each key's own sampling is a pure function of
/// `(SignalId, Timestamp)`, which [`SignalStore::sample`] already
/// guarantees.
#[derive(Debug, Default)]
pub struct SignalBindingStore {
    active: HashMap<(FixtureId, Attribute), SignalId>,
}

impl SignalBindingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn get(&self, fixture: FixtureId, attribute: Attribute) -> Option<SignalId> {
        self.active.get(&(fixture, attribute)).copied()
    }

    /// Installs `signal` as the controller for `(fixture, attribute)`,
    /// replacing whatever was bound there before (item 26: "new binding
    /// replaces old binding"). Never allocates beyond the `HashMap`'s own
    /// insert.
    pub fn bind(&mut self, fixture: FixtureId, attribute: Attribute, signal: SignalId) {
        self.active.insert((fixture, attribute), signal);
    }

    /// Removes and returns the binding active on `(fixture, attribute)`,
    /// if any — used by `=` and `->` to detach a signal before taking over
    /// the attribute themselves (items 27/28).
    pub fn unbind(&mut self, fixture: FixtureId, attribute: Attribute) -> Option<SignalId> {
        self.active.remove(&(fixture, attribute))
    }

    pub fn iter(&self) -> impl Iterator<Item = ActiveSignalBinding> + '_ {
        self.active
            .iter()
            .map(|(&(fixture, attribute), &signal)| ActiveSignalBinding {
                fixture,
                attribute,
                signal,
            })
    }

    /// Samples every active binding at `now` and writes the result
    /// straight into `state` — the one place a [`Value`] sampled from
    /// `signals` is converted into an `inception_core::AttributeValue` and
    /// applied. Never touches DMX or the renderer (see this module's
    /// doc). Called once per render frame by the runtime loop, exactly
    /// like `TransitionEngine::sample` — never per-tick inside the VM
    /// itself (item 20: `BIND_SIGNAL` only registers a binding, it never
    /// samples repeatedly).
    pub fn sample(
        &self,
        signals: &SignalStore,
        now: Timestamp,
        state: &mut LightingState,
    ) -> Result<(), SignalError> {
        for (&(fixture, attribute), &signal) in &self.active {
            let value = signals.sample(signal, now)?;
            if let Some(attribute_value) = to_attribute_value(attribute, value) {
                state.set_fixture_attribute(fixture, attribute_value);
            }
        }
        Ok(())
    }
}

/// Converts a sampled signal [`Value`] into an `inception_core`
/// `AttributeValue` for `attribute` — `None` if the value's variant
/// doesn't match, mirroring `Value::into_attribute_value`'s defensive
/// style (this can't actually happen for a verified module, since
/// `BIND_SIGNAL`'s operand type is checked by `lux_bytecode::verify`, but
/// `SignalBindingStore` stays as defensive as the rest of this crate about
/// values that didn't come from trusted bytecode).
fn to_attribute_value(
    attribute: Attribute,
    value: Value,
) -> Option<inception_core::AttributeValue> {
    let bytecode_attribute = match attribute {
        Attribute::Intensity => lux_bytecode::Attribute::Intensity,
        Attribute::Color => lux_bytecode::Attribute::Color,
    };
    value.into_attribute_value(bytecode_attribute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::SignalKind;
    use inception_core::{Intensity, ResolvedTarget, TargetId};

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

    #[test]
    fn binding_replaces_the_previous_one_on_the_same_key() {
        let fixture = FixtureId(0);
        let mut store = SignalBindingStore::new();
        store.bind(fixture, Attribute::Intensity, SignalId(0));
        store.bind(fixture, Attribute::Intensity, SignalId(1));
        assert_eq!(store.get(fixture, Attribute::Intensity), Some(SignalId(1)));
        assert_eq!(store.active_count(), 1);
    }

    #[test]
    fn sampling_writes_every_active_binding_into_lighting_state() {
        let fixture = FixtureId(0);
        let mut state = state_with(&[fixture]);
        let mut signals = SignalStore::new();
        let id = signals.insert(SignalKind::Constant(Value::Intensity(32768)));

        let mut store = SignalBindingStore::new();
        store.bind(fixture, Attribute::Intensity, id);
        store.sample(&signals, Timestamp::ZERO, &mut state).unwrap();

        assert_eq!(state.intensity(fixture), Intensity::new(32768));
    }

    #[test]
    fn unbind_removes_and_returns_the_signal() {
        let fixture = FixtureId(0);
        let mut store = SignalBindingStore::new();
        store.bind(fixture, Attribute::Intensity, SignalId(3));
        assert_eq!(
            store.unbind(fixture, Attribute::Intensity),
            Some(SignalId(3))
        );
        assert_eq!(store.unbind(fixture, Attribute::Intensity), None);
        assert_eq!(store.active_count(), 0);
    }

    #[test]
    fn sampling_is_stable_across_repeated_calls_at_different_timestamps() {
        let fixture = FixtureId(0);
        let mut state = state_with(&[fixture]);
        let mut signals = SignalStore::new();
        let id = signals.insert(SignalKind::Constant(Value::Intensity(20000)));
        let mut store = SignalBindingStore::new();
        store.bind(fixture, Attribute::Intensity, id);

        for at in [
            Timestamp::ZERO,
            Timestamp::from_millis(25),
            Timestamp::from_secs(1),
            Timestamp::from_secs(10),
            Timestamp::from_secs(3600),
        ] {
            store.sample(&signals, at, &mut state).unwrap();
            assert_eq!(state.intensity(fixture), Intensity::new(20000));
        }
    }
}
