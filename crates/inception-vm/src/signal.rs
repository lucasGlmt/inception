//! Runtime signal storage and sampling.
//!
//! A [`Signal<T>`][crate::value::Value::Signal] value doesn't carry its
//! definition inline — it's a [`SignalId`], a handle into a [`SignalStore`]
//! owned by the executing [`crate::Vm`]. This indirection is deliberate
//! even with today's flat [`SignalKind`] set (`Constant` plus the four
//! `std.Effects` oscillators): a future composed signal (`Map`, a node
//! graph, ...) needs to refer to *other* signals, which an inline `Value`
//! payload could never represent without an unbounded/recursive `Value`
//! size. Starting with a store now means that extension doesn't require
//! reworking how `Value::Signal` itself works, only adding new
//! `SignalKind` variants.
//!
//! Signal definitions are immutable once inserted — sampling never
//! mutates a [`SignalDefinition`], matching the task brief's "signal
//! definitions are immutable in V1" recommendation. Every `SignalKind` is
//! a pure function of `(kind, timestamp)` — `Constant` ignores the
//! timestamp entirely, and the four oscillators derive their value solely
//! from `(period, started_at, timestamp)`, never from any accumulated or
//! externally mutated state — which is what makes [`SignalStore::sample`]
//! trivially deterministic: the same `(id, timestamp)` always reads the
//! same immutable definition and computes the same result. `started_at`
//! is the one place an oscillator's *construction* legitimately depends
//! on the runtime clock (see `Vm::exec_call_intrinsic`'s `Effects*`
//! handling) — but sampling itself never reads a clock, only the
//! `at: Timestamp` passed in.
//!
//! Ownership: a [`SignalStore`] lives exactly as long as the [`crate::Vm`]
//! that owns it. Reloading a program means building a new `Vm` (and so a
//! new, empty `SignalStore`) for the newly compiled module — there is no
//! migration of old `SignalId`s into a new store, matching how the rest
//! of a reloaded program's state already isn't preserved across a
//! `LoadedProgram` swap.

use inception_core::{Duration, Timestamp};

use crate::value::Value;

/// A handle into a [`SignalStore`]. Never constructed from a name or
/// string — only ever handed out by [`SignalStore::insert`] — so the VM
/// never resolves a signal by anything other than this numeric id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub u32);

/// What a signal computes. `Constant` was the only variant before
/// `std.Effects`; the four oscillator kinds below are its first
/// time-varying siblings. Future kinds (`Map` over another `SignalId`,
/// composition, ...) are added here without changing [`SignalId`] or
/// [`Value::Signal`] — and, per `crate::binding`'s module doc, without
/// touching `SignalBindingStore` at all, since that code only ever calls
/// [`SignalStore::sample`], never matches on `SignalKind` itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalKind {
    /// Always samples to `value`, for any timestamp. `value` is always a
    /// non-`Signal` scalar `Value` — enforced by construction, since the
    /// only place this variant is ever built is `Vm::exec_call_intrinsic`
    /// handling a `SignalConstant*` intrinsic, whose one popped operand is
    /// guaranteed scalar-typed by `lux_bytecode::verify`.
    Constant(Value),
    /// A sine-wave oscillator: `0.5 + 0.5 * sin(2π * phase)`, so
    /// `phase 0.00 -> 0.5, 0.25 -> 1.0, 0.50 -> 0.5, 0.75 -> 0.0` — see
    /// `phase_at`'s docs for how `phase` itself is derived from `period`
    /// and `started_at`. Always samples to a `Value::Float` in `0.0..=1.0`.
    Sine {
        period: Duration,
        started_at: Timestamp,
    },
    /// A triangle-wave oscillator: `1 - |2 * phase - 1|`, so
    /// `phase 0.00 -> 0.0, 0.25 -> 0.5, 0.50 -> 1.0, 0.75 -> 0.5`.
    Triangle {
        period: Duration,
        started_at: Timestamp,
    },
    /// A rising sawtooth oscillator: `value = phase` directly, so
    /// `phase 0.00 -> 0.0` ramping linearly up to just under `1.0` before
    /// snapping back to `0.0` at the start of the next period. There is no
    /// falling/descending variant in V1.
    Saw {
        period: Duration,
        started_at: Timestamp,
    },
    /// A 50%-duty-cycle square wave: `1.0` for `phase < 0.5`, `0.0`
    /// otherwise.
    Square {
        period: Duration,
        started_at: Timestamp,
    },
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
        let definition = self
            .definitions
            .get(id.0 as usize)
            .ok_or(SignalError::UnknownSignal(id))?;
        Ok(match definition.kind {
            SignalKind::Constant(value) => value,
            SignalKind::Sine { period, started_at } => {
                Value::Float(sine_wave(phase_at(at, started_at, period)))
            }
            SignalKind::Triangle { period, started_at } => {
                Value::Float(triangle_wave(phase_at(at, started_at, period)))
            }
            SignalKind::Saw { period, started_at } => {
                Value::Float(saw_wave(phase_at(at, started_at, period)))
            }
            SignalKind::Square { period, started_at } => {
                Value::Float(square_wave(phase_at(at, started_at, period)))
            }
        })
    }
}

