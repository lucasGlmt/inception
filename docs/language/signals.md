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

## Time-varying signals: `std.Effects`

`Signal.constant` isn't the only way to build a signal anymore:
`std.Effects` adds four oscillators — `sine`, `triangle`, `saw`,
`square` — each `(period: Duration) -> Signal<Float>`. See
`docs/stdlib/Effects.md` for the full reference and phase tables.

```lux
import std.Effects;

scene main {
    let wave: Signal<Float> = Effects.sine(2s);
}
```

Notably, `<-` needed **no changes at all** to support this: it was
already written against `SignalId`/`sample(now)` alone, never against
`Signal.constant` specifically, so a new signal kind is purely additive
— see `docs/rfcs/0004-effects-oscillators.md`'s "architectural impact"
section.

Every oscillator returns `Signal<Float>`, not `Signal<Intensity>` or any
other lighting-domain type — so this, on its own, remains invalid:

```lux
Front.intensity <- Effects.sine(2s); // error: expected Signal<Intensity>, found Signal<Float>
```

`.range()` (below) is how a `Signal<Float>` becomes attribute-ready.

## Signal composition: `.range()`, `.phase()`, `.spread()`, `.invert()`

A `Signal<Float>` — from an oscillator, `Signal.constant`, or another
composition — can be transformed with method-call syntax, chaining
freely:

```lux
import std.Effects;

scene main {
    let breathe =
        Effects.sine(2s)
            .phase(90deg)
            .range(5%, 100%);

    Front.intensity <- breathe;
}
```

Each transformation **creates a new signal**; it never mutates its
source. `let b = a.phase(90deg);` leaves `a` exactly as it was — `b`
just references `a` by its `SignalId`, the same "graph of nodes, not a
copy of definitions" approach the runtime already used internally.
Reading the chain top to bottom, the type changes at each step:

```text
Effects.sine(2s)          Signal<Float>
    .phase(90deg)      -> Signal<Float>
    .range(5%, 100%)   -> Signal<Intensity>
```

### `.range(min, max)`

```lux
Effects.sine(2s).range(0%, 100%)   // Signal<Intensity>
Effects.sine(2s).range(0deg, 180deg) // Signal<Angle>
Effects.sine(2s).range(0.0, 10.0)    // Signal<Float>
```

Only defined on `Signal<Float>`. Remaps the source's value — clamped to
`0.0..1.0` first, so an already-out-of-range source (e.g. after
`.invert()`) never extrapolates past `min`/`max` — to
`min + x * (max - min)`, computed in `min`/`max`'s own native
representation. `min` and `max` must be the same type:

```lux
Effects.sine(2s).range(0%, 180deg); // error: range bounds must have the same type
```

`min` may be greater than `max` — that's a valid, deliberate way to
reverse the ramp (`range(100%, 0%)` falls from `100%` to `0%` as the
source rises from `0.0` to `1.0`), not an error.

`Color` isn't a supported `range` target yet: RGB/HSV/linear-light
interpolation each have different, non-obvious meanings, and picking one
prematurely would be hard to walk back.

### `.phase(offset)`

```lux
Effects.sine(2s).phase(90deg) // Signal<Float>
```

Shifts the signal's cycle by `offset / 360deg` of a period —
`phase(90deg)` is a quarter-cycle ahead of the unshifted signal, and
`phase(450deg)` behaves exactly like `phase(90deg)` (multi-turn offsets
wrap automatically).

Only valid **directly on an `Effects` oscillator** (`sine`/`triangle`/
`saw`/`square`), or on another `.phase(...)` call chained from one:

```lux
let a = Effects.sine(2s).phase(90deg).phase(90deg); // ok — same as phase(180deg)

let b = Effects.sine(2s).range(0%, 100%).phase(90deg); // error
```

This is a deliberate V1 restriction, not an oversight: shifting an
arbitrary signal's "phase" would require knowing its underlying period,
which only a base oscillator actually has — see
`docs/rfcs/0004-effects-oscillators.md` for the full rationale.

### `.spread(amount)`

Every signal so far has been the same for every fixture a binding
reaches: `Front.intensity <- Effects.sine(2s).range(0%, 100%);` makes
every fixture in `Front` breathe in perfect unison. `.spread(amount)`
breaks that unison, distributing `amount` as a phase offset across the
fixtures of whatever group the signal ends up bound to — fixture `i` of
`n` total gets `amount * i / n` added to its own phase:

```lux
import std.Effects;

scene main {
    Front.intensity <-
        Effects.sine(2s)
            .spread(360deg)
            .range(5%, 100%);
}
```

With 4 fixtures in `Front`, this renders a genuine wave sweeping across
the group — not four fixtures blinking together:

```text
4 fixtures, spread(360deg)

[  0°] [ 90°] [180°] [270°]
  f0     f1     f2     f3
```

Each bracket is one fixture's own phase offset into the *same* `2s` sine
cycle — at any instant, fixture 1 is a quarter-cycle ahead of fixture 0,
fixture 2 a quarter ahead of fixture 1, and so on, which is what reads as
motion across the fixture line rather than a shared pulse.

#### The formula, precisely

```text
offset = amount * fixture_index / fixture_count
```

**Deliberately `fixture_count`, never `fixture_count - 1`**: with 4
fixtures and a full `360deg` spread, the table above is `0°, 90°, 180°,
270°` — fixture 0 and fixture 3 end up **90° apart**, not back in phase.
Dividing by `fixture_count - 1` instead would put the first and last
fixture at the exact same phase for a full-turn spread, collapsing the
wave's seam back onto itself; `/ fixture_count` never does.

A single-fixture target (`fixture_count == 1`) always gets `offset ==
0` — `.spread()` is a no-op outside a real multi-fixture binding, not a
division by zero.

#### Fixture order is the rig's order

`fixture_index` is a fixture's position within the target's fixture list
as the **rig binding** wrote it — the same order `role Front: ...`'s
binding in the rig config lists `Front`'s fixtures, resolved once at link
time into a plain, ordered list. It is never a `HashMap`'s iteration
order and never changes on its own: the same rig always spreads a show
the same way, and re-ordering the fixtures in the rig binding is how you
re-order (or reverse) the spread — there is no separate in-language way
to do that in V1 (see "Not here yet" below).

#### Composing with `.phase()` and `.range()`

```lux
Effects.sine(2s)
    .phase(45deg)   // every fixture's phase, shifted 45° ahead
    .spread(360deg) // ...then each fixture's own spread offset on top
    .range(5%, 100%)
```

The static `.phase()` offset and each fixture's own `.spread()` offset
add together — a fixture's total phase shift is `45deg + amount * i / n`.
`.spread()` must come directly after an `Effects` oscillator, or after a
single `.phase(...)` chained from one — the same restriction `.phase()`
itself has, and for the same reason (only a base oscillator has a period
to shift). `.spread()` isn't itself chainable this way:
`.spread(...).spread(...)` and `.spread(...).phase(...)` are both
compile-time errors.

`.range()` is unaffected by where `.spread()` sits relative to it —
`.spread()` only ever changes *which phase* of the source oscillator a
fixture reads, never the sampled value itself, so `.range()` before or
after `.spread()` both type-check and both reach the fixture correctly;
writing `.spread()` before `.range()` (as in every example above) is the
natural reading order, since `.range()` is what converts the signal to
the attribute's own type at the very end of the chain.

#### How this works underneath: `SignalSampleContext`

Every signal now samples against a small context, not a bare timestamp:

```rust
pub struct SignalSampleContext {
    pub now: Timestamp,
    pub fixture_index: usize,
    pub fixture_count: usize,
}
```

Every signal kind other than `.spread()` ignores `fixture_index`/
`fixture_count` entirely — a plain oscillator, `.range()`, `.phase()` and
`.invert()` behave *exactly* as before this existed, sampling identically
regardless of which fixture (or none at all — a plain `Timestamp` still
works everywhere a context is expected, converting automatically) is
asking. The binding engine samples the *same* signal definition once per
fixture in the target, each time with that fixture's own
`fixture_index`/`fixture_count` — never a distinct signal per fixture —
which is what lets one `Effects.sine(2s).spread(360deg)` expression
render a different phase on each of `Front`'s fixtures.

### `.invert()`

```lux
Effects.sine(2s).invert() // Signal<Float>
```

`1.0 - source`, sampled fresh every time — not clamped, so inverting a
signal outside `0.0..1.0` (e.g. after `.range(10.0, 20.0)`) produces a
value outside `0.0..1.0` too.

## What's not here yet

- No signal arithmetic (`wave * 2.0`, `wave + otherWave`) and no
  composition beyond `.range()`/`.phase()`/`.spread()`/`.invert()` (no
  `map`, no combinators like `zip`/`combine`/`fold`).
- `.spread()` is index-based only: it distributes phase by a fixture's
  plain position in the rig-resolved list, nothing else. No 2D/3D
  physical-position spread, no coordinate-based selection, no reverse/
  custom group-ordering DSL (re-order the rig binding itself instead), no
  random spread, no tempo/BPM sync.
- No tempo/BPM-relative periods, no easing curves, no random/noise
  signal kinds.
- Named call arguments (`Effects.sine(period: 2s)`) don't exist — every
  call, including method calls, stays positional.

See `docs/rfcs/0002-signal-type.md` for the full design rationale.
