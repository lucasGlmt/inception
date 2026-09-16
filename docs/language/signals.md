# `Signal<T>`

```lux
import std.Signal;
```

A `Signal<T>` is a value that depends on time. Conceptually:

```text
Signal<T> + Timestamp -> T
```

Give a signal a specific point in time (a timestamp), and it produces a
`T`. Give it the same timestamp again, and it produces exactly the same
`T` — a signal is a pure function of time, not something that changes on
its own as a program runs.

## `Signal<T>` vs. `T`

An ordinary value like `Intensity` or `Color` is immediate: it just *is*
whatever it is, right now. A `Signal<Intensity>` is different — it's a
*description* of an intensity that can depend on when you ask. This
milestone only ships one kind of signal, `Signal.constant`, which doesn't
actually depend on time at all (every timestamp gives the same answer) —
but the type itself is what matters here: `Signal<Intensity>` and
`Intensity` are two different types, and Lux never converts between them
implicitly:

```lux
import std.Signal;

scene main {
    let level: Signal<Intensity> = Signal.constant(50%);

    // Every one of these is a compile-time error:
    // let x: Intensity = level;               // no implicit unwrap
    // let s: Signal<Intensity> = 50%;          // no implicit wrap
    // let bad: Signal<Color> = level;          // wrong element type
}
```

`Signal<T>` is written the same way for any of the types it can wrap:

```lux
let a: Signal<Intensity> = Signal.constant(50%);
let b: Signal<Color> = Signal.constant(red);
let c: Signal<Float> = Signal.constant(1.5);
let d: Signal<Angle> = Signal.constant(90deg);
```

(V1 supports `Signal<Int>`, `Signal<Float>`, `Signal<Angle>`,
`Signal<Intensity>` and `Signal<Color>` — the same 5 types the rest of
the standard library already works with.)

## Absolute-time sampling

A signal is always evaluated against an absolute timestamp, never a
frame count or a running total of elapsed ticks:

```text
signal.sample(t = 1s)  ->  some value
```

Sampling the same signal at the same timestamp — once, or a hundred
times, in any order — always produces the same result. This is what
makes signals safe to preview, test, and reason about without running a
show in real time: a `VirtualClock` can jump straight to `t = 3600s` and
get exactly the same answer a real clock would give after an hour.

Sampling is not yet exposed as a method you can call from Lux source
(`signal.sample(...)` isn't valid syntax in this milestone) — it exists
only as an internal runtime/testing capability, kept minimal on purpose
so it doesn't have to be redesigned once a real way to *use* a signal
exists.

## `Signal.constant`

The only way to build a signal in this milestone:

```lux
import std.Signal;

scene main {
    let level: Signal<Intensity> = Signal.constant(50%);
}
```

`Signal.constant(value)` returns a signal that always evaluates to
`value`, no matter the timestamp. Its return type follows its argument's
type: `Signal.constant(50%)` is `Signal<Intensity>`, `Signal.constant(red)`
is `Signal<Color>`, and so on.

## Binding a signal to an attribute: `<-`

A signal only matters once it drives something. `<-` continuously binds
an attribute to a signal:

```lux
import std.Signal;

scene main {
    let level = Signal.constant(50%);

    Front.intensity <- level;
}
```

From that point on, `Front.intensity` is *continuously derived* from
`level`: at every timestamp the lighting engine renders, it re-samples
the signal and uses whatever it produces —

```text
signal.sample(now) -> AttributeValue -> LightingState -> Renderer
```

— rather than copying the signal's value once and forgetting about the
signal. The binding stays in effect indefinitely: it does not "finish"
the way a transition does, and it survives the scene finishing (a
binding is not tied to any VM instruction still being "in flight" — see
`docs/rfcs/0002-signal-type.md`'s runtime section).

### Three distinct operators

Lux now has three ways to make `Front.intensity` take on a value, and
they mean three different things:

| Syntax | Meaning |
| --- | --- |
| `Front.intensity = 50%;` | **Immediate**: set the value right now, once. |
| `Front.intensity -> 100% over 2s;` | **Transition**: interpolate from the current value to `100%` over 2 seconds, then stop. |
| `Front.intensity <- level;` | **Signal binding**: continuously derive the value from `level`, forever (until replaced or detached). |

`<-` only accepts a `Signal<T>` matching the attribute's type — never a
direct value:

```lux
import std.Signal;

scene main {
    let level: Signal<Intensity> = Signal.constant(50%);

    Front.intensity <- level;             // ok
    // Front.intensity <- 50%;            // error: expected Signal<Intensity>, found Intensity
    // Front.intensity <- Signal.constant(red); // error: expected Signal<Intensity>, found Signal<Color>
    Front.intensity <- Signal.constant(50%); // also ok — the signal can be built inline
}
```

There is no implicit wrapping: if you want a binding, you write
`Signal.constant(...)` (or any future signal constructor) explicitly.

### Lifecycle: who controls an attribute

At most one thing controls a given `(fixture, attribute)` pair at a
time — a plain value, a running transition, or a signal binding, never
more than one simultaneously. Whichever of `=`, `->` or `<-` runs last
wins, and it always **replaces** whatever was controlling the attribute
before:

- **`<-` replaces `<-`**: binding a new signal instantly (no crossfade)
  swaps out any previously bound signal on that attribute.
- **`=` detaches `<-`**: an immediate assignment removes any active
  signal binding first, then sets the value. The signal does not regain
  control on a later frame.
- **`->` detaches `<-`**: starting a transition samples the currently
  bound signal *at that instant*, uses that as the transition's starting
  value, removes the binding, and proceeds as an ordinary transition from
  there. The transition never starts from a stale `LightingState` value.
- **`<-` detaches `->`**: binding a signal onto a fixture with an active
  transition samples the transition at that instant (so nothing is lost
  from the bookkeeping), removes it, and installs the binding. The
  signal's own sampled value then takes over immediately — there is no
  automatic crossfade between the old transition and the new signal.

```lux
import std.Signal;

scene main {
    let dimmed = Signal.constant(20%);

    Front.intensity <- dimmed;      // 0s: Front is signal-controlled, at 20%

    wait 2s;

    Front.intensity -> 100% over 1s; // 2s: signal detaches, transition starts from 20%
                                      // 3s+: stable at 100%
}
```

A binding, once installed, is destroyed only when: it's replaced by
another `<-`, `=` or `->` on the same attribute; the program is hot
reloaded (the old bindings are sampled one last time, then dropped —
same as transitions); or the runtime itself shuts down. There is no
other garbage collection.

## What's not here yet

This milestone wires signals into a running show, but keeps the signal
vocabulary itself minimal:

- There are no time-varying signal kinds — no `sine`, `triangle`, `saw`,
  or any oscillator. `Signal.constant` is deliberately the only
  constructor, so this milestone can validate the binding architecture
  before anything more interesting is layered on top. Adding, say,
  `SignalKind::Sine` later should not require any change to `<-` itself
  or to how bindings are sampled — only a new way to *construct* a
  signal.
- There's no way to combine or transform signals (no `map`, no `.range()`,
  no `.phase()`, no arithmetic on a `Signal<T>`).
- There's no per-fixture "spread": every fixture a binding reaches
  currently renders the exact same sampled value.
- `std.Effects` does not exist yet.

See `docs/rfcs/0002-signal-type.md` for the full design rationale.