/// The normalized phase, in `[0, 1)`, of an oscillator with the given
/// `period` and `started_at` origin, sampled `at` some timestamp.
///
/// `elapsed` is computed with `saturating_sub`, not plain subtraction: a
/// sample taken *before* `started_at` (only reachable by sampling a
/// `SignalId` directly rather than through the normal construct-then-
/// sample order) clamps to `elapsed = 0` rather than underflowing — see
/// the task brief's "timestamps before `started_at`" policy.
///
/// `remainder = elapsed % period` is exact `u64` arithmetic — the only
/// floating-point step is the final division that turns the remainder
/// into a `0.0..1.0` fraction. This is what keeps the phase stable over
/// arbitrarily long durations (item 21 of the task brief): the result is
/// always recomputed from `elapsed`/`period` directly, never accumulated
/// sample-over-sample.
///
/// # Panics
///
/// Only if `period` is zero. Callers must reject a zero period before
/// ever constructing a `SignalKind` oscillator variant (see
/// `Vm::exec_call_intrinsic`'s `EffectsSine`/... handling, which returns
/// a structured `VmErrorKind::InvalidSignalPeriod` instead of ever
/// reaching this function with one) — so this is an internal invariant,
/// not a condition sampling code needs to handle.
fn phase_at(at: Timestamp, started_at: Timestamp, period: Duration) -> f64 {
    let period_nanos = period.as_nanos();
    debug_assert!(period_nanos > 0, "oscillator period must be non-zero");
    let elapsed = at.as_nanos().saturating_sub(started_at.as_nanos());
    let remainder = elapsed % period_nanos;
    remainder as f64 / period_nanos as f64
}

fn sine_wave(phase: f64) -> f64 {
    (0.5 + 0.5 * (std::f64::consts::TAU * phase).sin()).clamp(0.0, 1.0)
}

fn triangle_wave(phase: f64) -> f64 {
    (1.0 - (2.0 * phase - 1.0).abs()).clamp(0.0, 1.0)
}

fn saw_wave(phase: f64) -> f64 {
    phase.clamp(0.0, 1.0)
}

