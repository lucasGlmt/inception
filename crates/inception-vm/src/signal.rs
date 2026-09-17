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
//! a pure function of `(kind, SignalSampleContext)` — `Constant` ignores
//! it entirely, the four oscillators derive their value solely from
//! `(period, started_at, context.now)`, and only [`SignalKind::Spread`]
//! also reads `context.fixture_index`/`context.fixture_count` — never
//! from any accumulated or externally mutated state — which is what makes
//! [`SignalStore::sample`] trivially deterministic: the same
//! `(id, context)` always reads the same immutable definition and
//! computes the same result. `started_at` is the one place an
//! oscillator's *construction* legitimately depends on the runtime clock
//! (see `Vm::exec_call_intrinsic`'s `Effects*` handling) — but sampling
//! itself never reads a clock, only the [`SignalSampleContext`] passed in.
//!
//! Ownership: a [`SignalStore`] lives exactly as long as the [`crate::Vm`]
//! that owns it. Reloading a program means building a new `Vm` (and so a
//! new, empty `SignalStore`) for the newly compiled module — there is no
//! migration of old `SignalId`s into a new store, matching how the rest
//! of a reloaded program's state already isn't preserved across a
//! `LoadedProgram` swap.

use inception_core::{Duration, Timestamp};

use crate::sequence::{SequenceError, SequenceId, SequenceStore};
use crate::value::Value;

/// Everything a signal needs to sample itself: an absolute timestamp,
/// plus (new in this milestone) *where* the sampling fixture sits within
/// the group the binding reaches — `fixture_index` of `fixture_count`
/// total. This is what makes [`SignalKind::Spread`] possible: every other
/// `SignalKind` ignores `fixture_index`/`fixture_count` entirely, so
/// sampling them is exactly as it was before this type existed (see the
/// [`From<Timestamp>`] impl below, which is what every pre-`.spread()`
/// call site — a bare `Timestamp` — implicitly means).
///
/// Never constructed from a fixture *name* or looked up by string: the
/// binding engine (`crate::binding::SignalBindingStore`) already resolves
/// a target's fixtures to a plain, rig-ordered `Vec<FixtureId>` once, at
/// link time — `fixture_index` is that `Vec`'s position, `fixture_count`
/// its length, both plain `usize`s carried alongside the bound
/// `SignalId`. Never a `HashMap`'s iteration order (see `AGENTS.md`'s
/// "rig architecture" rule and item 4 of the spread task brief): the same
/// rig always produces the same fixture order, so the same show always
/// spreads the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalSampleContext {
    pub now: Timestamp,
    pub fixture_index: usize,
    pub fixture_count: usize,
}

impl SignalSampleContext {
    pub fn new(now: Timestamp, fixture_index: usize, fixture_count: usize) -> Self {
        Self {
            now,
            fixture_index,
            fixture_count,
        }
    }
}

/// A signal sampled with no fixture-group context at all —
/// `fixture_index: 0`, `fixture_count: 1`. Lets every existing call site
/// that only ever had a `Timestamp` (a preview, a test, a single-fixture
/// binding, or any signal kind that predates `.spread()`) keep calling
/// [`SignalStore::sample`] with a bare `Timestamp` and an `.into()` at the
/// call boundary, unchanged: `fixture_index`/`fixture_count` only ever
/// affect [`SignalKind::Spread`]'s own offset formula, and `0 * amount /
/// 1 == 0`, so this default is inert for every other kind and for a
/// `Spread` sampled outside any real multi-fixture binding.
impl From<Timestamp> for SignalSampleContext {
    fn from(now: Timestamp) -> Self {
        Self {
            now,
            fixture_index: 0,
            fixture_count: 1,
        }
    }
}

/// One of the four base oscillators' value functions
/// (`sine_wave`/`triangle_wave`/`saw_wave`/`square_wave`), each taking a
/// normalized `phase ∈ [0, 1)` and returning a value in `0.0..=1.0`.
type Waveform = fn(f64) -> f64;

