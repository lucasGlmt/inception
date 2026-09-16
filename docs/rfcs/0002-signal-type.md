# RFC 0002: `Signal<T>`, the first generic type

Status: implemented (V1 — `Signal.constant` only).

## Summary

Lux gains its first generic/parameterized type, `Signal<T>`: a value that
is a pure function of an absolute timestamp, `Timestamp -> T`, rather than
an immediate `T`. This milestone builds only the type itself, its
compiler/runtime plumbing end to end, and one way to construct one:

```lux
import std.Signal;

scene main {
    let level: Signal<Intensity> = Signal.constant(50%);
}
```

`Signal<Intensity>` and `Intensity` are strictly distinct, non-implicitly
-convertible types — the type checker rejects both directions of
coercion. Sampling (`Signal<T> + Timestamp -> T`) exists in the runtime
(`inception_vm::SignalStore::sample`) but is deliberately **not** exposed
to Lux source in this milestone (no `.sample()` method) and **not**
connected to any lighting attribute — `docs/rfcs/0001-modules-and-stdlib.md`
explicitly called this out as future scope, and this RFC is that future
milestone, scoped narrowly on purpose.

## Motivation

`Signal<T>` is the foundation for time-varying show code — oscillators,
eases, the eventual `Front.intensity <- level;` binding operator — but
building all of that at once would mean designing the type system, the
runtime signal representation, *and* a binding operator's semantics in
one pass, with no way to validate the first two independently. This
milestone deliberately validates only the plumbing:

```text
type system -> HIR -> MIR -> bytecode -> verifier -> VM -> signal runtime -> sample(timestamp)
```

`Signal.constant` is not functionally interesting on its own (a constant
that stays constant) — its entire purpose is to exercise every stage of
that pipeline with the simplest possible signal, before oscillators or
composition make the runtime representation more complex.

## Design

### `Type::Signal` stays `Copy`

`lux_typeck::Type` is a flat, `Copy`, exhaustively-matched enum with no
prior notion of a parameterized type. The obvious `Type::Signal(Box<Type>)`
would strip `Copy` from the *entire* `Type` enum (a `Box` payload isn't
`Copy`), which ripples into every one of `Type`'s many by-value call
sites (`MirLocal.ty: Type`, `local_types: HashMap<LocalId, Type>` copied
out with `.copied()`, etc.) — a wide, high-risk mechanical diff for a
type that, in V1, can only ever wrap one of 5 non-`Signal` scalar types
anyway.

Instead:

```rust
pub enum Type {
    Bool, Int, Float, Duration, Intensity, Color, Angle, Frequency, Tempo,
    Signal(SignalElement),
}

pub enum SignalElement { Int, Float, Angle, Intensity, Color }
```

`SignalElement` is a small, separate, non-recursive `Copy` enum — not
`Type` reused recursively. This keeps `Type` itself fully `Copy`, and as
a side effect makes `Signal<Signal<T>>` *structurally* unrepresentable
(the type checker rejects the syntax with a clear diagnostic instead of
needing a runtime "no nesting" check — see "Non-goals" below). The same
duplication is mirrored independently in `lux_bytecode::ValueType::Signal
(ScalarValueType)`, matching this codebase's existing frontend/runtime
boundary-duplication idiom (`Type`/`ValueType`, `IntrinsicId`/`IntrinsicId`
— see RFC 0001).

`SignalElement`'s 5 variants (`Int, Float, Angle, Intensity, Color`) are
exactly the 5 types `lux_stdlib::ParamType` already represents. `Bool`,
`Duration`, `Frequency`, `Tempo` have no stdlib representation *at all*
today (not just inside `Signal<...>`), so excluding them from
`Signal<T>` is consistent with the existing limitation, not a new,
arbitrary one. Extending either later is the same mechanical step:
give `ParamType` (and `SignalElement`) another variant.

### Generic type syntax: `Name<Arg>`, parsed generally, validated narrowly

`lux-syntax::ast::TypeName` gains `type_args: Vec<TypeName>` (mirrored by
`lux-hir::TypeAnnotation`). The parser accepts `Name<Arg>` for *any*
`Name`, and even lets `Arg` itself carry type arguments — it has no
opinion on which names are actually generic or whether nesting is legal,
matching this crate's standing rule that only `lux-typeck` decides which
type names/shapes are valid. `<`/`>` needed no lexer changes: Lux defines
no comparison operators, so a standalone `Less`/`Greater` token in type
position is already unambiguous (the same reasoning already relied on for
`Group<Color + Intensity>` in rig-contract syntax).