fn square_wave(phase: f64) -> f64 {
    if phase < 0.5 { 1.0 } else { 0.0 }
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

    const TOLERANCE: f64 = 1e-9;

    fn assert_float_close(actual: Value, expected: f64) {
        match actual {
            Value::Float(v) => assert!(
                (v - expected).abs() < TOLERANCE,
                "expected {expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Item 52: sine at the five canonical phases of a 2s period, origin
    /// at `t=0`.
    #[test]
    fn sine_matches_the_documented_phase_table() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Sine {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        for (millis, expected) in [(0, 0.5), (500, 1.0), (1000, 0.5), (1500, 0.0), (2000, 0.5)] {
            assert_float_close(
                store.sample(id, Timestamp::from_millis(millis)).unwrap(),
                expected,
            );
        }
    }

    /// Item 53.
    #[test]
    fn triangle_matches_the_documented_phase_table() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Triangle {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        for (millis, expected) in [(0, 0.0), (500, 0.5), (1000, 1.0), (1500, 0.5), (2000, 0.0)] {
            assert_float_close(
                store.sample(id, Timestamp::from_millis(millis)).unwrap(),
                expected,
            );
        }
    }

    /// Item 54.
    #[test]
    fn saw_matches_the_documented_phase_table() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Saw {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        for (millis, expected) in [
            (0, 0.0),
            (500, 0.25),
            (1000, 0.5),
            (1500, 0.75),
            (2000, 0.0),
        ] {
            assert_float_close(
                store.sample(id, Timestamp::from_millis(millis)).unwrap(),
                expected,
            );
        }
    }

    /// Item 55: first half of the period is `1.0`, second half `0.0`,
    /// with the boundary landing on the second half (`phase < 0.5`, not
    /// `<=`).
    #[test]
    fn square_matches_the_documented_phase_table() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Square {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        for (millis, expected) in [
            (0, 1.0),
            (500, 1.0),
            (999, 1.0),
            (1000, 0.0),
            (1500, 0.0),
            (2000, 1.0),
        ] {
            assert_float_close(
                store.sample(id, Timestamp::from_millis(millis)).unwrap(),
                expected,
            );
        }
    }

    /// Item 56: a short period doesn't implicitly assume second-scale
    /// granularity.
    #[test]
    fn short_period_oscillates_correctly() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Saw {
            period: Duration::from_millis(100),
            started_at: Timestamp::ZERO,
        });
        assert_float_close(store.sample(id, Timestamp::from_millis(25)).unwrap(), 0.25);
        assert_float_close(store.sample(id, Timestamp::from_millis(150)).unwrap(), 0.5);
    }

    /// Item 57: a long (1h) period, sampled at several positions.
    #[test]
    fn long_period_oscillates_correctly() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Saw {
            period: Duration::from_secs(3600),
            started_at: Timestamp::ZERO,
        });
        assert_float_close(store.sample(id, Timestamp::from_secs(900)).unwrap(), 0.25);
        assert_float_close(store.sample(id, Timestamp::from_secs(1800)).unwrap(), 0.5);
        assert_float_close(store.sample(id, Timestamp::from_secs(3600)).unwrap(), 0.0);
    }

    /// Item 58: a non-zero `started_at` shifts the cycle's origin, not
    /// just the sampled values — `phase` is computed relative to
    /// `started_at`, never to absolute zero.
    #[test]
    fn nonzero_started_at_shifts_the_cycle_origin() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Sine {
            period: Duration::from_secs(2),
            started_at: Timestamp::from_secs(5),
        });
        assert_float_close(store.sample(id, Timestamp::from_millis(5000)).unwrap(), 0.5);
        assert_float_close(store.sample(id, Timestamp::from_millis(5500)).unwrap(), 1.0);
        assert_float_close(store.sample(id, Timestamp::from_millis(6000)).unwrap(), 0.5);
    }

    /// Item 59: only ever sampling `0ms`, `1750ms`, `5250ms` (never the
    /// in-between frames) must still produce the mathematically correct
    /// phase at each of those — no dependency on any previous sample.
    #[test]
    fn skipped_frames_still_produce_the_correct_phase() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Saw {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        assert_float_close(store.sample(id, Timestamp::from_millis(0)).unwrap(), 0.0);
        assert_float_close(
            store.sample(id, Timestamp::from_millis(1750)).unwrap(),
            0.875,
        );
        assert_float_close(
            store.sample(id, Timestamp::from_millis(5250)).unwrap(),
            0.625,
        );
    }

    /// Item 60: sampling the same timestamp many times gives an identical
    /// result every time.
    #[test]
    fn repeated_oscillator_sampling_is_identical() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Sine {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        let at = Timestamp::from_millis(1234);
        let first = store.sample(id, at);
        for _ in 0..100 {
            assert_eq!(store.sample(id, at), first);
        }
    }

    /// Item 61/18: non-monotonic sampling order never affects the result.
    #[test]
    fn non_monotonic_oscillator_sampling_is_correct() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Triangle {
            period: Duration::from_secs(2),
            started_at: Timestamp::ZERO,
        });
        for (secs, expected) in [(4, 0.0), (1, 1.0), (7, 1.0), (0, 0.0)] {
            assert_float_close(
                store.sample(id, Timestamp::from_secs(secs)).unwrap(),
                expected,
            );
        }
    }

    /// Item 19: sampling before `started_at` clamps `elapsed` to zero
    /// rather than underflowing.
    #[test]
    fn sampling_before_started_at_clamps_elapsed_to_zero() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Sine {
            period: Duration::from_secs(2),
            started_at: Timestamp::from_secs(10),
        });
        assert_float_close(store.sample(id, Timestamp::ZERO).unwrap(), 0.5);
        assert_float_close(store.sample(id, Timestamp::from_secs(5)).unwrap(), 0.5);
    }

    /// Item 15: every oscillator's output stays within `[0.0, 1.0]`
    /// across a dense sweep of phases, including near-boundary floating
    /// point cases.
    #[test]
    fn oscillator_outputs_always_stay_within_unit_bounds() {
        let mut store = SignalStore::new();
        let period = Duration::from_millis(997); // deliberately not a round number
        let sine = store.insert(SignalKind::Sine {
            period,
            started_at: Timestamp::ZERO,
        });
        let triangle = store.insert(SignalKind::Triangle {
            period,
            started_at: Timestamp::ZERO,
        });
        let saw = store.insert(SignalKind::Saw {
            period,
            started_at: Timestamp::ZERO,
        });
        let square = store.insert(SignalKind::Square {
            period,
            started_at: Timestamp::ZERO,
        });
        for millis in 0..2000u64 {
            for id in [sine, triangle, saw, square] {
                let Value::Float(v) = store.sample(id, Timestamp::from_millis(millis)).unwrap()
                else {
                    panic!("expected Float");
                };
                assert!((0.0..=1.0).contains(&v), "out of bounds: {v} at {millis}ms");
            }
        }
    }
}