/// What [`SignalStore::oscillator_basis`] resolves a `Phase`/`Spread`
/// node's `source` to: the underlying oscillator's `period`/`started_at`/
/// `Waveform`, plus whatever static phase offset (in millidegrees) is
/// already baked in from a wrapping `Phase` node — `0` if `source` is a
/// base oscillator directly.
type OscillatorBasis = (Duration, Timestamp, Waveform, i32);

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
    /// `.range(min, max)`: remaps `source`'s `Value::Float` (clamped to
    /// `0.0..1.0`) to `min + x * (max - min)`, in `min`/`max`'s own type
    /// (`Float`/`Intensity`/`Angle` — `min`/`max` are always the same
    /// `Value` variant as each other, enforced by construction: this is
    /// only ever built from one of the three monomorphized
    /// `SignalRange*` intrinsics). Composes with *any* `source` kind, not
    /// just an oscillator — a pure value remap has no notion of "period"
    /// to depend on.
    Range {
        source: SignalId,
        min: Value,
        max: Value,
    },
    /// `.phase(offset)`: shifts `source`'s cycle by
    /// `offset_millideg / 360_000` of a period, already normalized to
    /// `0..360_000` at construction (so `450deg` and `90deg` behave
    /// identically, and a negative offset shifts the other way). `source`
    /// must be a base oscillator (`Sine`/`Triangle`/`Saw`/`Square`) —
    /// `lux-typeck` enforces this statically (transitively through other
    /// `.phase()` calls, which `Vm::exec_call_intrinsic` flattens onto the
    /// same base oscillator at construction, so this variant's `source`
    /// is always a base oscillator directly, never another `Phase`); see
    /// `docs/stdlib/Signal.md` for why phase-shifting isn't generic over
    /// every signal kind.
    Phase {
        source: SignalId,
        offset_millideg: i32,
    },
    /// `.spread(amount)`: distributes `amount_millideg` as a *per-fixture*
    /// phase offset across whatever group the signal ends up bound to —
    /// `offset = amount * fixture_index / fixture_count`, read from the
    /// [`SignalSampleContext`] passed to [`SignalStore::sample`], added on
    /// top of `source`'s own phase (including any static `.phase()`
    /// offset already baked into `source`, if `source` is a `Phase` node —
    /// see `oscillator_basis`). Deliberately `i * amount / n`, never
    /// `i * amount / (n - 1)`: the latter would put fixture `0` and
    /// fixture `n - 1` back in phase for a full `360deg` spread, which is
    /// exactly the seam item 3 of the task brief says a spread must not
    /// have.
    ///
    /// Sampled with `fixture_count == 0` (unreachable from a real
    /// binding — the linker rejects an empty role binding, see
    /// `inception_linker::LinkError::EmptyBinding` — but not something
    /// this method trusts blindly) yields offset `0` rather than dividing
    /// by zero.
    ///
    /// `source` must resolve (via [`SignalStore::oscillator_basis`]) to a
    /// base oscillator, either directly or through one `Phase` node —
    /// never through another `Spread` — for the same reason
    /// [`SignalKind::Phase`] itself is restricted: only a base oscillator
    /// has a period to compute a phase against. `lux-typeck` enforces
    /// this statically (see `docs/stdlib/Signal.md`); a hand-built
    /// `SignalKind::Spread` that violates it is a structured
    /// [`SignalError::UnsupportedSpreadSource`], never a panic.
    Spread {
        source: SignalId,
        amount_millideg: i32,
    },
    /// `.invert()`: `1.0 - source`, not clamped — inverting a `source`
    /// outside `0.0..1.0` (e.g. after `.range(10.0, 20.0)`) produces a
    /// value outside `0.0..1.0` too, matching `.range()`'s own
    /// no-implicit-clamping-of-its-own-output stance.
    Invert { source: SignalId },
    /// `Effects.step(sequence, every)`: strictly time-based, discrete
    /// stepping through `sequence`'s elements — see
    /// `docs/rfcs` and `Vm::exec_call_intrinsic`'s `EffectsStep*`
    /// handling. Never copies `sequence`'s elements inline (same
    /// `SequenceId`-indirection rationale as `Value::Sequence` itself);
    /// sampling reads whichever element `elapsed / every` (floored, mod
    /// the sequence's length) lands on, recomputed fresh from
    /// `(context.now, started_at)` every time — never accumulated
    /// frame-by-frame — so a missed frame never causes drift (item 1 of
    /// the `Effects.step` task brief). `started_at` is this signal's
    /// *construction* time (item 2), exactly like an oscillator's own
    /// `started_at`. `sequence` is guaranteed non-empty for any Step built
    /// from real Lux source (`Sequence.of()` is already a compile-time
    /// error — see `lux-typeck`'s `check_sequence_of`), but `sample`
    /// still checks defensively rather than risking a division by zero on
    /// hand-corrupted bytecode.
    Step {
        sequence: SequenceId,
        every: Duration,
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
    /// A `Phase` node's `source` doesn't resolve to a base oscillator.
    /// Unreachable from real Lux source — `lux-typeck` statically requires
    /// `.phase()`'s receiver to be an `Effects` oscillator (see
    /// `docs/stdlib/Signal.md`) — kept only for the same
    /// never-trust-hand-built-bytecode reason every other `SignalError`
    /// exists.
    UnsupportedPhaseSource(SignalId),
    /// A `Spread` node's `source` doesn't resolve to a base oscillator
    /// (directly, or through one `Phase`) or a `Step`. Unreachable from
    /// real Lux source for the same reason `UnsupportedPhaseSource` is —
    /// see `SignalKind::Spread`'s docs.
    UnsupportedSpreadSource(SignalId),
    /// A `Step` (bare or spread) referenced a `SequenceId` this signal's
    /// owning `Vm` doesn't know about — unreachable for a verified module
    /// (every `Step` is built from a `Value::Sequence` that must already
    /// exist), kept only for the same defensive reason every other
    /// `SignalError` exists.
    UnknownSequence(SequenceId),
    /// A `Step` referenced an empty sequence. Unreachable from real Lux
    /// source — `Sequence.of()` is already a compile-time error (see
    /// `lux-typeck`'s `check_sequence_of`) — kept only so `sample` never
    /// has to divide by a zero-length cycle.
    EmptySequence(SequenceId),
    /// A `Step`'s `every` is zero. A *constant* zero `every` is already
    /// rejected at compile time (mirroring `Effects.sine`/etc.'s zero-period
    /// check); `Vm::exec_call_intrinsic` additionally rejects a
    /// non-constant zero `every` before ever constructing the signal (see
    /// `VmErrorKind::InvalidSignalPeriod`) — so this is unreachable outside
    /// hand-built bytecode, kept only so `sample` never divides by a
    /// zero-length cycle.
    ZeroStepInterval(SequenceId),
}

impl From<SequenceError> for SignalError {
    /// `SequenceError::IndexOutOfBounds` can't actually occur here: every
    /// index `Step`'s sampling computes is derived as `_ % length`, always
    /// `< length` by construction — this conversion exists only so `?`
    /// works uniformly on `sequences.get(...)`'s `Result`, not because
    /// that arm is reachable. Mapped to `UnknownSequence` rather than
    /// modeled as its own `SignalError` variant, since it's not a
    /// meaningfully distinct failure for anything sampling a `Step` could
    /// do differently about it.
    fn from(err: SequenceError) -> Self {
        match err {
            SequenceError::UnknownSequence(id) => SignalError::UnknownSequence(id),
            SequenceError::IndexOutOfBounds { id, .. } => SignalError::UnknownSequence(id),
        }
    }
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

    /// The `SignalKind` `id` was constructed with, or `None` if it
    /// doesn't exist. Used only by `Vm::exec_call_intrinsic`'s
    /// `SignalPhase` construction, to flatten a chain of `.phase()`
    /// calls onto the same base oscillator (see `SignalKind::Phase`'s
    /// docs) — everything else should prefer `sample`, the only
    /// operation a signal's *definition* is otherwise meant to support.
    pub(crate) fn kind_of(&self, id: SignalId) -> Option<SignalKind> {
        self.definitions.get(id.0 as usize).map(|d| d.kind)
    }