`lux_typeck::types::resolve_annotation` is the new, single place that
turns a parsed `TypeAnnotation` into a `Type`, and is the *only* place
that knows `"Signal"` is the one generic name Lux currently has:

- any non-`"Signal"` name with a type argument is rejected
  (`` `Foo` does not take type arguments ``) — this is deliberately not a
  general generics feature; per `AGENTS.md`, `Signal<T>` is a special-cased
  builtin, not the first of many user-definable generic types.
- `Signal` requires exactly one argument, which must itself resolve to a
  `SignalElement`-representable type (else a diagnostic naming exactly
  which 5 types are valid) and must not itself be a `Signal` (rejecting
  `Signal<Signal<...>>` explicitly, even though `Type`'s own shape would
  have made it unrepresentable anyway — this gives a real diagnostic
  instead of a confusing "unknown type" fallback).

### `Signal.constant`'s polymorphic return type, without real generics

The obvious shape for `Signal.constant`'s type is `T -> Signal<T>`, but
`lux_stdlib::Signature::return_ty: ParamType` is a fixed value baked into
`'static` table data — there's no way for it to depend on the call's
actual argument type, and `lux-hir`/`lux-typeck`'s call resolution has no
notion of a generic function.

Rather than inventing a parallel, generics-shaped resolution path, this
is solved by monomorphizing one level down, at `ParamType` — the same
"list every concrete overload as its own table row" idiom this codebase
already uses for `Math.abs(Int)`/`Math.abs(Float)`:

```rust
pub enum ParamType {
    Int, Float, Angle, Intensity, Color,
    SignalInt, SignalFloat, SignalAngle, SignalIntensity, SignalColor,
    Unsupported,
}
```

`std.Signal`'s registry entry lists 5 concrete `Signature`s for
`constant` — `constant(Int) -> SignalInt`, ..., `constant(Color) ->
SignalColor` — each with its own monomorphized `IntrinsicId`
(`SignalConstantInt`, ..., `SignalConstantColor`). `lux_stdlib::resolve_overload`,
`lux-hir`'s import/call resolution, and `lux-lsp`'s completion/hover/
signature-help all work **completely unchanged** — no new `HirCallee`
variant, no parallel resolution path, no LSP-specific code beyond
teaching `param_type_name` the 5 new labels. Signature help on
`Signal.constant(` shows 5 concrete overloads rather than one generic
`constant(value: T) -> Signal<T>` line; this is the "adapted presentation"
for a case where the LSP can't render real generics, not a compromise
that leaks into the compiler.

### No new MIR/bytecode instruction

`Signal.constant` lowers exactly like `Color.rgb` does: a `CallIntrinsic`
with a monomorphized `IntrinsicId`. `lux-mir`'s `CallIntrinsic` lowering
and `lux-bytecode`'s verifier are both already fully generic over
`IntrinsicId` — they needed **zero** structural changes. The verifier's
existing stack simulation (pop `param_types().len()` operands, check each
against `ValueType`, push `return_type()`) already distinguishes
`Signal(Intensity)` from `Signal(Color)` from `Intensity` correctly, once
`ValueType::Signal` carries `ScalarValueType` and derives `PartialEq`
(which it already does, trivially, as a `Copy` enum).

### Runtime: `SignalId` + `SignalStore`, not an inline payload

```rust
pub struct SignalId(pub u32);
pub enum SignalKind { Constant(Value) }
pub struct SignalDefinition { pub kind: SignalKind }
pub struct SignalStore { /* Vec<SignalDefinition>, append-only */ }
```

`inception_vm::Value::Signal` carries `(ScalarValueType, SignalId)` — the
element type tagged alongside a handle into a `SignalStore` the executing
`Vm` owns, not the signal's definition inline. Two things drove this:

- **Extensibility.** V1 has only `SignalKind::Constant`, but a future
  composed signal (`Map` over another signal, a node graph) needs to
  refer to *other* signals by id — an inline `Value::Signal(SignalValue)`
  payload could represent `Constant` today, but would need reworking
  the moment `Value` itself would otherwise have to become
  self-referential/unbounded in size. Starting with a store now means
  that future extension only adds new `SignalKind` variants; it never
  has to change what `Value::Signal` *is*.
