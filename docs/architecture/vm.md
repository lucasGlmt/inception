# Inception VM (V1)

A minimal, deterministic, single-fiber interpreter for `lux_bytecode::BytecodeModule`.
This is a summary — the authoritative documentation lives as rustdoc on
`inception-vm::vm` (VM model, execution loop) and `inception-core::time`
(`Clock`/`VirtualClock`, `WAIT` semantics).

## Dependency direction

```
lux-bytecode
      ↓
inception-vm  (may also depend on inception-core)
```

Never the reverse. The VM has no notion of Lux source, the AST, HIR or
MIR — it consumes exactly one thing, a verified `BytecodeModule`.

## VM model

`Vm` holds: the module, the entry `FunctionId`, an explicit `VmState`, a
single operand stack shared across the whole call stack, the current
`Frame` (`function`, `pc`, `locals: Vec<Option<Value>>`), and a
`call_stack: Vec<Frame>` of saved callers.

States: `Ready` (constructed, not started) → `Running` (only observed
transiently inside `run_until_blocked`) → either `WaitingUntil(Timestamp)`,
`Finished`, or `Faulted(VmError)`. Modeled as one enum, not several
booleans, so contradictory combinations (e.g. "waiting" and "finished" at
once) can't be represented.

`Vm::new` always calls `lux_bytecode::verify` and requires an `entry`
function — a `Vm` can only exist for a module already known to be
statically safe. Every execution-time operation still defends itself
anyway (bounds/type checks return a `VmError`, never `panic!`/`unwrap()`),
because `Vm::new` having verified once doesn't excuse trusting a
hand-built or corrupted module blindly.

## Clock abstraction

`inception_core::Clock` is the only way engine/VM code learns the time.
`VirtualClock` (nanosecond `Cell<u64>`, `&self`-mutable via interior
mutability) lets tests advance time instantly with no real sleeping and
no locking. `MonotonicClock` (built on `Instant`) exists for later use
outside tests, but is not this milestone's focus.

## `WAIT` semantics

`WAIT` never sleeps. It pops a `Duration`, computes an absolute wake-up
timestamp (`clock.now() + duration`), moves the VM to
`WaitingUntil(wake_at)`, and returns control to the caller — there is no
`sleep()`, no busy loop, and no `remaining -= tick` countdown.

`run_until_blocked(&clock)` compares `clock.now()` against `wake_at` on
each call: too early is a no-op (still blocked), and once elapsed the VM
resumes exactly after the `WAIT` instruction (the frame's `pc` already
points past it). A runtime that polls late — e.g. advancing the clock by
5s when only 1s was needed — resumes in one step; it never tries to
"catch up" through intermediate ticks.

## Arithmetic

The VM's `apply_binary` re-implements the same type-combination table
already enforced statically by `lux_bytecode::verify` (which in turn
mirrors `lux_typeck::rules`). This is intentional, boundary-preserving
duplication — `inception-vm` must not depend on the compiler frontend —
not an oversight; drift between the tables would show up as an
end-to-end test failure. Overflow/underflow/division-by-zero are always
structured `VmError`s, never a silent wraparound or a Rust panic.
