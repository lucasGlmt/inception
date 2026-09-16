# `std.Effects`

```lux
import std.Effects;
```

Four time-varying oscillators, each returning `Signal<Float>` normalized
to `0.0..1.0`. Unlike `std.Math`/`std.Color`/`std.Signal.constant`,
these are **not** pure functions of their arguments alone: each call
captures the current runtime clock as the oscillator's time origin, so
two calls to the exact same expression at different times produce
signals that sample differently. See `docs/language/signals.md` for the
conceptual introduction to `Signal<T>` and
`docs/rfcs/0003-signal-binding-operator.md`/`docs/rfcs/0002-signal-type.md`
for the surrounding design.

Once created, an oscillator behaves exactly like any other `Signal<T>`:
it is a pure function of `(kind, timestamp)` — `signal.sample(now)`
always returns the same value for the same `now`, and sampling never
depends on how many times, or in what order, the signal was sampled
before. Only *construction* reads the clock.

## Common shape

```lux
sine(period: Duration) -> Signal<Float>
triangle(period: Duration) -> Signal<Float>
saw(period: Duration) -> Signal<Float>
square(period: Duration) -> Signal<Float>
```

`period` must be strictly greater than zero — a literal `0s` is a
compile-time error, and a non-constant `Duration` that turns out to be
zero at construction time is a structured runtime error (never a panic
or a division by zero).

An oscillator's cycle starts (`phase == 0`) at the timestamp it was
constructed at — the moment the VM executes the `Effects.*` call, using
the runtime clock at that instant. Two calls to `Effects.sine(2s)`
separated by a `wait` get independent origins:

```lux
import std.Effects;

scene main {
    let a = Effects.sine(2s); // origin: t=0

    wait 1s;

    let b = Effects.sine(2s); // origin: t=1s — a different cycle
}
```

## `sine`

```lux
Effects.sine(2s)
```

| phase | value |
| ----- | ----- |
| 0.00  | 0.5   |
| 0.25  | 1.0   |
| 0.50  | 0.5   |
| 0.75  | 0.0   |

Computed as `0.5 + 0.5 * sin(2π * phase)`.

## `triangle`

```lux
Effects.triangle(2s)
```

| phase | value |
| ----- | ----- |
| 0.00  | 0.0   |
| 0.25  | 0.5   |
| 0.50  | 1.0   |
| 0.75  | 0.5   |

Computed as `1 - |2 * phase - 1|`.

## `saw`

```lux
Effects.saw(2s)
```

| phase | value |
| ----- | ----- |
| 0.00  | 0.0   |
| 0.25  | 0.25  |
| 0.50  | 0.5   |
| 0.75  | 0.75  |

A rising sawtooth: `value = phase` directly, ramping linearly up to just
under `1.0` before snapping back to `0.0` at the start of the next
period. There is no falling/descending variant in V1.

## `square`

```lux
Effects.square(2s)
```

| phase        | value |
| ------------ | ----- |
| `< 0.50`     | 1.0   |
| `>= 0.50`    | 0.0   |

A fixed 50% duty cycle: `1.0` for the first half of each period, `0.0`
for the second half.

## Phase

Every oscillator is driven by the same normalized `phase`, derived
purely from `period`, the oscillator's origin (`started_at`), and the
timestamp it's sampled at (`now`):

```text
elapsed = now - started_at   (clamped to zero if now < started_at)
phase   = (elapsed mod period) / period      ∈ [0, 1)
```

This is recomputed from scratch on every sample — never accumulated
frame-over-frame — so a missed frame (e.g. jumping straight from `t=0`
to `t=1750ms`) always produces the mathematically correct value, with no
drift and no dependency on how many previous samples happened.

## What's not here yet

- `.range(min, max)`, to remap `Signal<Float>` into `Signal<Intensity>`
  (or any other element type) — without it, an oscillator's `Signal<Float>`
  **cannot** be bound directly to a lighting attribute:

  ```lux
  import std.Effects;

  scene main {
      // error: expected Signal<Intensity>, found Signal<Float>
      Front.intensity <- Effects.sine(2s);
  }
  ```

  This is deliberate: `.range()` is how a future milestone will make
  that binding possible, and skipping it now keeps that transformation
  from being silently baked into `<-` itself.
- `.phase(offset)`, `.speed(factor)`, or any other transformation.
- Signal arithmetic (`wave * 2.0`, `wave + 0.5`) or composition (`map`,
  blending two signals).
- Per-fixture spread — not applicable yet, since no oscillator can bind
  to an attribute at all without `.range()`.
- Tempo/BPM-relative periods, easing curves, random/noise signals.
