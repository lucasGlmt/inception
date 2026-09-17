//! Runtime sequence storage.
//!
//! A [`Sequence<T>`][crate::value::Value::Sequence] value doesn't carry its
//! elements inline — it's a [`SequenceId`], a handle into a
//! [`SequenceStore`] owned by the executing [`crate::Vm`], mirroring
//! [`crate::signal::SignalId`]/[`crate::signal::SignalStore`]'s shape
//! exactly (see that module's docs for the rationale: keeping `Value`
//! itself small and `Copy`, and leaving room for a future feature to add
//! more without reworking `Value::Sequence`).
//!
//! Unlike a signal, a sequence has no "sampling" step — it's already a
//! plain, immutable, ordered list of already-computed [`Value`]s by the
//! time it's constructed (`Sequence.of`'s arguments are evaluated once,
//! left to right, before the sequence is built). So [`SequenceStore`]
//! only needs to support insertion and read access (`length`/`get`),
//! never anything resembling `SignalStore::sample`'s per-timestamp
//! evaluation.

use crate::value::Value;

/// A handle into a [`SequenceStore`]. Never constructed from a name or
/// string — only ever handed out by [`SequenceStore::insert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SequenceId(pub u32);

/// A sequence's error path for the two runtime operations that can fail
/// against a possibly-hand-corrupted `SequenceId`/index — see
/// `crate::signal::SignalError`'s docs for why every runtime lookup here
/// is `Result`-shaped rather than a panic, matching this workspace's
/// "never trust that bytecode came from the real compiler" rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceError {
    UnknownSequence(SequenceId),
    /// `Instruction::Index`'s index operand was outside `0..length`.
    /// Never reachable from real Lux source when the length is a compile-
    /// time-known literal count (`lux-typeck` does not currently attempt
    /// that check — see item 13 of the task brief — so this *is*
    /// reachable today for any non-trivially-constant index), but always
    /// reported as a structured error rather than a panic either way.
    IndexOutOfBounds {
        id: SequenceId,
        index: i64,
        length: usize,
    },
}

/// An append-only arena of sequence definitions, indexed by [`SequenceId`].
/// Deliberately not deduplicated, matching [`crate::signal::SignalStore`]'s
/// same choice: two structurally-equal `Sequence.of(...)` calls produce
/// two distinct ids, which is fine — identity was never a guarantee V1
/// makes for either type.
#[derive(Debug, Default)]
pub struct SequenceStore {
    definitions: Vec<Vec<Value>>,
}

impl SequenceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new sequence, returning the id it can be read through
    /// from now on. `elements` must be non-empty and every element must
    /// share the same `Value` variant — both guaranteed by `lux-typeck`
    /// for real Lux source (`Sequence.of()` and mixed-type calls are
    /// compile-time errors), but not re-checked here: like
    /// `SignalStore::insert`, this trusts its caller (`Vm`), which itself
    /// only ever calls this after `lux_bytecode::verify` already
    /// confirmed the popped operands' types.
    pub fn insert(&mut self, elements: Vec<Value>) -> SequenceId {
        let id = SequenceId(self.definitions.len() as u32);
        self.definitions.push(elements);
        id
    }

    /// The number of elements in sequence `id`, or `None` if it doesn't
    /// exist.
    pub fn length(&self, id: SequenceId) -> Option<usize> {
        self.definitions.get(id.0 as usize).map(Vec::len)
    }

    /// The element at `index` in sequence `id`. A negative or
    /// out-of-range `index` is a structured [`SequenceError::IndexOutOfBounds`],
    /// never a panic — see that variant's docs.
    pub fn get(&self, id: SequenceId, index: i64) -> Result<Value, SequenceError> {
        let elements = self
            .definitions
            .get(id.0 as usize)
            .ok_or(SequenceError::UnknownSequence(id))?;
        usize::try_from(index)
            .ok()
            .and_then(|i| elements.get(i))
            .copied()
            .ok_or(SequenceError::IndexOutOfBounds {
                id,
                index,
                length: elements.len(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_length_and_get_round_trip() {
        let mut store = SequenceStore::new();
        let id = store.insert(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(store.length(id), Some(3));
        assert_eq!(store.get(id, 0), Ok(Value::Int(1)));
        assert_eq!(store.get(id, 1), Ok(Value::Int(2)));
        assert_eq!(store.get(id, 2), Ok(Value::Int(3)));
    }

    #[test]
    fn out_of_bounds_index_is_a_structured_error_not_a_panic() {
        let mut store = SequenceStore::new();
        let id = store.insert(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(
            store.get(id, 2),
            Err(SequenceError::IndexOutOfBounds {
                id,
                index: 2,
                length: 2
            })
        );
    }

    #[test]
    fn negative_index_is_a_structured_error_not_a_panic() {
        let mut store = SequenceStore::new();
        let id = store.insert(vec![Value::Int(1)]);
        assert_eq!(
            store.get(id, -1),
            Err(SequenceError::IndexOutOfBounds {
                id,
                index: -1,
                length: 1
            })
        );
    }

    #[test]
    fn unknown_sequence_id_is_a_structured_error_not_a_panic() {
        let store = SequenceStore::new();
        assert_eq!(
            store.get(SequenceId(0), 0),
            Err(SequenceError::UnknownSequence(SequenceId(0)))
        );
        assert_eq!(store.length(SequenceId(0)), None);
    }

    #[test]
    fn insert_returns_distinct_ids_even_for_equal_definitions() {
        let mut store = SequenceStore::new();
        let a = store.insert(vec![Value::Int(1)]);
        let b = store.insert(vec![Value::Int(1)]);
        assert_ne!(a, b);
    }
}
