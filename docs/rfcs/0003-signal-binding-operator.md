# RFC 0003: the `<-` signal-binding operator

Status: implemented (V1 — one active controller per attribute, no
crossfade, no spread).

## Summary

RFC 0002 introduced `Signal<T>` as a type with full compiler/runtime
plumbing, but connected it to nothing: a signal could be built and
sampled, but never drove a lighting attribute. This RFC adds the operator
that closes that gap:

```lux
import std.Signal;

scene main {
    let level = Signal.constant(50%);

    Front.intensity <- level;
}
```

`Front.intensity <- level;` makes `Front.intensity` continuously derived
from `level`, at every timestamp the runtime renders, until the binding
is replaced or detached — as opposed to `=` (set once, immediately) and
`->` (interpolate once, over a fixed duration, then stop). See
`docs/language/signals.md` for the user-facing explanation of all three.

## Design

### Frontend: a fourth statement kind, not a variant of `=`

`<-` gets its own token (`TokenKind::LeftArrow`, lexed the same way `->`
is — one extra lookahead byte on `<`), its own AST/HIR node
(`BindSignalStatement`/`HirBindSignal`, shaped exactly like
`Assign`/`Transition`), its own MIR instruction
(`MirInstruction::BindSignal`), and its own bytecode opcode
(`Instruction::BindSignal`). It is never desugared into "sample once,
then `SET_ATTRIBUTE`" — that would throw away the "continuous" part of
its semantics, which is the entire point of the operator (see AGENTS.md's
existing rule that `=`/`->`/`<-` must stay distinct all the way down).

Type checking requires the right-hand side to be exactly
`Signal<T>`, where `T` is the attribute's own value type
(`SignalElement::from_type(attribute.value_type())`) — never `T` itself
(no implicit wrap) and never a `Signal<U>` for a mismatched `U` (no
element-type coercion). Capability checking is unchanged: `<-` runs
through the same `check_role_capability` call `=`/`->` already use, so a
role lacking `Color` still rejects `Front.color <- signal;` exactly like
it rejects `Front.color = red;`.

### Bytecode verifier

`BIND_SIGNAL target attribute` is verified like `SET_ATTRIBUTE`: `target`
must be in range, and the popped operand must have type
`attribute.signal_value_type()` (`Signal<Intensity>` or `Signal<Color>`,
never a bare `Intensity`/`Color`). Unlike `TRANSITION_ATTRIBUTE`, there is
no "supported attribute" gate — binding works for every attribute the
type checker accepts, matching `SET_ATTRIBUTE`'s breadth rather than
`TRANSITION_ATTRIBUTE`'s V1 intensity-only restriction.

### Runtime representation: `SignalBindingStore`

