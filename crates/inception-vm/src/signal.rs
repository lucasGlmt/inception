//! Runtime signal storage and sampling.
//!
//! A [`Signal<T>`][crate::value::Value::Signal] value doesn't carry its
//! definition inline — it's a [`SignalId`], a handle into a [`SignalStore`]
//! owned by the executing [`crate::Vm`]. This indirection is deliberate
//! even though V1 has only one [`SignalKind`] (`Constant`): a future
//! composed signal (`Map`, a node graph, ...) needs to refer to *other*
//! signals, which an inline `Value` payload could never represent without
//! an unbounded/recursive `Value` size. Starting with a store now means
//! that extension doesn't require reworking how `Value::Signal` itself
//! works, only adding new `SignalKind` variants.
//!
//! Signal definitions are immutable once inserted — sampling never
//! mutates a [`SignalDefinition`], matching the task brief's "signal
//! definitions are immutable in V1" recommendation. That, plus every
//! `SignalKind` being pure, is what makes [`SignalStore::sample`]
//! trivially deterministic: the same `(id, timestamp)` always reads the
//! same immutable definition and computes the same result.
//!
//! Ownership: a [`SignalStore`] lives exactly as long as the [`crate::Vm`]
//! that owns it. Reloading a program means building a new `Vm` (and so a
//! new, empty `SignalStore`) for the newly compiled module — there is no
//! migration of old `SignalId`s into a new store, matching how the rest
//! of a reloaded program's state already isn't preserved across a
//! `LoadedProgram` swap.

use inception_core::Timestamp;

use crate::value::Value;

/// A handle into a [`SignalStore`]. Never constructed from a name or
/// string — only ever handed out by [`SignalStore::insert`] — so the VM
/// never resolves a signal by anything other than this numeric id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub u32);

/// What a signal computes. `Constant` is the only variant in V1: it
/// ignores the timestamp entirely and always returns the same wrapped
/// value. Future kinds (`Sine`, `Map` over another `SignalId`, ...) are
/// added here without changing [`SignalId`] or [`Value::Signal`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalKind {
    /// Always samples to `value`, for any timestamp. `value` is always a
    /// non-`Signal` scalar `Value` — enforced by construction, since the
    /// only place this variant is ever built is `Vm::exec_call_intrinsic`
    /// handling a `SignalConstant*` intrinsic, whose one popped operand is
    /// guaranteed scalar-typed by `lux_bytecode::verify`.
    Constant(Value),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalDefinition {
    pub kind: SignalKind,
}

/// A signal doesn't know how to resolve its own `SignalId` failures into
/// anything but this — [`SignalStore::sample`] never panics, even for a
/// hand-corrupted `SignalId` (e.g. from artificially malformed bytecode
/// state), matching this workspace's "never trust that bytecode came from
/// the real compiler" rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalError {
    UnknownSignal(SignalId),
}

/// An append-only arena of signal definitions, indexed by [`SignalId`].
/// Deliberately not deduplicated — two `Signal.constant(50%)` calls
/// produce two distinct `SignalId`s pointing at two `SignalDefinition`s
/// with equal content, which is fine: identity was never a guarantee V1
/// makes.
#[derive(Debug, Default)]
pub struct SignalStore {
    definitions: Vec<SignalDefinition>,
}

impl SignalStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new signal definition, returning the id it can be
    /// sampled through from now on.
    pub fn insert(&mut self, kind: SignalKind) -> SignalId {
        let id = SignalId(self.definitions.len() as u32);
        self.definitions.push(SignalDefinition { kind });
        id
    }

    /// Evaluates the signal `id` at `at`. Takes the timestamp explicitly
    /// rather than reading a clock itself — see this module's doc comment
    /// and the task brief's "a signal doesn't own the clock" rule — so
    /// this is exactly as deterministic and testable as the caller's own
    /// timestamp values are, with no dependency on real or virtual time
    /// beyond what's passed in. `at` need not be monotonically increasing
    /// across calls: nothing here assumes or requires it.
    pub fn sample(&self, id: SignalId, at: Timestamp) -> Result<Value, SignalError> {
        let _ = at;
        let definition = self
            .definitions
            .get(id.0 as usize)
            .ok_or(SignalError::UnknownSignal(id))?;
        match definition.kind {
            SignalKind::Constant(value) => Ok(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lux_bytecode::ColorValue;

    #[test]
    fn constant_signal_samples_the_same_value_at_any_timestamp() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Constant(Value::Intensity(32767)));

        for at in [
            Timestamp::ZERO,
            Timestamp::from_millis(1),
            Timestamp::from_secs(1),
            Timestamp::from_secs(3600),
        ] {
            assert_eq!(store.sample(id, at), Ok(Value::Intensity(32767)));
        }
    }

    #[test]
    fn constant_color_signal_samples_the_same_value_at_any_timestamp() {
        let mut store = SignalStore::new();
        let red = ColorValue { r: 255, g: 0, b: 0 };
        let id = store.insert(SignalKind::Constant(Value::Color(red)));

        for at in [Timestamp::ZERO, Timestamp::from_secs(1)] {
            assert_eq!(store.sample(id, at), Ok(Value::Color(red)));
        }
    }

    #[test]
    fn repeated_sampling_is_identical() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Constant(Value::Float(1.5)));
        let at = Timestamp::from_millis(1234);

        let first = store.sample(id, at);
        for _ in 0..100 {
            assert_eq!(store.sample(id, at), first);
        }
    }

    #[test]
    fn sampling_does_not_assume_monotonic_timestamps() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Constant(Value::Int(42)));

        for at in [
            Timestamp::from_secs(10),
            Timestamp::from_secs(2),
            Timestamp::from_secs(50),
            Timestamp::ZERO,
        ] {
            assert_eq!(store.sample(id, at), Ok(Value::Int(42)));
        }
    }

    #[test]
    fn unknown_signal_id_is_a_structured_error_not_a_panic() {
        let store = SignalStore::new();
        assert_eq!(
            store.sample(SignalId(0), Timestamp::ZERO),
            Err(SignalError::UnknownSignal(SignalId(0)))
        );
    }

    #[test]
    fn insert_returns_distinct_ids_even_for_equal_definitions() {
        let mut store = SignalStore::new();
        let a = store.insert(SignalKind::Constant(Value::Intensity(100)));
        let b = store.insert(SignalKind::Constant(Value::Intensity(100)));
        assert_ne!(a, b);
    }
}
