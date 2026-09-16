# RFC 0004: `std.Effects` — the four base oscillators

Status: implemented (V1 — `sine`/`triangle`/`saw`/`square`, no
transformations).

## Summary

Adds `std.Effects`, a stdlib module of four time-varying signal
constructors — `sine`, `triangle`, `saw`, `square` — each with signature
`(period: Duration) -> Signal<Float>`, producing a value normalized to
`0.0..1.0` that depends only on absolute time:

```lux
import std.Effects;

scene main {
    let wave: Signal<Float> = Effects.sine(2s);
}
```

This is the first *time-varying* `SignalKind` — RFC 0002's `Signal<T>`
and RFC 0003's `<-` binding operator both only ever dealt with
`Constant`, a signal whose value never changes. `std.Effects` validates
that the architecture built for those two milestones already supports a
real oscillator with zero changes to either — see "Architectural
impact" below.

## Design

### Why `Signal<Float>`, not `Signal<Intensity>`

Every oscillator returns `Signal<Float>`, never a lighting-domain type
directly. This means `Front.intensity <- Effects.sine(2s);` is a
compile-time error today (`expected Signal<Intensity>, found
Signal<Float>`) — deliberately: a future `.range(min, max)` will be the
one way to turn a `Signal<Float>` into a `Signal<Intensity>`/
`Signal<Color>`/etc. Building that remapping into `Effects.sine` itself
now would mean every future signal transformation needs its own
special-cased "and also produce an attribute-ready version" variant.
Keeping oscillators strictly `Signal<Float>`-typed keeps `.range()` (and
`.phase()`, `.speed()`, arithmetic) as pure post-hoc transformations over
an already-simple value.

### Phase: the one shared computation

All four waveforms are expressed as a function of a single normalized
`phase ∈ [0, 1)`, computed once (`inception_vm::signal::phase_at`) and
reused by all four:

```text
elapsed = now.as_nanos().saturating_sub(started_at.as_nanos())
phase   = (elapsed % period.as_nanos()) as f64 / period.as_nanos() as f64
```

`saturating_sub` (not plain subtraction) implements the "timestamps
before `started_at`" policy: `elapsed` clamps to zero rather than
underflowing a `u64`, so a sample taken before an oscillator's origin
reads phase `0` instead of panicking or wrapping to a huge value.
`elapsed % period` is exact `u64` integer arithmetic; only the final
division to produce the `0.0..1.0` fraction touches floating point. This
is what keeps the phase numerically stable over arbitrarily long
durations (a 1-hour period sampled after a virtual hour is exactly as
precise as one sampled a second in) — the value is always recomputed
from `elapsed`/`period` directly, never accumulated sample-over-sample.

Each waveform is then a one-line function of `phase`:

| Waveform   | Formula                        |
|------------|---------------------------------|
| `sine`     | `0.5 + 0.5 * sin(2π * phase)`  |
| `triangle` | `1 - |2 * phase - 1|`          |
| `saw`      | `phase`                        |
| `square`   | `phase < 0.5 ? 1.0 : 0.0`      |

`sine`/`triangle` are explicitly clamped to `[0.0, 1.0]` after
computing, as a defensive measure against floating-point edge cases
right at a boundary (item 15 of the task brief); `saw`/`square` are
bounded by construction and don't need it.

### `started_at`: the one place construction may read the clock

Every other operation on a `Signal<T>` (including sampling an
oscillator) is a pure function of its explicit inputs — no clock, no
`Vm`-owned mutable state beyond the append-only `SignalStore` itself.
Oscillator *construction* is the deliberate, narrow exception: building
`SignalKind::Sine { period, started_at }` needs `started_at`, and the
task brief requires that to be "the timestamp this signal was created
at" — which only the executing `Vm` knows, via `clock.now()`. This is
threaded through by giving `Vm::exec_call_intrinsic` (which already
special-cases the 5 `SignalConstant*` intrinsics for the same "needs
`Vm`-owned state" reason) a `&C: Clock` parameter, used only by the 4
new `Effects*` branches.

### Validating `period`

Two layers, matching the task brief's recommendation:

- **Compile time**: `lux-typeck`'s `check_literal_constant_misuse` (the
  same mechanism that already flags `Math.clamp(v, 5, 2)` and an
  out-of-range `Color.rgb` channel) rejects a *literal* `0s` argument
  immediately, with `oscillator period must be greater than zero`.
- **Runtime**: `Vm::exec_call_intrinsic`'s `Effects*` branch checks the
  popped `Duration` regardless — covering a non-constant period (a
  `let`-bound variable) that only turns out to be zero when the
  oscillator is actually constructed — and returns
  `VmErrorKind::InvalidSignalPeriod` instead of ever calling
  `phase_at` with a zero divisor.

Negative periods need no separate handling: `Duration` cannot be
negative in this language at all (`lux_typeck::rules::unary_result_type`
never defines `Neg` for `Duration`, and `Duration::checked_sub` already
returns a structured error rather than wrapping), so this is an existing
invariant, not new work.

### stdlib integration: no special-casing anywhere