A new `inception_vm::binding::SignalBindingStore`, deliberately placed in
`inception-vm` rather than `inception-core`: sampling a binding needs a
`SignalStore` (which owns `SignalKind`s, including `Value`s from this
crate), and `inception-core` must never depend on `inception-vm` (see
AGENTS.md's dependency-direction rule). This is the one place that knows
both "what a signal is" and "what a `(fixture, attribute)` binding is" —
`LightingState`/`TransitionEngine`/the renderer know neither:

```text
Signals (SignalStore)
    v
SignalBindingStore
    v
LightingState (signal-agnostic)
    v
Renderer (signal-agnostic)
    v
DMX
```

`SignalBindingStore` stores at most one `SignalId` per `(FixtureId,
Attribute)` key — the same invariant, and the same `HashMap`-backed
shape, as `inception_core::TransitionEngine::active`. `Vm` owns one
alongside its `SignalStore`.

`BIND_SIGNAL`'s VM handler (`Vm::exec_bind_signal`) does exactly three
things, per this RFC's task brief item 20 ("the VM must not manage
successive samples"):

1. resolve `target` to its fixtures;
2. for each fixture, detach (and stabilize — see below) any transition
   active on the same attribute;
3. register the binding.

It never itself calls `SignalStore::sample` for the *new* binding. The
first visible value comes from the runtime loop's existing per-frame
cycle (see below) — same tick, so there is no visible delay, but the
responsibility stays outside the VM.

### Sampling: a new per-frame step, parallel to `TransitionEngine::sample`

`Vm::sample_signal_bindings(now, &mut lighting)` samples every active
binding and writes the result into `LightingState` — the exact runtime
counterpart to `TransitionEngine::sample`. `inception_runtime::LoadedProgram::advance`
now runs three steps per tick, in order:

```text
vm.run_until_blocked(...)       // execute BIND_SIGNAL/SET_ATTRIBUTE/... if any
transitions.sample(now, ...)    // existing transition interpolation
vm.sample_signal_bindings(now, ...) // new: signal bindings
```

Because this runs every tick regardless of whether the VM produced any
new instructions that tick, a binding keeps driving DMX long after the
scene that created it finishes (task brief item 36) — it is not tied to
any VM instruction still "in flight".

### Replacement and detachment rules (V1: one controller per attribute)

Exactly one of {no controller, a transition, a signal binding} exists for
a given `(fixture, attribute)` at a time:

- **`<-` replaces `<-`**: `SignalBindingStore::bind` overwrites the
  `HashMap` entry unconditionally.
- **`=` detaches `<-`**: `exec_set_attribute` unbinds every fixture in
  the target before delegating to `TransitionEngine::set_target_attribute`
  (which already cancels any transition the same way).
- **`->` detaches `<-`**: `exec_transition_attribute` unbinds each
  fixture *and*, if it had a binding, samples that signal at `now` and
  writes the result into `LightingState` before starting the transition —
  so `TransitionEngine::start_transition`'s own read of "the current
  value" (its `from`) picks up the signal's real value, never a stale
  `LightingState` default. This is the one place `<-`'s detachment logic
  needs to reach into `SignalStore` directly rather than only the
  binding store, precisely to fix this ordering hazard.
- **`<-` detaches `->`**: a new `TransitionEngine::cancel(now, fixture,
  attribute, state)` method samples the active transition at `now`,
  writes that value into `LightingState`, and removes it — before the
  new binding is installed. The signal then wins on the very next
  `sample_signal_bindings` call, same tick — no automatic crossfade.

## Non-goals (do not implement against this RFC)

Carried over unchanged from the task brief that produced this milestone:

- `std.Effects`, `Signal.sine`/`triangle`/`saw`/`square`, or any
  time-varying signal kind beyond `Constant` — this RFC's architecture is
  built so adding one later (a new `SignalKind` variant) requires no
  change to `<-`, `SignalBindingStore`, or the sampling loop at all: they
  only ever call `SignalStore::sample(id, now)`, never match on
  `SignalKind`.
- Signal composition (`.map`, `.range`, `.phase`, arithmetic on a
  `Signal<T>`).
- Per-fixture spread — every fixture a binding reaches renders the same
  sampled value; nothing here distinguishes fixtures within one target.
- Crossfading between an outgoing transition/signal and an incoming one —
  V1 always switches instantaneously.
- A blackout *mode* that persists across ticks — the runtime today only
  has a one-shot `blackout()` DMX send, not a mode `LightingState`
  participates in, so there is nothing for signal bindings to interact
  with here beyond what already holds: the next real `tick()` re-renders
  from freshly sampled state regardless.

## Testing

Same pyramid as RFC 0002: `lux-syntax` (lexing `<-` unambiguously against
`<`/`-`/`->`/`=`, parsing, LSP-style incomplete-input recovery),
`lux-typeck` (valid binding, direct-value rejection, wrong-element-type
rejection, capability rejection, `<-` followed by `->` type-checking
together), `lux-bytecode`/`lux-mir` (verifier acceptance via the existing
compiler pipeline), `inception-vm` (binding creation across every fixture
of a target, replacement, `=`/`->` detachment including the
sampled-value-not-default assertion, transition-detached-by-binding with
the "wins immediately" assertion, signal sharing across targets),
`inception-runtime` (stable DMX at arbitrary timestamps through the real
`RuntimeEngine`, multi-fixture targets, shared signals across two roles,
persistence past scene completion, `=` after `<-` staying stable, hot
reload sampling a binding before discarding it), and `lux-lsp` (expected
type after `<-` per attribute, completion ranking by `Signal<T>` element,
hover on a signal local, immediate diagnostics for both invalid cases,
inline `Signal.` member completion inside a `<-` statement).