- **`value_type()` must stay total and panic-free.** `Vm` calls
  `Value::value_type()` defensively — e.g. `exec_store_local` re-checking
  an already-verified type — and must never panic even on artificially
  malformed bytecode (a standing rule: `lux-bytecode`/`inception-vm`
  never trust that bytecode came from the real compiler). A bare
  `Value::Signal(SignalId)`, with the element type only recoverable by
  looking the id up in a `SignalStore`, can't answer `value_type()` on
  its own — so the element type is carried directly on the value
  (`ScalarValueType`), keeping `value_type()` a pure, total function of
  `Value` alone, exactly like every other variant.

`SignalStore::sample(id, timestamp) -> Result<Value, SignalError>` takes
the timestamp explicitly — a signal never reads a clock itself, the
caller always provides `now`, which is what makes sampling trivially
testable and deterministic (`docs/language/signals.md` covers this from
the language-user's side). `SignalError::UnknownSignal` covers a
corrupted/out-of-range `SignalId` cleanly rather than panicking.
Signal definitions are immutable once inserted; sampling never mutates
one. Two `Signal.constant(50%)` calls at different call sites produce two
distinct `SignalId`s with equal content — deduplication was never a V1
guarantee.

`SignalStore` is created empty by `Vm::new` and owned by the `Vm` for its
whole lifetime; reloading a program means constructing a new `Vm` (and so
a new, empty store) for the newly compiled module, with no migration of
old `SignalId`s — consistent with how the rest of a reloaded program's
state isn't preserved across a `LoadedProgram` swap either.

The 5 `SignalConstant*` intrinsics are the one exception to
`inception_vm::intrinsic::eval_intrinsic` being a pure, `Vm`-state-free
function: constructing a signal needs to insert into `self.signals`,
so `Vm::exec_call_intrinsic` recognizes those 5 ids and handles them
directly, the same way `SetAttribute`/`TransitionAttribute`/`Wait`
already need `Vm`-owned state outside the purely-functional intrinsic
path.

## Non-goals (do not implement against this RFC)

Carried over unchanged from the task brief that produced this milestone
(the list below describes this RFC's own scope; the `<-` operator was
since implemented as a *later*, separate milestone — see
`docs/rfcs/0003-signal-binding-operator.md`):

- The `<-` binding operator, or any assignment of a `Signal<T>` to a
  lighting attribute (`Front.intensity = signal;` remains a type error —
  an attribute always expects its immediate type, never `Signal<T>`).
- `std.Effects`, `Signal.sine`/`triangle`/`saw`/`square`, or any
  time-varying signal kind beyond `Constant`.
- Signal composition (`map`, `range`, `phase`, a node graph) — the
  `SignalId`/`SignalStore` design is chosen so this is addable later
  without reworking `Value::Signal`, but nothing here builds it.
- A `.sample()` method exposed to Lux source — sampling exists only as an
  internal runtime/test API (`SignalStore::sample`), so it can change
  freely before a real binding operator needs to expose *some* form of it.
- User-definable generic types (`struct Foo<T> { ... }`) or generic
  functions — `Signal<T>` is one special-cased builtin type, not the
  first instance of a general generics feature.
- Fixture/frame context in sampling (`signal + timestamp` only, no
  fixture index, target id, or renderer state).

## Testing

Unit tests at every layer, matching this workspace's existing pyramid:
`lux-syntax` (parses `Signal<Intensity>`, including rejecting-later
`Signal<Signal<Intensity>>` syntactically-valid-but-semantically-rejected
nesting), `lux-typeck` (type inference per element type, type equality/
inequality, every documented diagnostic), `lux-stdlib` (overload
resolution for all 5 `constant` signatures), `lux-bytecode` (verifier
accepts/rejects `Signal<T>` on the stack correctly), `lux-mir` (lowering
+ full bytecode verification), `inception-vm` (`SignalStore` deterministic
sampling at various and non-monotonic timestamps, repeated-sampling
equality, unknown-id error — plus one test driving sampling through a
real `Vm`/bytecode path). Compile-pass/compile-fail `.lux` fixtures under
`tests/compiler/{pass,fail}` cover the end-to-end accept/reject surface.
One end-to-end test (`inception-vm/tests/e2e_lux.rs::signal_constant_program_compiles_and_runs_to_completion`)
compiles and runs this RFC's worked example through the full pipeline.
