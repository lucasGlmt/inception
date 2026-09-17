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
//! Only `SignalStore::sample(id, timestamp, sequences)` is ever called — never a
//! `match` on `SignalKind` — so adding a new signal kind (e.g. a future
//! `Sine`) never touches this module (item 77 of the signal-binding task
//! brief).

use std::collections::HashMap;

use inception_core::{Attribute, FixtureId, LightingState, Timestamp};

use crate::sequence::SequenceStore;
use crate::signal::{SignalError, SignalId, SignalSampleContext, SignalStore};
use crate::value::Value;

/// One fixture's active signal binding, as handed out by
/// [`SignalBindingStore::iter`]. Not how bindings are stored internally
/// (see [`SignalBindingStore`]'s docs) — just a read-only view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveSignalBinding {
    pub fixture: FixtureId,
    pub attribute: Attribute,
    pub signal: SignalId,
    /// This fixture's position within the target group it was bound
    /// through, and that group's total fixture count — see
    /// [`BoundSignal`]'s docs for why both are recorded per fixture
    /// rather than derived later.
    pub fixture_index: usize,
    pub fixture_count: usize,
}

/// One `(fixture, attribute)` binding's full stored state: not just
/// *which* signal controls it, but *where* — within the target group
/// `<-` bound it through — this particular fixture sits. `.spread()`
/// (see `crate::signal::SignalKind::Spread`) is the reason this exists:
/// the same `SignalId` is bound to every fixture in a group (never one
/// signal per fixture — item 8 of the spread task brief), so telling two
/// fixtures apart at sample time needs this pair carried alongside the
/// id, not derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoundSignal {
    signal: SignalId,
    fixture_index: usize,
    fixture_count: usize,
}

/// At most one signal binding exists for a `(fixture, attribute)` key —
/// the same invariant `TransitionEngine` enforces for transitions, and for
/// the same V1 reason (item 26/43 of the task brief: one active controller
/// per fixture/attribute, no simultaneous transition + signal binding).
///
/// Backed by a `HashMap`, same as `TransitionEngine::active` and
/// `LightingState`'s fields: every write is independent per key (there is
/// no interaction between two bindings), so iteration order never affects
/// the result — determinism (item 42, and item 4 of the spread task
/// brief) doesn't require an ordered map here, only that each key's own
/// sampling is a pure function of `(SignalId, SignalSampleContext)`, which
/// [`SignalStore::sample`] already guarantees, and that `fixture_index`/
/// `fixture_count` themselves came from a deterministic source in the
/// first place — see `Vm::exec_bind_signal`, which derives them from the
/// target's already rig-ordered `Vec<FixtureId>`, never from this map's
/// own iteration.
#[derive(Debug, Default)]
pub struct SignalBindingStore {
    active: HashMap<(FixtureId, Attribute), BoundSignal>,
}

