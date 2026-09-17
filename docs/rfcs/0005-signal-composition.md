# RFC 0005: signal composition — `.range()`, `.phase()`, `.invert()`

Status: implemented (V1 — three builtin transformations, no user-defined
ones, no `Color` range, no spread).

## Summary

Adds the first real signal *graph*: method-call syntax on `Signal<Float>`
values, and three builtin transformations built on it:

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

This is also the first time Lux gets genuine method-call syntax
(`<expr>.<name>(<args>)` on an arbitrary expression, not just a bare
module qualifier) — everything downstream (typeck, MIR, bytecode, VM,
LSP) had to grow a second, parallel call shape alongside the existing
qualified stdlib call.

## Design

### Method-call syntax: reusing what already existed for the common case

`Ident.Ident(args)` (`Effects.sine(2s)`, but also `wave.range(...)`) was
already fully parsed by `parse_primary_expression`'s existing qualified-
call handling — the parser has never known the difference between "a
module qualifier" and "a local variable name", both are just an
identifier followed by `.name(...)`. So a single-level method call on a
`let`-bound local (`wave.range(...)`) needed **zero parser changes**:
it parses into the exact same `ast::CallExpr`/`CallPath` shape
`Math.sin(...)` always has.

What's new is *chaining*: `Effects.sine(2s).phase(90deg)` needs a
further `.phase(90deg)` attached to something that is no longer a bare
identifier (it's already a `CallExpr`). That's the one genuinely new
piece of grammar: `parse_postfix_expression` wraps
`parse_primary_expression` in a loop that looks for `.method(args)`
suffixes and wraps them in the new `ast::MethodCallExpr { receiver,
method, args, span }` node — additive, no changes to existing call
parsing.

### Disambiguating "module call" from "method call" — at name resolution, not parse time

The parser can't tell `Effects.sine(...)` from `wave.range(...)` apart
(same shape), so `lux-hir`'s `resolve_call` does: it first tries the
qualifier against the file's imports (unchanged, existing behavior —
modules always win), and only if that fails, checks whether the
qualifier resolves to a local variable in scope. If it does, the call is
re-shaped into the same `HirExpr::MethodCall` node a genuine chained
`ast::MethodCallExpr` produces. This means HIR has exactly **one**
canonical shape for "call a method on a value", regardless of which of
the two syntactic forms produced it — `lux-typeck`, `lux-mir` and the
LSP's semantic analysis never need to know which.

### Method resolution: a second, small `Signature` table — never a hardcoded string

Item 6 of the task brief is explicit: `.range()` must not become a
special-cased string anywhere in the VM. It doesn't: `lux_stdlib::methods`
defines `SIGNAL_FLOAT_METHODS`, reusing the exact same `Signature`/
`Param`/`ParamType`/`IntrinsicId` types the `std.Math`/`std.Effects`
registry already uses, just addressed by *receiver type* instead of
*import path* (there is no `import` for a method — its "namespace" is
`Signal<Float>` itself). `range` is monomorphized per target element
type exactly like `Signal.constant`'s five overloads — three
`Signature`s (`Float`/`Intensity`/`Angle`), disambiguated by argument
type through the same overload-resolution shape `resolve_overload`
already uses. This is also what gives "range bounds must have the same
type" for free: passing `(Intensity, Angle)` matches none of the three
overloads (each requires both bounds to share one `T`), so it's an
ordinary `NoMatchingOverload`, not a special-cased check.

`lux-typeck`'s `check_method_call` checks the receiver's type once, up
front (`Signal<Float>`, required by every method in the table today),
rather than baking that requirement into each signature — this is also
where `.phase()`'s extra structural restriction lives (see below).

### Bytecode: the receiver is operand 0, nothing else changes

A method call lowers to the exact same `CallIntrinsic` instruction
regular stdlib calls use: the receiver is pushed first, then arguments
left to right, then `CallIntrinsic { intrinsic, arg_count: 1 + args.len() }`.
`lux_bytecode::IntrinsicId::param_types()` for `SignalRange*`/
`SignalPhase`/`SignalInvert` lists the receiver (`Signal<Float>`) as
operand 0 — so the *existing*, fully generic `CallIntrinsic` verifier
arm (no changes needed) already rejects a malformed receiver type, a
wrong bound type, or a wrong arity, exactly as it already did for every
other intrinsic.