    /// Evaluates the signal `id` against `context` (see
    /// [`SignalSampleContext`]'s docs — a bare `Timestamp` converts
    /// automatically, exactly as if `fixture_index: 0, fixture_count: 1`
    /// had been written out). Takes the timestamp explicitly rather than
    /// reading a clock itself — see this module's doc comment and the
    /// task brief's "a signal doesn't own the clock" rule — so this is
    /// exactly as deterministic and testable as the caller's own
    /// timestamp values are, with no dependency on real or virtual time
    /// beyond what's passed in. `context.now` need not be monotonically
    /// increasing across calls: nothing here assumes or requires it.
    /// `Range`/`Invert` recurse into their `source` via this same method,
    /// threading `context` through unchanged; `Phase`/`Spread` resolve
    /// their source's oscillator basis in one step without recursing (see
    /// [`Self::oscillator_basis`]). A cycle is structurally impossible to
    /// build through the normal `insert`-then-reference API: `insert`
    /// only ever hands out the *next* sequential `SignalId`, so a node's
    /// `source` (read from a `Value::Signal` that must already exist by
    /// the time this node is constructed) can only ever name an id
    /// strictly smaller than its own — there is no way to reference a
    /// not-yet-inserted signal, so no runtime cycle-detection is needed.
    /// Recursion depth is bounded by how deeply a single Lux expression
    /// chains `.range()`/`.phase()`/`.spread()`/`.invert()` — Lux has no
    /// loops or user recursion in this milestone, so that depth is exactly
    /// the source file's own written nesting, not something a program can
    /// grow unboundedly at runtime.
    ///
    /// Takes `sequences` (the same `Vm`'s `crate::sequence::SequenceStore`)
    /// purely to let [`SignalKind::Step`] read its elements — every other
    /// `SignalKind` ignores it entirely. This is the one place `sample`
    /// depends on anything beyond `self`/`context`; see `SignalKind::Step`'s
    /// docs for why a `Step` doesn't copy its sequence inline instead.
    pub fn sample(
        &self,
        id: SignalId,
        context: impl Into<SignalSampleContext>,
        sequences: &SequenceStore,
    ) -> Result<Value, SignalError> {
        let context = context.into();
        let definition = self
            .definitions
            .get(id.0 as usize)
            .ok_or(SignalError::UnknownSignal(id))?;
        Ok(match definition.kind {
            SignalKind::Constant(value) => value,
            SignalKind::Sine { period, started_at } => {
                Value::Float(sine_wave(phase_at(context.now, started_at, period)))
            }
            SignalKind::Triangle { period, started_at } => {
                Value::Float(triangle_wave(phase_at(context.now, started_at, period)))
            }
            SignalKind::Saw { period, started_at } => {
                Value::Float(saw_wave(phase_at(context.now, started_at, period)))
            }
            SignalKind::Square { period, started_at } => {
                Value::Float(square_wave(phase_at(context.now, started_at, period)))
            }
            SignalKind::Range { source, min, max } => {
                let x = match self.sample(source, context, sequences)? {
                    Value::Float(f) => f.clamp(0.0, 1.0),
                    _ => unreachable!(
                        "SignalStore::sample: Range source must sample to Float — \
                         enforced by lux-typeck"
                    ),
                };
                range_value(x, min, max)
            }
            SignalKind::Phase {
                source,
                offset_millideg,
            } => {
                let definition = self
                    .definitions
                    .get(source.0 as usize)
                    .ok_or(SignalError::UnknownSignal(source))?;
                let (period, started_at, waveform): (Duration, Timestamp, Waveform) =
                    match definition.kind {
                        SignalKind::Sine { period, started_at } => {
                            (period, started_at, sine_wave as Waveform)
                        }
                        SignalKind::Triangle { period, started_at } => {
                            (period, started_at, triangle_wave as Waveform)
                        }
                        SignalKind::Saw { period, started_at } => {
                            (period, started_at, saw_wave as Waveform)
                        }
                        SignalKind::Square { period, started_at } => {
                            (period, started_at, square_wave as Waveform)
                        }
                        _ => return Err(SignalError::UnsupportedPhaseSource(source)),
                    };
                let base_phase = phase_at(context.now, started_at, period);
                let offset_fraction = offset_millideg as f64 / 360_000.0;
                let shifted = (base_phase + offset_fraction).rem_euclid(1.0);
                Value::Float(waveform(shifted))
            }
            SignalKind::Spread {
                source,
                amount_millideg,
            } => {
                // `i * amount / n`, never `i * amount / (n - 1)` — see
                // `SignalKind::Spread`'s docs for why the latter would be
                // wrong. Guarded against `fixture_count == 0` (item 10):
                // that combination can't come from a real binding (the
                // linker rejects an empty role), but `sample` never
                // trusts that blindly. Shared by both branches below:
                // an oscillator interprets this as a phase fraction of
                // its own period, a `Step` as a time fraction of its
                // full sequence cycle (`every * length`) — see item 10
                // of the `Effects.step` task brief.
                let spread_offset_millideg = if context.fixture_count == 0 {
                    0
                } else {
                    (amount_millideg as i64 * context.fixture_index as i64
                        / context.fixture_count as i64) as i32
                };

                match self.definitions.get(source.0 as usize).map(|d| d.kind) {
                    Some(SignalKind::Step {
                        sequence,
                        every,
                        started_at,
                    }) => sample_step(
                        sequences,
                        sequence,
                        every,
                        started_at,
                        context.now,
                        spread_offset_millideg,
                    )?,
                    _ => {
                        let (period, started_at, waveform, static_offset_millideg) = self
                            .oscillator_basis(source)
                            .ok_or(SignalError::UnsupportedSpreadSource(source))?;
                        let total_offset_millideg =
                            (static_offset_millideg + spread_offset_millideg).rem_euclid(360_000);
                        let base_phase = phase_at(context.now, started_at, period);
                        let offset_fraction = total_offset_millideg as f64 / 360_000.0;
                        let shifted = (base_phase + offset_fraction).rem_euclid(1.0);
                        Value::Float(waveform(shifted))
                    }
                }
            }
            SignalKind::Step {
                sequence,
                every,
                started_at,
            } => sample_step(sequences, sequence, every, started_at, context.now, 0)?,
            SignalKind::Invert { source } => {
                let x = match self.sample(source, context, sequences)? {
                    Value::Float(f) => f,
                    _ => unreachable!(
                        "SignalStore::sample: Invert source must sample to Float — \
                         enforced by lux-typeck"
                    ),
                };
                Value::Float(1.0 - x)
            }
        })
    }

    /// The `(period, started_at, waveform, static_offset_millideg)` a
    /// [`SignalKind::Spread`] node's `source` resolves to — `source` is
    /// either a base oscillator directly (`static_offset_millideg: 0`) or
    /// a `Phase` node wrapping one (`static_offset_millideg` is that
    /// `Phase`'s own `offset_millideg`, so a `.phase(...).spread(...)`
    /// chain's global phase and per-fixture spread add together, exactly
    /// as `docs/language/signals.md`'s "global phase + fixture-specific
    /// spread phase" describes). `None` for anything else (a `Constant`,
    /// `Range`, `Invert`, or another `Spread`) — `lux-typeck` guarantees
    /// real Lux source never builds one of those, so `Spread`'s own
    /// `sample` arm turns `None` into the structured
    /// [`SignalError::UnsupportedSpreadSource`], never a panic.
    fn oscillator_basis(&self, id: SignalId) -> Option<OscillatorBasis> {
        fn base(kind: SignalKind) -> Option<(Duration, Timestamp, Waveform)> {
            match kind {
                SignalKind::Sine { period, started_at } => {
                    Some((period, started_at, sine_wave as Waveform))
                }
                SignalKind::Triangle { period, started_at } => {
                    Some((period, started_at, triangle_wave as Waveform))
                }
                SignalKind::Saw { period, started_at } => {
                    Some((period, started_at, saw_wave as Waveform))
                }
                SignalKind::Square { period, started_at } => {
                    Some((period, started_at, square_wave as Waveform))
                }
                _ => None,
            }
        }

        let kind = self.definitions.get(id.0 as usize)?.kind;
        match kind {
            SignalKind::Phase {
                source,
                offset_millideg,
            } => {
                let inner = self.definitions.get(source.0 as usize)?.kind;
                let (period, started_at, waveform) = base(inner)?;
                Some((period, started_at, waveform, offset_millideg))
            }
            other => {
                let (period, started_at, waveform) = base(other)?;
                Some((period, started_at, waveform, 0))
            }
        }
    }
}