impl SignalBindingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn get(&self, fixture: FixtureId, attribute: Attribute) -> Option<SignalId> {
        self.active.get(&(fixture, attribute)).map(|b| b.signal)
    }

    /// Installs `signal` as the controller for `(fixture, attribute)`,
    /// replacing whatever was bound there before (item 26: "new binding
    /// replaces old binding"). `fixture_index`/`fixture_count` describe
    /// `fixture`'s position within the target group this binding came
    /// from (see [`BoundSignal`]'s docs) — the caller (`Vm::exec_bind_signal`)
    /// passes the same `signal` for every fixture in the group, varying
    /// only `fixture_index`. Never allocates beyond the `HashMap`'s own
    /// insert.
    pub fn bind(
        &mut self,
        fixture: FixtureId,
        attribute: Attribute,
        signal: SignalId,
        fixture_index: usize,
        fixture_count: usize,
    ) {
        self.active.insert(
            (fixture, attribute),
            BoundSignal {
                signal,
                fixture_index,
                fixture_count,
            },
        );
    }

    /// Removes and returns the binding active on `(fixture, attribute)`,
    /// if any — used by `=` and `->` to detach a signal before taking over
    /// the attribute themselves (items 27/28). Returns the fixture's own
    /// `fixture_index`/`fixture_count` alongside the `SignalId` so a
    /// caller resampling the signal one last time (see
    /// `Vm::exec_transition_attribute`) can build the exact same
    /// [`SignalSampleContext`] the binding itself was sampled with.
    pub fn unbind(
        &mut self,
        fixture: FixtureId,
        attribute: Attribute,
    ) -> Option<(SignalId, usize, usize)> {
        self.active
            .remove(&(fixture, attribute))
            .map(|b| (b.signal, b.fixture_index, b.fixture_count))
    }

    pub fn iter(&self) -> impl Iterator<Item = ActiveSignalBinding> + '_ {
        self.active
            .iter()
            .map(|(&(fixture, attribute), &bound)| ActiveSignalBinding {
                fixture,
                attribute,
                signal: bound.signal,
                fixture_index: bound.fixture_index,
                fixture_count: bound.fixture_count,
            })
    }

    /// Samples every active binding at `now` and writes the result
    /// straight into `state` — the one place a [`Value`] sampled from
    /// `signals` is converted into an `inception_core::AttributeValue` and
    /// applied. Never touches DMX or the renderer (see this module's
    /// doc). Called once per render frame by the runtime loop, exactly
    /// like `TransitionEngine::sample` — never per-tick inside the VM
    /// itself (item 20: `BIND_SIGNAL` only registers a binding, it never
    /// samples repeatedly). Each binding is sampled with *its own*
    /// `fixture_index`/`fixture_count` (item 8 of the spread task brief):
    /// two fixtures sharing the same bound `SignalId` still get distinct
    /// `SignalSampleContext`s, which is exactly what lets a `Spread` node
    /// render a different phase per fixture despite being one signal
    /// definition sampled multiple times.
    pub fn sample(
        &self,
        signals: &SignalStore,
        sequences: &SequenceStore,
        now: Timestamp,
        state: &mut LightingState,
    ) -> Result<(), SignalError> {
        for (&(fixture, attribute), &bound) in &self.active {
            let context = SignalSampleContext::new(now, bound.fixture_index, bound.fixture_count);
            let value = signals.sample(bound.signal, context, sequences)?;
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
        Attribute::Strobe => lux_bytecode::Attribute::Strobe,
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
        store.bind(fixture, Attribute::Intensity, SignalId(0), 0, 1);
        store.bind(fixture, Attribute::Intensity, SignalId(1), 0, 1);
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
        store.bind(fixture, Attribute::Intensity, id, 0, 1);
        store
            .sample(&signals, &SequenceStore::new(), Timestamp::ZERO, &mut state)
            .unwrap();

        assert_eq!(state.intensity(fixture), Intensity::new(32768));
    }

    #[test]
    fn unbind_removes_and_returns_the_signal_and_its_fixture_context() {
        let fixture = FixtureId(0);
        let mut store = SignalBindingStore::new();
        store.bind(fixture, Attribute::Intensity, SignalId(3), 2, 4);
        assert_eq!(
            store.unbind(fixture, Attribute::Intensity),
            Some((SignalId(3), 2, 4))
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
        store.bind(fixture, Attribute::Intensity, id, 0, 1);

        for at in [
            Timestamp::ZERO,
            Timestamp::from_millis(25),
            Timestamp::from_secs(1),
            Timestamp::from_secs(10),
            Timestamp::from_secs(3600),
        ] {
            store
                .sample(&signals, &SequenceStore::new(), at, &mut state)
                .unwrap();
            assert_eq!(state.intensity(fixture), Intensity::new(20000));
        }
    }

    /// Item 8: two fixtures sharing the same bound `SignalId` still get
    /// distinct `fixture_index`/`fixture_count` at sample time — the
    /// mechanism `.spread()` (see `crate::signal::SignalKind::Spread`)
    /// relies on, tested here directly against `SignalBindingStore`
    /// rather than through a whole `Vm`. Uses a saw wave (`value ==
    /// phase`, no trig involved) so the two fixtures' expected values are
    /// plain, distinguishable numbers rather than two phases that happen
    /// to land on the same amplitude.
    #[test]
    fn each_fixture_samples_the_shared_signal_with_its_own_context() {
        let fixture_a = FixtureId(0);
        let fixture_b = FixtureId(1);
        let mut state = state_with(&[fixture_a, fixture_b]);
        let mut signals = SignalStore::new();
        let saw = signals.insert(SignalKind::Saw {
            period: inception_core::Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        // 180deg spread over 2 fixtures: offsets 0deg and 90deg.
        let spread = signals.insert(SignalKind::Spread {
            source: saw,
            amount_millideg: 180_000,
        });
        // `.range(0%, 100%)`: a raw `Signal<Float>` never converts to an
        // `Intensity` attribute value (see `Value::into_attribute_value`)
        // — real Lux source hits the same rule via `lux-typeck`.
        let ranged = signals.insert(SignalKind::Range {
            source: spread,
            min: Value::Intensity(0),
            max: Value::Intensity(u16::MAX),
        });

        let mut store = SignalBindingStore::new();
        store.bind(fixture_a, Attribute::Intensity, ranged, 0, 2);
        store.bind(fixture_b, Attribute::Intensity, ranged, 1, 2);
        store
            .sample(&signals, &SequenceStore::new(), Timestamp::ZERO, &mut state)
            .unwrap();

        // fixture 0: offset 0deg -> saw phase 0.0 -> value 0.0 -> 0%.
        // fixture 1: offset 90deg -> saw phase 0.25 -> value 0.25 -> 25%.
        assert_eq!(state.intensity(fixture_a), Intensity::new(0));
        assert_eq!(state.intensity(fixture_b), Intensity::new(16384));
    }
}
