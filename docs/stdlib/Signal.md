# `std.Signal`

```lux
import std.Signal;
```

Time-sampled values: a `Signal<T>` evaluates to a `T` at a given
timestamp, rather than being an immediate `T` itself. See
`docs/language/signals.md` for the conceptual introduction to `Signal<T>`
and `docs/rfcs/0002-signal-type.md` for the full design.

**`constant` is pure**, in the same sense `std.Math`/`std.Color`'s
functions are: no clock, IO, randomness, DMX or mutable global state.
The *signal it produces* is also pure — sampling it at the same
timestamp always returns the same value, and at V1 it returns the same
value for every timestamp, since `Constant` is the only signal kind that
exists yet.

## `constant`

```lux
constant(value: Int) -> Signal<Int>
constant(value: Float) -> Signal<Float>
constant(value: Angle) -> Signal<Angle>
constant(value: Intensity) -> Signal<Intensity>
constant(value: Color) -> Signal<Color>
```

Creates a signal that always evaluates to `value`, for any timestamp.
Five overloads, one per element type it can wrap — resolved by argument
type, exactly like `Math.abs`'s `Int`/`Float` overloads. The return
type's element always matches the argument's type: `Signal.constant(50%)`
is `Signal<Intensity>`, `Signal.constant(red)` is `Signal<Color>`.

- `Signal.constant(50%)` sampled at `t = 0`, `t = 1ms`, `t = 1s` or
  `t = 1h` always evaluates to `50%`.
- `Signal.constant(red)` sampled at any timestamp always evaluates to
  `red`.
- Two separate calls, `Signal.constant(50%)` and `Signal.constant(50%)`,
  produce two independent signals — V1 makes no identity/deduplication
  guarantee between them, only that each one individually samples
  deterministically.

There is no implicit conversion in either direction between `T` and
`Signal<T>`:

```lux
import std.Signal;

scene main {
    let a: Signal<Intensity> = Signal.constant(50%); // ok
    // let b: Intensity = Signal.constant(50%);      // error: no implicit unwrap
    // let c: Signal<Intensity> = 50%;                // error: no implicit wrap
}
```