/// `min + x * (max - min)`, `x` already clamped to `0.0..1.0` by the
/// caller — computed in `min`/`max`'s own native representation
/// (`Intensity`'s raw `u16`, `Angle`'s millidegrees), never via a
/// percent/degree string round trip. `min` may be greater than `max`
/// (item 14): the formula itself handles a reversed ramp with no special
/// casing.
fn range_value(x: f64, min: Value, max: Value) -> Value {
    match (min, max) {
        (Value::Float(min), Value::Float(max)) => Value::Float(min + x * (max - min)),
        (Value::Intensity(min), Value::Intensity(max)) => {
            let result = min as f64 + x * (max as f64 - min as f64);
            Value::Intensity(result.round().clamp(0.0, u16::MAX as f64) as u16)
        }
        (Value::Angle(min), Value::Angle(max)) => {
            let result = min as f64 + x * (max as f64 - min as f64);
            Value::Angle(result.round() as i32)
        }
        _ => unreachable!(
            "range_value: min/max variant mismatch — enforced by lux-typeck/construction"
        ),
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

/// The element of `sequence` that `SignalKind::Step`/`SignalKind::Spread`
/// (wrapping a `Step`) sample to at `now`. Shared by both call sites (see
/// their sample arms) — `offset_millideg` is `0` for a bare `Step`, or the
/// per-fixture spread share for a `Spread`-wrapped one (see
/// `SignalKind::Spread`'s sample arm), interpreted as a fraction of the
/// full sequence cycle (`every * length`) rather than of a single
/// element's `every` — this is what item 10 of the `Effects.step` task
/// brief means by "fixture-specific phase offset -> equivalent time
/// offset within full sequence cycle".
///
/// All-integer nanosecond arithmetic (`i128` only as an overflow-safe
/// intermediate for the offset's multiplication) — no floating-point
/// drift, and `elapsed`/`shifted_ns` are recomputed from scratch on every
/// call, never accumulated, so a missed frame or a non-monotonic `now`
/// never skews which index this lands on (items 1/7 of the task brief).
/// Returns a structured [`SignalError`], never panics, on an unknown
/// sequence, an empty one, or a zero `every` — see those variants' docs
/// for why each is unreachable from real Lux source but still guarded.
fn sample_step(
    sequences: &SequenceStore,
    sequence: SequenceId,
    every: Duration,
    started_at: Timestamp,
    now: Timestamp,
    offset_millideg: i32,
) -> Result<Value, SignalError> {
    let length = sequences
        .length(sequence)
        .ok_or(SignalError::UnknownSequence(sequence))?;
    if length == 0 {
        return Err(SignalError::EmptySequence(sequence));
    }
    let every_ns = every.as_nanos();
    if every_ns == 0 {
        return Err(SignalError::ZeroStepInterval(sequence));
    }
    let cycle_ns = every_ns.saturating_mul(length as u64);
    let elapsed_ns = now.as_nanos().saturating_sub(started_at.as_nanos()) as i128;
    let offset_ns = (offset_millideg as i128 * cycle_ns as i128) / 360_000i128;
    let shifted_ns = (elapsed_ns + offset_ns).rem_euclid(cycle_ns as i128) as u64;
    let index = ((shifted_ns / every_ns) % length as u64) as usize;
    sequences
        .get(sequence, index as i64)
        .map_err(SignalError::from)
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
            assert_eq!(
                store.sample(id, at, &SequenceStore::new()),
                Ok(Value::Intensity(32767))
            );
        }
    }

    #[test]
    fn constant_color_signal_samples_the_same_value_at_any_timestamp() {
        let mut store = SignalStore::new();
        let red = ColorValue { r: 255, g: 0, b: 0 };
        let id = store.insert(SignalKind::Constant(Value::Color(red)));

        for at in [Timestamp::ZERO, Timestamp::from_secs(1)] {
            assert_eq!(
                store.sample(id, at, &SequenceStore::new()),
                Ok(Value::Color(red))
            );
        }
    }

    #[test]
    fn repeated_sampling_is_identical() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Constant(Value::Float(1.5)));
        let at = Timestamp::from_millis(1234);

        let first = store.sample(id, at, &SequenceStore::new());
        for _ in 0..100 {
            assert_eq!(store.sample(id, at, &SequenceStore::new()), first);
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
            assert_eq!(
                store.sample(id, at, &SequenceStore::new()),
                Ok(Value::Int(42))
            );
        }
    }

    #[test]
    fn unknown_signal_id_is_a_structured_error_not_a_panic() {
        let store = SignalStore::new();
        assert_eq!(
            store.sample(SignalId(0), Timestamp::ZERO, &SequenceStore::new()),
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
                store
                    .sample(id, Timestamp::from_millis(millis), &SequenceStore::new())
                    .unwrap(),
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
                store
                    .sample(id, Timestamp::from_millis(millis), &SequenceStore::new())
                    .unwrap(),
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
                store
                    .sample(id, Timestamp::from_millis(millis), &SequenceStore::new())
                    .unwrap(),
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
                store
                    .sample(id, Timestamp::from_millis(millis), &SequenceStore::new())
                    .unwrap(),
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
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(25), &SequenceStore::new())
                .unwrap(),
            0.25,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(150), &SequenceStore::new())
                .unwrap(),
            0.5,
        );
    }

    /// Item 57: a long (1h) period, sampled at several positions.
    #[test]
    fn long_period_oscillates_correctly() {
        let mut store = SignalStore::new();
        let id = store.insert(SignalKind::Saw {
            period: Duration::from_secs(3600),
            started_at: Timestamp::ZERO,
        });
        assert_float_close(
            store
                .sample(id, Timestamp::from_secs(900), &SequenceStore::new())
                .unwrap(),
            0.25,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_secs(1800), &SequenceStore::new())
                .unwrap(),
            0.5,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_secs(3600), &SequenceStore::new())
                .unwrap(),
            0.0,
        );
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
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(5000), &SequenceStore::new())
                .unwrap(),
            0.5,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(5500), &SequenceStore::new())
                .unwrap(),
            1.0,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(6000), &SequenceStore::new())
                .unwrap(),
            0.5,
        );
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
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(0), &SequenceStore::new())
                .unwrap(),
            0.0,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(1750), &SequenceStore::new())
                .unwrap(),
            0.875,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_millis(5250), &SequenceStore::new())
                .unwrap(),
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
        let first = store.sample(id, at, &SequenceStore::new());
        for _ in 0..100 {
            assert_eq!(store.sample(id, at, &SequenceStore::new()), first);
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
                store
                    .sample(id, Timestamp::from_secs(secs), &SequenceStore::new())
                    .unwrap(),
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
        assert_float_close(
            store
                .sample(id, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
            0.5,
        );
        assert_float_close(
            store
                .sample(id, Timestamp::from_secs(5), &SequenceStore::new())
                .unwrap(),
            0.5,
        );
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
                let Value::Float(v) = store
                    .sample(id, Timestamp::from_millis(millis), &SequenceStore::new())
                    .unwrap()
                else {
                    panic!("expected Float");
                };
                assert!((0.0..=1.0).contains(&v), "out of bounds: {v} at {millis}ms");
            }
        }
    }

    fn sine_source(store: &mut SignalStore, period_ms: u64) -> SignalId {
        store.insert(SignalKind::Sine {
            period: Duration::from_millis(period_ms),
            started_at: Timestamp::ZERO,
        })
    }

    /// Item 11: `range(10.0, 20.0)` over a controlled `Constant` source
    /// (no need to route through an oscillator for a pure formula check).
    #[test]
    fn range_float_matches_the_linear_formula() {
        let mut store = SignalStore::new();
        for (x, expected) in [(0.0, 10.0), (0.25, 12.5), (0.5, 15.0), (1.0, 20.0)] {
            let source = store.insert(SignalKind::Constant(Value::Float(x)));
            let range = store.insert(SignalKind::Range {
                source,
                min: Value::Float(10.0),
                max: Value::Float(20.0),
            });
            let Value::Float(v) = store
                .sample(range, Timestamp::ZERO, &SequenceStore::new())
                .unwrap()
            else {
                panic!("expected Float");
            };
            assert!(
                (v - expected).abs() < 1e-9,
                "x={x}: expected {expected}, got {v}"
            );
        }
    }

    /// Item 12: `range(20%, 100%)`, native `Intensity` arithmetic (never
    /// a percent-string round trip).
    #[test]
    fn range_intensity_matches_the_documented_table() {
        let min = 13107u16; // 20%
        let max = 65535u16; // 100%
        for (x, expected) in [
            (0.0, 13107u16),
            (0.25, 26214),
            (0.5, 39321),
            (0.75, 52428),
            (1.0, 65535),
        ] {
            let mut store = SignalStore::new();
            let source = store.insert(SignalKind::Constant(Value::Float(x)));
            let range = store.insert(SignalKind::Range {
                source,
                min: Value::Intensity(min),
                max: Value::Intensity(max),
            });
            assert_eq!(
                store
                    .sample(range, Timestamp::ZERO, &SequenceStore::new())
                    .unwrap(),
                Value::Intensity(expected),
                "x={x}"
            );
        }
    }

    /// Item 13: `range(0deg, 180deg)`.
    #[test]
    fn range_angle_matches_the_documented_table() {
        for (x, expected_millideg) in [(0.0, 0), (0.5, 90_000), (1.0, 180_000)] {
            let mut store = SignalStore::new();
            let source = store.insert(SignalKind::Constant(Value::Float(x)));
            let range = store.insert(SignalKind::Range {
                source,
                min: Value::Angle(0),
                max: Value::Angle(180_000),
            });
            assert_eq!(
                store
                    .sample(range, Timestamp::ZERO, &SequenceStore::new())
                    .unwrap(),
                Value::Angle(expected_millideg),
                "x={x}"
            );
        }
    }

    /// Item 14: reversed bounds invert the ramp naturally, no special
    /// casing (`min` need not be `<= max`).
    #[test]
    fn range_with_reversed_bounds_inverts_the_ramp() {
        // min=100% (65535), max=0%: result = 65535 * (1 - x).
        for (x, expected) in [(0.0, 65535u16), (0.5, 32768), (1.0, 0)] {
            let mut store = SignalStore::new();
            let source = store.insert(SignalKind::Constant(Value::Float(x)));
            let range = store.insert(SignalKind::Range {
                source,
                min: Value::Intensity(65535),
                max: Value::Intensity(0),
            });
            assert_eq!(
                store
                    .sample(range, Timestamp::ZERO, &SequenceStore::new())
                    .unwrap(),
                Value::Intensity(expected),
                "x={x}"
            );
        }
    }

    /// Item 15: range clamps its *source* into `0.0..1.0` before applying
    /// the formula — never extrapolates.
    #[test]
    fn range_clamps_out_of_bounds_source_values() {
        let mut store = SignalStore::new();
        let below = store.insert(SignalKind::Constant(Value::Float(-5.0)));
        let above = store.insert(SignalKind::Constant(Value::Float(5.0)));
        let range_below = store.insert(SignalKind::Range {
            source: below,
            min: Value::Float(0.0),
            max: Value::Float(10.0),
        });
        let range_above = store.insert(SignalKind::Range {
            source: above,
            min: Value::Float(0.0),
            max: Value::Float(10.0),
        });
        assert_eq!(
            store
                .sample(range_below, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
            Value::Float(0.0)
        );
        assert_eq!(
            store
                .sample(range_above, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
            Value::Float(10.0)
        );
    }

    /// Item 58: `.phase(90deg)` on a sine at `t=0` reads what the
    /// unshifted signal would read at its own quarter-period.
    #[test]
    fn phase_90_degrees_shifts_a_quarter_cycle() {
        let mut store = SignalStore::new();
        let source = sine_source(&mut store, 2000);
        let shifted = store.insert(SignalKind::Phase {
            source,
            offset_millideg: 90_000,
        });
        let assert_close = |at_ms: u64, expected: f64| {
            let Value::Float(v) = store
                .sample(
                    shifted,
                    Timestamp::from_millis(at_ms),
                    &SequenceStore::new(),
                )
                .unwrap()
            else {
                panic!("expected Float");
            };
            assert!(
                (v - expected).abs() < 1e-9,
                "at {at_ms}ms: expected {expected}, got {v}"
            );
        };
        assert_close(0, 1.0); // same as unshifted sine at t=500ms
        assert_close(500, 0.5); // same as unshifted sine at t=1000ms
    }

    /// Item 59: `.phase(180deg)` at `t=0` samples the same *value* as the
    /// unshifted sine (both read 0.5 — sine is symmetric there), but the
    /// two must diverge at a different timestamp.
    #[test]
    fn phase_180_degrees_is_distinguishable_from_the_original_elsewhere() {
        let mut store = SignalStore::new();
        let source = sine_source(&mut store, 2000);
        let shifted = store.insert(SignalKind::Phase {
            source,
            offset_millideg: 180_000,
        });
        let (Value::Float(shifted_at_zero), Value::Float(source_at_zero)) = (
            store
                .sample(shifted, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
            store
                .sample(source, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
        ) else {
            panic!("expected Float");
        };
        assert!((shifted_at_zero - source_at_zero).abs() < 1e-9);
        assert_ne!(
            store
                .sample(shifted, Timestamp::from_millis(500), &SequenceStore::new())
                .unwrap(),
            store
                .sample(source, Timestamp::from_millis(500), &SequenceStore::new())
                .unwrap(),
        );
    }

    /// Item 60: a full-turn offset is a no-op.
    #[test]
    fn phase_360_degrees_is_identical_to_no_phase() {
        let mut store = SignalStore::new();
        let source = sine_source(&mut store, 2000);
        let shifted = store.insert(SignalKind::Phase {
            source,
            offset_millideg: 360_000,
        });
        for millis in [0, 250, 999, 1750] {
            assert_eq!(
                store
                    .sample(
                        shifted,
                        Timestamp::from_millis(millis),
                        &SequenceStore::new()
                    )
                    .unwrap(),
                store
                    .sample(
                        source,
                        Timestamp::from_millis(millis),
                        &SequenceStore::new()
                    )
                    .unwrap(),
            );
        }
    }

    /// Item 61: a multi-turn offset (`450deg`) behaves like its
    /// mod-360 equivalent (`90deg`).
    #[test]
    fn phase_450_degrees_behaves_like_90_degrees() {
        let mut store = SignalStore::new();
        let source_a = sine_source(&mut store, 2000);
        let phase_450 = store.insert(SignalKind::Phase {
            source: source_a,
            offset_millideg: 450_000,
        });
        let source_b = sine_source(&mut store, 2000);
        let phase_90 = store.insert(SignalKind::Phase {
            source: source_b,
            offset_millideg: 90_000,
        });
        for millis in [0, 500, 1000] {
            assert_eq!(
                store
                    .sample(
                        phase_450,
                        Timestamp::from_millis(millis),
                        &SequenceStore::new()
                    )
                    .unwrap(),
                store
                    .sample(
                        phase_90,
                        Timestamp::from_millis(millis),
                        &SequenceStore::new()
                    )
                    .unwrap(),
            );
        }
    }

    /// Item 24: a negative offset shifts the other way, wrapping via
    /// `rem_euclid`, never panicking.
    #[test]
    fn phase_negative_offset_wraps_correctly() {
        let mut store = SignalStore::new();
        let source_a = sine_source(&mut store, 2000);
        let phase_neg_90 = store.insert(SignalKind::Phase {
            source: source_a,
            offset_millideg: -90_000,
        });
        let source_b = sine_source(&mut store, 2000);
        let phase_270 = store.insert(SignalKind::Phase {
            source: source_b,
            offset_millideg: 270_000,
        });
        for millis in [0, 333, 1999] {
            assert_eq!(
                store
                    .sample(
                        phase_neg_90,
                        Timestamp::from_millis(millis),
                        &SequenceStore::new()
                    )
                    .unwrap(),
                store
                    .sample(
                        phase_270,
                        Timestamp::from_millis(millis),
                        &SequenceStore::new()
                    )
                    .unwrap(),
            );
        }
    }

    /// A `Phase` whose source isn't a base oscillator (only reachable via
    /// hand-built `SignalKind`, never real Lux source — `lux-typeck`
    /// rejects this statically) is a structured error, not a panic.
    #[test]
    fn phase_on_a_non_oscillator_source_is_a_structured_error() {
        let mut store = SignalStore::new();
        let constant = store.insert(SignalKind::Constant(Value::Float(0.5)));
        let bad_phase = store.insert(SignalKind::Phase {
            source: constant,
            offset_millideg: 90_000,
        });
        assert_eq!(
            store.sample(bad_phase, Timestamp::ZERO, &SequenceStore::new()),
            Err(SignalError::UnsupportedPhaseSource(constant))
        );
    }

    /// Item 30.
    #[test]
    fn invert_computes_one_minus_source() {
        let mut store = SignalStore::new();
        let source = store.insert(SignalKind::Constant(Value::Float(0.3)));
        let inverted = store.insert(SignalKind::Invert { source });
        let Value::Float(v) = store
            .sample(inverted, Timestamp::ZERO, &SequenceStore::new())
            .unwrap()
        else {
            panic!("expected Float");
        };
        assert!((v - 0.7).abs() < 1e-9);
    }

    /// Item 62: `sine -> phase(90deg) -> range(20%, 100%)`, sampled at
    /// several timestamps.
    #[test]
    fn chained_transformations_compose_correctly() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let phased = store.insert(SignalKind::Phase {
            source: sine,
            offset_millideg: 90_000,
        });
        let ranged = store.insert(SignalKind::Range {
            source: phased,
            min: Value::Intensity(13107), // 20%
            max: Value::Intensity(65535), // 100%
        });
        // phased sine at t=0 reads 1.0 (see `phase_90_degrees_shifts_a_quarter_cycle`),
        // so range(20%, 100%) at x=1.0 must read exactly 100%.
        assert_eq!(
            store
                .sample(ranged, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
            Value::Intensity(65535)
        );
        // at t=500ms, phased sine reads 0.5, the midpoint of the range.
        assert_eq!(
            store
                .sample(ranged, Timestamp::from_millis(500), &SequenceStore::new())
                .unwrap(),
            Value::Intensity(39321)
        );
    }

    /// Item 3/11: `.spread(360deg)` over 4 fixtures distributes phase as
    /// `0°, 90°, 180°, 270°` — `i * amount / n`, never `i * amount /
    /// (n - 1)` (which would put fixture 0 and fixture 3 back in phase).
    #[test]
    fn spread_360_over_four_fixtures_matches_the_documented_table() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let spread = store.insert(SignalKind::Spread {
            source: sine,
            amount_millideg: 360_000,
        });
        for (fixture_index, expected_offset_deg) in [(0, 0), (1, 90), (2, 180), (3, 270)] {
            let context = SignalSampleContext::new(Timestamp::ZERO, fixture_index, 4);
            let Value::Float(spread_value) = store
                .sample(spread, context, &SequenceStore::new())
                .unwrap()
            else {
                panic!("expected Float");
            };
            // Compare against the unshifted sine sampled at the
            // equivalent phase offset directly, rather than
            // hand-computing `sine_wave` again here.
            let Value::Float(expected) = store
                .sample(
                    sine,
                    Timestamp::from_millis((expected_offset_deg * 2000 / 360) as u64),
                    &SequenceStore::new(),
                )
                .unwrap()
            else {
                panic!("expected Float");
            };
            assert!(
                (spread_value - expected).abs() < 1e-9,
                "fixture {fixture_index}: expected {expected}, got {spread_value}"
            );
        }
    }

    /// Item 12: `.spread(180deg)` over 4 fixtures distributes phase as
    /// `0°, 45°, 90°, 135°`.
    #[test]
    fn spread_180_over_four_fixtures_matches_the_documented_table() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let spread = store.insert(SignalKind::Spread {
            source: sine,
            amount_millideg: 180_000,
        });
        for (fixture_index, expected_offset_deg) in [(0, 0), (1, 45), (2, 90), (3, 135)] {
            let context = SignalSampleContext::new(Timestamp::ZERO, fixture_index, 4);
            let Value::Float(spread_value) = store
                .sample(spread, context, &SequenceStore::new())
                .unwrap()
            else {
                panic!("expected Float");
            };
            let Value::Float(expected) = store
                .sample(
                    sine,
                    Timestamp::from_millis((expected_offset_deg * 2000 / 360) as u64),
                    &SequenceStore::new(),
                )
                .unwrap()
            else {
                panic!("expected Float");
            };
            assert!(
                (spread_value - expected).abs() < 1e-9,
                "fixture {fixture_index}: expected {expected}, got {spread_value}"
            );
        }
    }

    /// Item 9: a single fixture (`fixture_count: 1`) always gets offset
    /// `0`, regardless of `amount` — identical to the unshifted source.
    #[test]
    fn spread_with_a_single_fixture_is_a_no_op() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let spread = store.insert(SignalKind::Spread {
            source: sine,
            amount_millideg: 360_000,
        });
        for millis in [0, 250, 999, 1750] {
            let context = SignalSampleContext::new(Timestamp::from_millis(millis), 0, 1);
            assert_eq!(
                store
                    .sample(spread, context, &SequenceStore::new())
                    .unwrap(),
                store
                    .sample(sine, Timestamp::from_millis(millis), &SequenceStore::new())
                    .unwrap(),
            );
        }
    }

    /// Item 10: an empty group (`fixture_count: 0`) never divides by
    /// zero — it's treated the same as offset `0`, not a panic.
    #[test]
    fn spread_with_zero_fixtures_does_not_panic() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let spread = store.insert(SignalKind::Spread {
            source: sine,
            amount_millideg: 360_000,
        });
        let context = SignalSampleContext::new(Timestamp::ZERO, 0, 0);
        assert_eq!(
            store
                .sample(spread, context, &SequenceStore::new())
                .unwrap(),
            store
                .sample(sine, Timestamp::ZERO, &SequenceStore::new())
                .unwrap(),
        );
    }

    /// Item 6: `.phase(45deg).spread(360deg)` composes the two offsets
    /// additively — the static `.phase()` offset applies to every
    /// fixture, on top of each fixture's own spread offset.
    #[test]
    fn spread_composes_with_a_preceding_phase() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let phased = store.insert(SignalKind::Phase {
            source: sine,
            offset_millideg: 45_000,
        });
        let spread = store.insert(SignalKind::Spread {
            source: phased,
            amount_millideg: 360_000,
        });
        for fixture_index in 0..4usize {
            let context = SignalSampleContext::new(Timestamp::ZERO, fixture_index, 4);
            let spread_offset_deg = 90 * fixture_index as i32;
            let total_offset_millideg = 45_000 + spread_offset_deg * 1000;
            let expected_millis = (total_offset_millideg as i64 * 2000 / 360_000) as u64;
            let Value::Float(spread_value) = store
                .sample(spread, context, &SequenceStore::new())
                .unwrap()
            else {
                panic!("expected Float");
            };
            let Value::Float(expected) = store
                .sample(
                    sine,
                    Timestamp::from_millis(expected_millis),
                    &SequenceStore::new(),
                )
                .unwrap()
            else {
                panic!("expected Float");
            };
            assert!(
                (spread_value - expected).abs() < 1e-6,
                "fixture {fixture_index}: expected {expected}, got {spread_value}"
            );
        }
    }

    /// Item 14: skipping frames never matters — `Spread` is recomputed
    /// from `(context.now, fixture_index, fixture_count)` alone, exactly
    /// like every other oscillator-derived kind.
    #[test]
    fn spread_survives_missed_frames() {
        let mut store = SignalStore::new();
        let sine = sine_source(&mut store, 2000);
        let spread = store.insert(SignalKind::Spread {
            source: sine,
            amount_millideg: 360_000,
        });
        let context_at =
            |millis: u64| SignalSampleContext::new(Timestamp::from_millis(millis), 1, 4);
        // Regardless of what (if anything) was sampled before, jumping
        // straight to an irregular timestamp reads the same value as
        // sampling it directly.
        let direct = store
            .sample(spread, context_at(1750), &SequenceStore::new())
            .unwrap();
        let _ = store
            .sample(spread, context_at(0), &SequenceStore::new())
            .unwrap();
        let _ = store
            .sample(spread, context_at(333), &SequenceStore::new())
            .unwrap();
        let after_skips = store
            .sample(spread, context_at(1750), &SequenceStore::new())
            .unwrap();
        assert_eq!(direct, after_skips);
    }

    /// A `Spread` whose source isn't a base oscillator or a `Phase`
    /// wrapping one (only reachable via hand-built `SignalKind`, never
    /// real Lux source — `lux-typeck` rejects this statically) is a
    /// structured error, not a panic.
    #[test]
    fn spread_on_a_non_oscillator_source_is_a_structured_error() {
        let mut store = SignalStore::new();
        let constant = store.insert(SignalKind::Constant(Value::Float(0.5)));
        let bad_spread = store.insert(SignalKind::Spread {
            source: constant,
            amount_millideg: 360_000,
        });
        assert_eq!(
            store.sample(bad_spread, Timestamp::ZERO, &SequenceStore::new()),
            Err(SignalError::UnsupportedSpreadSource(constant))
        );
    }

    /// Items 3/4: `b = a.phase(...)` references `a`'s `SignalId`, it does
    /// not copy its definition — mutating what `a` *would* sample (there
    /// is no mutation API, but this asserts the reference is live: `a`
    /// and `b` share the same underlying oscillator origin/period).
    #[test]
    fn transformations_reference_their_source_by_id_not_by_copy() {
        let mut store = SignalStore::new();
        let a = sine_source(&mut store, 2000);
        let b = store.insert(SignalKind::Phase {
            source: a,
            offset_millideg: 0,
        });
        assert_eq!(
            store.kind_of(b),
            Some(SignalKind::Phase {
                source: a,
                offset_millideg: 0
            })
        );
        // `a` itself is untouched — still a plain Sine.
        assert!(matches!(store.kind_of(a), Some(SignalKind::Sine { .. })));
    }

    fn palette() -> (SequenceStore, SequenceId) {
        let mut sequences = SequenceStore::new();
        let id = sequences.insert(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);
        (sequences, id)
    }

    /// The task brief's own worked example: `0.0s -> element 0`,
    /// `0.5s -> element 1`, `1.0s -> element 2`, `1.5s -> element 0`
    /// (looping), for a 3-element sequence stepped every 500ms.
    #[test]
    fn step_loops_through_elements_on_a_strict_time_cadence() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        for (millis, expected) in [
            (0, 0),
            (499, 0),
            (500, 1),
            (999, 1),
            (1000, 2),
            (1499, 2),
            (1500, 0),
            (2000, 1),
        ] {
            assert_eq!(
                store.sample(step, Timestamp::from_millis(millis), &sequences),
                Ok(Value::Int(expected)),
                "at {millis}ms"
            );
        }
    }

    /// Item 2: the signal's origin is its *construction* time, not
    /// absolute zero — sampled here directly against a non-zero
    /// `started_at` (the `Vm`-level "wait 2s; then step(...)" scenario is
    /// covered in `inception-vm`'s e2e tests).
    #[test]
    fn step_origin_is_started_at_not_absolute_zero() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::from_secs(2),
        });
        assert_eq!(
            store.sample(step, Timestamp::from_secs(2), &sequences),
            Ok(Value::Int(0))
        );
        assert_eq!(
            store.sample(step, Timestamp::from_millis(2500), &sequences),
            Ok(Value::Int(1))
        );
        // Sampling before the origin clamps to element 0, same policy as
        // an oscillator sampled before its own `started_at`.
        assert_eq!(
            store.sample(step, Timestamp::ZERO, &sequences),
            Ok(Value::Int(0))
        );
    }

    /// Item 1/7: a missed frame never causes drift — sampling out of
    /// order (`4s`, `1s`, `7s`) still returns exactly what a direct
    /// sample at each of those timestamps would.
    #[test]
    fn step_sampling_is_correct_regardless_of_order() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_secs(1),
            started_at: Timestamp::ZERO,
        });
        for (secs, expected) in [(4, 1), (1, 1), (7, 1), (0, 0), (2, 2)] {
            assert_eq!(
                store.sample(step, Timestamp::from_secs(secs), &sequences),
                Ok(Value::Int(expected)),
                "at {secs}s"
            );
        }
    }

    #[test]
    fn step_sampling_is_stable_across_repeated_calls() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        let at = Timestamp::from_millis(1234);
        let first = store.sample(step, at, &sequences);
        for _ in 0..100 {
            assert_eq!(store.sample(step, at, &sequences), first);
        }
    }

    #[test]
    fn step_on_unknown_sequence_is_a_structured_error_not_a_panic() {
        let mut store = SignalStore::new();
        let bogus = SequenceId(0);
        let step = store.insert(SignalKind::Step {
            sequence: bogus,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        assert_eq!(
            store.sample(step, Timestamp::ZERO, &SequenceStore::new()),
            Err(SignalError::UnknownSequence(bogus))
        );
    }

    #[test]
    fn step_on_empty_sequence_is_a_structured_error_not_a_panic() {
        let mut sequences = SequenceStore::new();
        let empty = sequences.insert(vec![]);
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence: empty,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        assert_eq!(
            store.sample(step, Timestamp::ZERO, &sequences),
            Err(SignalError::EmptySequence(empty))
        );
    }

    #[test]
    fn step_with_zero_every_is_a_structured_error_not_a_panic() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::ZERO,
            started_at: Timestamp::ZERO,
        });
        assert_eq!(
            store.sample(step, Timestamp::ZERO, &sequences),
            Err(SignalError::ZeroStepInterval(sequence))
        );
    }

    /// Item 10 of the `Effects.step` task brief, the exact worked example:
    /// 4 elements stepped every 500ms (a 2s full cycle), spread 360deg
    /// over 4 fixtures — fixture `i` reads element `i` at `t=0`, i.e. an
    /// effective time offset of `i * 500ms`.
    #[test]
    fn spread_on_step_distributes_a_time_offset_across_fixtures() {
        let mut sequences = SequenceStore::new();
        let sequence = sequences.insert(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]);
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        let spread = store.insert(SignalKind::Spread {
            source: step,
            amount_millideg: 360_000,
        });

        for (fixture_index, expected_element) in [(0, 0), (1, 1), (2, 2), (3, 3)] {
            let context = SignalSampleContext::new(Timestamp::ZERO, fixture_index, 4);
            assert_eq!(
                store.sample(spread, context, &sequences),
                Ok(Value::Int(expected_element)),
                "fixture {fixture_index}"
            );
        }
    }

    /// A half-cycle (180deg) spread over the same 4-fixture/4-element
    /// setup distributes a 1s (half of the 2s cycle) offset: fixture `i`
    /// effectively reads `250ms * i` ahead.
    #[test]
    fn spread_on_step_with_a_partial_turn_distributes_a_fractional_offset() {
        let mut sequences = SequenceStore::new();
        let sequence = sequences.insert(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]);
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        let spread = store.insert(SignalKind::Spread {
            source: step,
            amount_millideg: 180_000,
        });

        // 180deg over 4 fixtures: offsets 0, 250, 500, 750ms -> elements
        // (250ms alone isn't a full step yet, so fixture 1 still reads
        // element 0 at t=0; fixture 2's 500ms offset lands exactly on
        // element 1).
        for (fixture_index, expected_element) in [(0, 0), (1, 0), (2, 1), (3, 1)] {
            let context = SignalSampleContext::new(Timestamp::ZERO, fixture_index, 4);
            assert_eq!(
                store.sample(spread, context, &sequences),
                Ok(Value::Int(expected_element)),
                "fixture {fixture_index}"
            );
        }
    }

    /// A single-fixture sample (`fixture_count: 1`, the default outside
    /// any real multi-fixture binding) always gets offset `0`, identical
    /// to the unshifted `Step` — same policy as spread over an oscillator.
    #[test]
    fn spread_on_step_with_a_single_fixture_is_a_no_op() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        let spread = store.insert(SignalKind::Spread {
            source: step,
            amount_millideg: 360_000,
        });
        for millis in [0, 250, 750, 1600] {
            let context = SignalSampleContext::new(Timestamp::from_millis(millis), 0, 1);
            assert_eq!(
                store.sample(spread, context, &sequences),
                store.sample(step, Timestamp::from_millis(millis), &sequences),
                "at {millis}ms"
            );
        }
    }

    /// An empty group (`fixture_count: 0`) never divides by zero — same
    /// defensive policy as spread over an oscillator.
    #[test]
    fn spread_on_step_with_zero_fixtures_does_not_panic() {
        let (sequences, sequence) = palette();
        let mut store = SignalStore::new();
        let step = store.insert(SignalKind::Step {
            sequence,
            every: Duration::from_millis(500),
            started_at: Timestamp::ZERO,
        });
        let spread = store.insert(SignalKind::Spread {
            source: step,
            amount_millideg: 360_000,
        });
        let context = SignalSampleContext::new(Timestamp::ZERO, 0, 0);
        assert_eq!(
            store.sample(spread, context, &sequences),
            store.sample(step, Timestamp::ZERO, &sequences),
        );
    }
}