`std.Effects` is registered exactly like `std.Math`/`std.Color`/
`std.Signal`: four `Signature`s in `lux-stdlib`'s registry, dispatched
purely by `IntrinsicId` (`EffectsSine`/`EffectsTriangle`/`EffectsSaw`/
`EffectsSquare`, mirrored independently in `lux-bytecode` per this
workspace's frontend/runtime duplication idiom). `import std.Effects;`,
member completion, overload resolution, and argument type-checking are
all already generic over `lux_stdlib::STD_MODULES` — adding the module
to that one list is what makes every one of those paths work, with zero
changes to the parser, `lux-hir`'s import resolution, or `lux-lsp`'s
completion logic. The one new piece of shared infrastructure is
`lux_stdlib::ParamType::Duration` (and its `lux_typeck::Type::Duration`
round trip): no stdlib function needed a `Duration` parameter before
`Effects.sine`, only a `Duration` *return* path existed (for `wait`).

Unlike every other signature in the registry so far, the four `Effects*`
signatures are marked `pure: false` — `Signature::pure`'s own doc
comment already anticipated this ("a future, deliberately-impure module
doesn't have to retrofit this field"): reading the clock at construction
is exactly the kind of impurity that field exists to flag.

### Architectural impact: zero changes to `<-`, `SignalBindingStore`, or `LightingState`

This is the RFC's main validation target (task brief items 68/79). Grep
for every file this milestone touches outside `lux-stdlib`/`lux-typeck`/
`lux-bytecode`/`lux-mir`/`lux-lsp`/tests: `inception-vm/src/signal.rs`
(new `SignalKind` variants + `phase_at`/waveform functions) and
`inception-vm/src/vm.rs` (`exec_call_intrinsic`'s new branch, plus
threading `clock` through it) are the *only* runtime files that changed.
`inception-vm/src/binding.rs` (`SignalBindingStore`, from RFC 0003) is
untouched — it only ever calls `SignalStore::sample(id, now)`, never
matches on `SignalKind`, so it already knew how to sample a `Sine`
signal the moment this milestone's `SignalKind` variants existed.
`inception_core::LightingState`/`TransitionEngine` and
`inception_renderer` are equally untouched, and can't even see a
`Signal` type at all. This confirms the layering RFC 0003 set up:

```text
Signals (SignalStore: Constant | Sine | Triangle | Saw | Square)
    v
SignalBindingStore  (signal-kind-agnostic)
    v
LightingState       (signal-agnostic)
    v
Renderer             (signal-agnostic)
```

A future `SignalKind::Range { source: SignalId, min, max }` (or
`.phase()`/`.speed()`/a full node graph) is a new `SignalStore`/
`SignalKind` case and a new `sample` match arm — again, not a change to
`SignalBindingStore`, `<-`, or anything below it.

## Non-goals (do not implement against this RFC)

Carried over unchanged from the task brief:

- `.range()`, `.phase()`, `.speed()`, or any other signal transformation
  — this is exactly what keeps `Signal<Float>` from binding to a
  lighting attribute today, and that's intentional.
- Signal composition, arithmetic (`wave * 2.0`), or a node graph.
- Per-fixture spread — not reachable yet anyway, since no oscillator can
  bind to an attribute without `.range()`.
- Tempo/BPM-relative periods, beat sync.
- Easing curves.
- Random/noise signal kinds.
- Named call arguments (`Effects.sine(period: 2s)`) — Lux has no named
  argument syntax yet, and this milestone doesn't add one just for this;
  calls stay purely positional (`Effects.sine(2s)`), like every other
  stdlib call in the language.

## Testing

Same pyramid as RFC 0002/0003: `lux-stdlib` (registry shape, overload
resolution, the deliberate `pure: false` on all four signatures),
`lux-typeck` (`Signal<Float>` inference, explicit/mismatched annotations,
non-`Duration` argument diagnostics for all four oscillators, literal-zero
rejection for all four, the `Signal<Float>` → `Intensity`/`Color`
binding rejection), `lux-bytecode` (verifier acceptance via the generic
`CallIntrinsic` operand-type check — no oscillator-specific verifier code
needed), `inception-vm` (`phase_at`/waveform unit tests against the
documented phase tables, short/long periods, non-zero `started_at`,
skipped-frame and non-monotonic sampling, repeated-sampling determinism,
output-bounds sweep, `VirtualClock`-driven construction-time-origin
tests, the runtime `InvalidSignalPeriod` backstop, and one test per
oscillator through a real `Vm`), `inception-vm/tests/e2e_lux.rs` (the
full compiler pipeline producing the exact documented `sine` phase
table, and a `wait`-then-construct test proving the origin is the
construction timestamp, not the program's start), and `lux-lsp`
(`std.Effects` import completion, member completion listing all four,
signature help, hover, and immediate wrong-argument-type diagnostics).
Compile-pass/compile-fail `.lux` fixtures under `tests/compiler/{pass,fail}`
cover the end-to-end accept/reject surface, including the
`Signal<Float>` → attribute rejection.
