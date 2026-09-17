# RFC 0006: per-fixture spread — `.spread()` and `SignalSampleContext`

Status: implemented (V1 — index-based spread only, no physical position,
no reverse/custom ordering DSL, no random spread).

## Summary

Adds a fourth builtin `Signal<Float>` transformation, `.spread(amount)`,
and the concept that makes it possible: a **sampling context**, not just
a timestamp, threaded through `SignalStore::sample`. Before this RFC,
every fixture a `<-` binding reached rendered the exact same sampled
value; `.spread()` is what turns one signal definition into a genuine
spatial wave across a fixture group:

```lux
import std.Effects;

scene main {
    Front.intensity <-
        Effects.sine(2s)
            .spread(360deg)
            .range(5%, 100%);
}
```

With 4 fixtures in `Front`, fixture `i` reads the sine `amount * i / 4`
ahead of fixture `0` — `0°, 90°, 180°, 270°` for `amount = 360deg` — and
the same show works unmodified for 2, 8, or `N` fixtures.

## Design

### The sampling context: `SignalSampleContext`

`SignalStore::sample` used to take a bare `Timestamp`. It now takes:

```rust
pub struct SignalSampleContext {
    pub now: Timestamp,
    pub fixture_index: usize,
    pub fixture_count: usize,
}
```

`sample` accepts `impl Into<SignalSampleContext>`, and `Timestamp`
implements `From<Timestamp> for SignalSampleContext` as `fixture_index:
0, fixture_count: 1`. This is the crux of "existing signals must keep
their exact behavior" (item 1 of the task brief): every call site that
only ever had a `Timestamp` — every unit test, every non-spread binding,
every preview — keeps compiling and behaving identically with zero
changes, because `fixture_index`/`fixture_count` are inert for every
`SignalKind` except `Spread` (`0 * amount / 1 == 0`, always). Only
`inception_vm::binding::SignalBindingStore` and `Vm::exec_transition_attribute`
(which resamples a binding once, on detach) construct a real,
non-default context — see "the binding engine" below.

### `SignalKind::Spread`

```rust
Spread { source: SignalId, amount_millideg: i32 }
```

Sampling computes:

```text
offset = amount * fixture_index / fixture_count        (integer arithmetic)
value  = waveform((phase_at(now, started_at, period) + (static_offset + offset) / 360deg) mod 1)
```

`static_offset` comes from `source` if it's a `Phase` node (see
"composing with `.phase()`" below), or `0` if `source` is a base
oscillator directly. `fixture_count == 0` short-circuits to `offset ==
0` rather than dividing by zero — unreachable from a real binding (the
linker's `bind_role` already rejects an empty role binding as
`LinkError::EmptyBinding`, so the runtime never actually sees
`fixture_count: 0` from a `<-`), but `sample` never trusts that blindly,
matching every other defensive check in this crate.

**`fixture_index / fixture_count`, never `fixture_index / (fixture_count
- 1)`.** The task brief is explicit about this (item 3): with 4 fixtures
and a full `360deg` spread, dividing by `fixture_count - 1` would put
fixture `0` and fixture `3` at the exact same phase, collapsing the
wave's seam back onto itself. Dividing by `fixture_count` keeps them 90°
apart, which is what a *continuous* wave across the group actually looks
like.

### `.spread()`'s receiver restriction — reusing `.phase()`'s exactly