### The signal graph: `Range`/`Phase`/`Invert` reference their source by `SignalId`

```rust
SignalKind::Range { source: SignalId, min: Value, max: Value }
SignalKind::Phase { source: SignalId, offset_millideg: i32 }
SignalKind::Invert { source: SignalId }
```

No deep copy: `let b = a.phase(90deg);` makes `b`'s definition hold
`a`'s `SignalId`, never a copy of `a`'s own definition — `a` is
untouched. A cycle is structurally impossible to build through the
normal construct-then-reference path: `SignalStore::insert` only ever
hands out the *next* sequential id, so a node's `source` (read from a
`Value::Signal` that must already exist by construction time) can only
ever name an id strictly smaller than its own. No runtime cycle
detection is needed as a result.

`Range`/`Invert` compose with *any* `Signal<Float>` source — they're
pure value remaps with no notion of "period" to depend on.
`SignalStore::sample` recurses into `source` for both; recursion depth
is bounded by how deeply a single Lux expression chains method calls,
which (Lux having no loops or user recursion) is exactly the source
file's own written nesting.

### `.range()`: clamp the source, never the output

```text
x = clamp(sample(source, now), 0.0, 1.0)
result = min + x * (max - min)          // in min/max's own representation
```

Computed directly in the target type's native form (`Intensity`'s raw
`u16`, `Angle`'s millidegrees) — never through a percent/degree string
round trip. `min` may be greater than `max`; the formula handles a
reversed ramp with no special case. The clamp is on the *input*
(keeping the `0.0..1.0` contract the four oscillators already guarantee,
even if a future signal kind or a chained `.invert()` produces something
outside that range) — `.range()`'s own *output* is exactly `min..max`
by construction, and `.invert()`'s output is deliberately **not**
clamped (`1.0 - source`, literally), so a `.range().invert()` chain can
legitimately produce a value outside `0.0..1.0` — documented, not a bug.

### `.phase()`: restricted to base oscillators, on purpose

Item 25 of the task brief flags the real hazard directly: implementing
phase-shifting as "sample the source at `now + some Duration`" needs to
convert an angle offset into a time delta, which needs to know the
source's period — a concept only `Sine`/`Triangle`/`Saw`/`Square`
actually have. A `Range`- or `Invert`-derived `Signal<Float>` has no
such period to convert against.

Rather than build a generic "phase-shift any signal" abstraction (a much
larger problem — item 26 gestures at a `SignalSampleContext`, but even
that still needs a period from *somewhere*), V1 takes the restriction
item 29 explicitly allows: `.phase()` is only valid directly on an
`Effects` oscillator, or on another `.phase(...)` chained from one.
`lux-typeck`'s `check_method_call` enforces this **statically** by
walking the receiver's HIR shape (`is_phase_source_valid`, recursing
through `.phase()` calls) — so `Effects.sine(2s).range(0.0, 1.0).phase(90deg)`
is a compile-time error, not a runtime one. A chain of `.phase()` calls
is flattened at construction time in `Vm::exec_call_intrinsic` (offsets
combined, always referencing the original base oscillator directly, never
nesting `Phase` inside `Phase`), and `SignalStore::sample`'s `Phase` arm
keeps a defensive fallback (`SignalError::UnsupportedPhaseSource`) for
hand-built bytecode that bypasses typeck — never reachable from real Lux
source, kept only for the same reason every other `SignalError` exists.

The shift itself needs no time-to-angle conversion at all: it operates
on the already-normalized `phase ∈ [0, 1)` directly —
`shifted = (phase_at(now, started_at, period) + offset/360deg).rem_euclid(1.0)`
— then reapplies the *same* waveform function (`sine_wave`/`triangle_wave`/
...) the source itself would have used. `offset_millideg` is normalized
to `0..360_000` once, at construction, so `450deg` and `90deg` (and a
negative offset, via `rem_euclid`) all behave correctly with no special
casing at sample time.

## Non-goals (do not implement against this RFC)

Carried over unchanged from the task brief:

- Per-fixture spread (`.spread(...)`) — no signal node knows about
  `FixtureId`/`TargetId`/the rig, and this RFC doesn't change that (see
  "prepares `.spread()`" below). **Superseded**: implemented in
  `docs/rfcs/0006-signal-spread.md`.
- Tempo/BPM, easing curves, random/noise signal kinds, additional
  `Effects` oscillators.
- Generic signal arithmetic (`wave * 0.5`, `wave + otherWave`) or
  combinators (`map`, `zip`, `combine`, `fold`, `select`, `switch`).
- `.range()` for `Color` — RGB/HSV/linear-light interpolation are each a
  real, different design decision, deliberately deferred rather than
  picked hastily.
- User-defined generic types or functions — `range`/`phase`/`invert`
  are three specific builtin methods with a builtin signature table,
  not a general method-dispatch or generics feature.
- Named call arguments (`Effects.sine(period: 2s)`) — calls, including
  method calls, stay positional; Lux still has no named-argument syntax.

## Architectural impact: `<-` and the binding engine are still untouched

Same validation as RFC 0004: `inception_vm::binding::SignalBindingStore`
is not part of this milestone's diff at all. It only ever calls
`SignalStore::sample(id, now)` and converts the resulting `Value` via
`into_attribute_value` — precisely because `Range`'s output is exactly
`Value::Intensity`/`Value::Color`/... in the right shape, `<-` binds a
composed signal exactly as it would a plain `Signal.constant`, with zero
new code.

## Prepares `.spread()`

A future per-fixture `.spread(...)` needs the binding engine (not any
`SignalKind`) to sample a signal *per fixture* rather than once per
`(fixture, attribute)` binding — e.g. by threading a fixture index into
`sample`, or by each fixture getting a distinct, small phase/seed offset
applied at the `SignalBindingStore` level. Nothing in this RFC's design
forecloses that: `Range`/`Phase`/`Invert` are pure `(SignalId, Timestamp)
-> Value` functions with no fixture awareness baked in, matching every
existing `SignalKind`, so `.spread()` can be layered on as a *binding-
level* concept later without revisiting `sample`'s signature or any
existing `SignalKind` variant.

**Update**: this is exactly the shape `docs/rfcs/0006-signal-spread.md`
implements — `sample`'s signature grew a `SignalSampleContext` (superseding
the bare `Timestamp` this RFC describes) carrying `fixture_index`/
`fixture_count`, and `SignalBindingStore` now samples each fixture in a
target with its own context. `Range`/`Phase`/`Invert` needed zero changes
beyond that signature threading, confirming the prediction above.

## Testing

Same pyramid as RFC 0003/0004: `lux-syntax` (chained method-call
parsing, the bare-identifier-receiver case staying a `CallExpr`,
recovery on incomplete method calls), `lux-stdlib` (`SIGNAL_FLOAT_METHODS`
overload resolution, the "range bounds must match" fallout, purity),
`lux-typeck` (return-type inference for all three methods and their
compositions, the receiver-type gate, the `.phase()` structural
restriction including the chained-`.phase()` exception and the
after-`.range()` rejection, the `<-` binding acceptance/rejection by
final element type), `lux-bytecode` (verifier acceptance via the
existing generic `CallIntrinsic` check), `inception-vm` (`phase_at`/
waveform-reuse and clamp/formula unit tests for all three
transformations against the documented tables, reversed bounds, phase
360°/450°/negative-offset equivalence, phase-on-non-oscillator structured
error, source-by-reference-not-copy, a real `Vm`-driven construction
test including `.phase()` chain flattening), `inception-runtme`
(the exact documented DMX table through the real `RuntimeEngine`, missed
frames, `.phase()`-before-`.range()` reaching DMX, hot reload rebuilding
the graph), and `lux-lsp` (method completion on a `Signal<Float>` local,
signature help/hover for all three methods, argument-position narrowing
for `range`'s second bound, unannotated-composition hover reporting the
final inferred type, immediate diagnostics for unsupported `Color`
bounds). Compile-pass/compile-fail `.lux` fixtures under
`tests/compiler/{pass,fail}` cover the end-to-end accept/reject surface.
