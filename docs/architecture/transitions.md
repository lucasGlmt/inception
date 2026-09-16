# Transition model

Lux V1 supports non-blocking linear intensity transitions:

```lux
Washes.intensity -> 100% over 2s;
```

## Absolute-time semantics

Each resolved fixture gets an independent `ActiveTransition` containing its
current value, target value, start timestamp, and end timestamp. Sampling at
time `T` computes the value directly from those timestamps. It never advances a
tick counter and never sleeps.

**Transitions are time-based, not frame-based. Missing a render frame must
never cause timing drift.**

The VM's `TRANSITION_ATTRIBUTE` instruction pops the duration first and then
the target value, reads the injected monotonic clock once, asks the transition
engine to start the command, and continues immediately. A transition does not
block the executing scene.

## State ownership

`LightingState` stores the most recently resolved semantic fixture values.
`TransitionEngine` stores active temporal overlays keyed by numeric
`(FixtureId, Attribute)` pairs. Before rendering, the runtime samples the
engine into `LightingState`; the renderer remains unaware of transitions and
only consumes effective semantic values.

When a transition reaches or passes its end timestamp, sampling writes the
exact target value to `LightingState` and removes the active entry. A zero
duration immediately writes the target and creates no active entry.

## Interpolation and replacement

Intensity interpolation uses integer arithmetic with `u64` intermediates and
round-to-nearest:

```text
travelled = round(abs(to - from) * elapsed / duration)
value = from +/- travelled
```

At or before the start the value is exactly `from`; at or after the end it is
exactly `to`.

Starting another transition on the same `(fixture, attribute)` first samples
the previous transition at the new command's timestamp. That effective value
becomes the new `from`, and the old transition is replaced. This happens per
fixture, so fixtures in one target may start from different values.

## V1 limits

Only `Intensity` transitions and linear interpolation are supported. Color,
easing curves, blending, priorities, scene ownership, timelines, tempo, and
parallel execution are intentionally outside this milestone.