`.spread()` needs to resolve a period to shift, exactly like `.phase()`
does, for exactly the same reason (see RFC 0005's "`.phase()`: restricted
to base oscillators, on purpose"). Rather than invent a second
restriction, `lux-typeck`'s `check_method_call` reuses `.phase()`'s own
`is_phase_source_valid` predicate for `.spread()` too: valid directly on
an `Effects` oscillator, or on a `.phase(...)` call chained from one.

`.spread()` is deliberately **not** itself part of that recursion:
`.spread(...).spread(...)` and `.spread(...).phase(...)` are both
compile-time errors. This isn't a fundamental limitation — the offset
math composes linearly, so a flattening scheme analogous to `.phase()`'s
own chain-flattening could support it — but V1 has no real use case for
stacking two spreads or phase-shifting one, and the task brief scopes
spread to "index-based only." `SignalStore::oscillator_basis` (used only
by `Spread::sample`) mirrors this exactly at the runtime level: it
resolves a `source` that's either a base oscillator directly or a
`Phase` wrapping one, and returns `None` (surfaced as the structured
`SignalError::UnsupportedSpreadSource`) for anything else — a `Spread`
can never legitimately wrap another `Spread`, `Range`, or `Invert`.

### Composing with `.phase()`

```lux
Effects.sine(2s).phase(45deg).spread(360deg)
```

`oscillator_basis` returns the wrapped `Phase` node's own
`offset_millideg` as `static_offset`, so the final total offset per
fixture is `45deg + 360deg * i / n` — the "global phase + fixture-specific
spread phase" the task brief describes, added together rather than one
overriding the other.

### Composing with `.range()`

`.spread()` only ever changes *which phase* of the underlying oscillator
a fixture reads — never the sampled value itself — so it has no
dependency on `Intensity`, `Angle`, or any other `.range()` target type
(item 7 of the task brief explicitly asks for this). Every documented
example writes `.spread()` before `.range()`, since `.range()` is what
converts to the attribute's own type at the end of the chain, and that's
the only order `.spread()`'s receiver restriction actually allows in V1
(`.range()`'s output is `Signal<Intensity>`/`Signal<Angle>`/..., never
`Signal<Float>`, so `.spread()` can't follow it — same rule that already
rejects `.phase()` after `.range()`).

### The binding engine: same `SignalId`, one context per fixture

This is the part RFC 0005 predicted almost exactly. `Vm::exec_bind_signal`
still installs **one** `SignalId` for the whole target — never a distinct
signal per fixture (item 8) — but now also records each fixture's
`fixture_index`/`fixture_count`, taken directly from `fixtures`' own
position in the already rig-ordered `Vec<FixtureId>` `inception_linker`
produced (`bind_role` pushes `fixture_ids` by iterating
`RoleBinding::fixtures` in written order — never a `HashMap`, so the same
rig always spreads a show the same way; item 4). `SignalBindingStore`'s
internal `BoundSignal { signal, fixture_index, fixture_count }` carries
that alongside the `SignalId`, and `SignalBindingStore::sample` builds a
fresh `SignalSampleContext` per `(fixture, attribute)` entry, every
frame — so two fixtures sharing the same bound signal still each get
their own phase.

`Vm::exec_transition_attribute`'s detach-and-resample step (starting a
`->` transition on a fixture that had a `<-` binding) now reads back the
fixture's own `fixture_index`/`fixture_count` from `unbind` and resamples
with the exact same context the binding was rendering with — a
`.spread()`-bound fixture freezes at *its own* phase when a transition
takes over, not fixture `0`'s.

## Non-goals (do not implement against this RFC)

Carried over unchanged from the task brief:

- Position-based spread (2D/3D coordinates), coordinate-based fixture
  selection.
- A reverse/custom group-ordering DSL — re-ordering the rig binding
  itself is the only way to change spread order in V1.
- Random/seeded spread, tempo/BPM sync, beat sync, palettes, sequences,
  events, Launchpad integration.
- Chaining `.spread()` onto `.spread()`, or `.phase()` onto `.spread()`.

## Testing

`inception-vm::signal` (the offset formula against the documented `360deg`/
`180deg` tables, the single-fixture no-op, the `fixture_count == 0` guard,
composing with a preceding `.phase()`, missed-frame stability, the
structured `UnsupportedSpreadSource` error), `inception-vm::binding` (two
fixtures sharing one bound signal sampling with distinct contexts),
`lux-typeck` (the receiver restriction reusing `.phase()`'s, the
after-`.range()` and chained-`.spread()` rejections, `.spread()` then
`.range()` accepted), `lux-lsp` (completion/hover/signature-help for
`.spread()`, generic over the same `SIGNAL_FLOAT_METHODS` table `.phase()`/
`.range()`/`.invert()` already used, needing no LSP-specific code change),
and `inception-runtime` (the documented DMX table across 4 real fixtures,
`180deg`'s table, deterministic fixture order across two independent
links, missed frames, single-fixture no-op, composing with `.phase()`).
Compile-pass/compile-fail `.lux` fixtures under `tests/compiler/{pass,fail}`
cover the end-to-end accept/reject surface.
