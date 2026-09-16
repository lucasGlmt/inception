# AGENTS.md

## Project

Inception is an open-source, strongly typed programming environment for real-time lighting.

- **Lux**: strongly typed lighting language and compiler.
- **Inception**: deterministic runtime executing compiled Lux programs.
- **Rig**: maps semantic lighting roles to physical fixtures and DMX channels.
- **Goal**: detect as many errors as possible at compile/link time and keep runtime execution simple, deterministic, testable and fast.

License: Apache-2.0.

## Core principles

1. **Correctness before convenience**
   - Prefer compile-time errors over runtime errors.
   - Avoid implicit conversions and string-based references.
   - Invalid states should be difficult or impossible to represent.

2. **Deterministic runtime**
   - Runtime code must remain predictable and lightweight.
   - Avoid allocations, parsing, filesystem access and name resolution in hot paths.
   - Timing must be based on a monotonic clock, never accumulated sleeps.

3. **Hardware-independent show code**
   - Lux programs must not depend directly on fixture names, DMX addresses or universes.
   - Shows target typed roles/capabilities.
   - Physical resolution happens through rig/patch linking.

4. **Single source of truth**
   - Compiler rules belong in compiler crates.
   - LSP, CLI, Studio and editors reuse compiler APIs.
   - Never duplicate language semantics in tooling.

5. **Testability is architecture**
   - Core logic must work without physical hardware.
   - Prefer traits/interfaces for clocks, outputs and inputs.
   - Every important feature should be testable with virtual time and virtual DMX.

---

## Workspace boundaries

### Lux

`lux-syntax`
- Lexer, parser, AST, spans.
- No semantic analysis.
- No runtime dependencies.

`lux-hir`
- Name resolution and semantic representation.
- Converts textual names to IDs/symbols.

`lux-typeck`
- Type checking, capabilities and compile-time validation.
- Must reject invalid lighting operations early.

`lux-mir`
- Lower-level representation used before bytecode generation.
- Keep it simple and explicit.

`lux-bytecode`
- Bytecode structures, serialization and verification.
- Must not depend on the compiler.

`lux-compiler`
- Orchestrates:
  `source -> AST -> HIR -> typecheck -> MIR -> bytecode`.

### Inception

`inception-core`
- Pure deterministic domain types and algorithms.
- No USB, MIDI, filesystem, networking or compiler dependencies.
- Keep external dependencies minimal.

`inception-vm`
- Bytecode execution, fibers/tasks and logical scheduling.
- Depends only on runtime/core abstractions and bytecode.

`inception-renderer`
- Converts semantic lighting state into universe/channel buffers.
- Must be independently testable.

`inception-linker`
- Resolves compiled Lux roles against rig, patch and fixture definitions.
- Produces runtime-ready data with numeric IDs and precomputed mappings.

`inception-runtime`
- Orchestration layer.
- Loads verified programs, runs VM, renderer, event sources and outputs.
- Business/domain logic should live in lower-level crates when possible.

### Drivers

Drivers translate hardware/protocol concerns into Inception abstractions.

Examples:
- `inception-driver-dmx`
- `inception-driver-launchpad`

Do not leak protocol-specific types into `inception-core`.

### Tooling

`lux-cli`
- User-facing `lux` executable.
- Reuse compiler/runtime APIs.

`lux-lsp`
- Language server.
- Never reimplement parser/type-checker rules.

---

## Dependency direction

Dependencies must flow downward.

```text
lux-syntax
    ↓
lux-hir
    ↓
lux-typeck
    ↓
lux-mir
    ↓
lux-bytecode

lux-compiler → above Lux crates

lux-bytecode → inception-vm
inception-core → VM / renderer / runtime / drivers

compiler + rig → inception-linker → runtime image
```

Avoid circular dependencies.

`inception-core` must never depend on higher-level crates.

---

## Runtime rules

The runtime must operate on resolved numeric IDs, not names.

Prefer:

```rust
TargetId(u32)
FixtureId(u32)
UniverseId(u16)
```

Avoid runtime lookups such as:

```rust
HashMap<String, Fixture>
```

for execution-critical paths.

Transitions are time-based objects, not loops with sleeps.

Do not implement:

```text
increment value
sleep
increment value
sleep
```

Instead compute state from:

```text
start value
target value
start time
end time
current time
```

Missing a frame must not cause timing drift.

Parallel Lux execution should use lightweight VM tasks/fibers, not one OS thread per scene.

---

## Types

Lux should be strict.

Domain values should use distinct types, e.g.:

```text
Duration
Intensity
Color
Angle
Frequency
Tempo
```

Do not silently treat them as interchangeable numeric values.

Prefer domain-safe internal representations such as integer/fixed precision where appropriate.

Examples of invalid operations that should fail before runtime:

```lux
wait red;
front.color = 50%;
let intensity: Intensity = 150%;
```

Capabilities must also be type checked.

A target without `PanTilt` cannot accept pan/tilt operations.

---

## Rig architecture

Lux show code targets semantic roles:

```lux
role Movers: Group<Color + Intensity + PanTilt>;
```

It should not contain:

```lux
fixture("beam-1");
universe(1);
```

Physical mapping belongs to rig/patch configuration.

The linker must validate:

- missing roles;
- missing capabilities;
- invalid fixture mappings;
- DMX address conflicts;
- invalid universe/channel ranges.

---

## Errors and diagnostics

Diagnostics are part of the product.

Errors should explain:

- what failed;
- where;
- expected value/type;
- received value/type;
- useful correction when obvious.

Prefer:

```text
expected `Color`, found `Intensity`
```

over:

```text
type mismatch
```

Preserve source spans through compiler stages whenever possible.

---

## Testing

Every feature should include tests at the lowest useful level.

Use:

- unit tests for pure logic;
- compile-pass tests;
- compile-fail diagnostic tests;
- linker validation tests;
- VM tests using virtual time;
- renderer tests using virtual fixtures;
- end-to-end tests:
  `Lux -> compile -> link -> VM -> virtual DMX`.

Never require physical lighting hardware for normal CI.

Use a `VirtualClock` instead of real sleeps in tests.

Examples in `/examples` should compile in CI.

---

## Performance

Do not optimize blindly, but protect runtime hot paths.

Prefer:

- precomputed mappings;
- contiguous structures;
- numeric IDs;
- fixed/preallocated buffers;
- reusable memory.

Avoid in hot paths:

- parsing;
- string resolution;
- unnecessary heap allocation;
- blocking I/O;
- filesystem access;
- excessive locking.

Performance-sensitive changes should be benchmarked.

Correctness always comes before micro-optimization.

---

## Development rules

Before adding a new abstraction:

1. determine which layer owns it;
2. avoid leaking higher-level concepts downward;
3. keep APIs minimal;
4. add tests;
5. update documentation if semantics change.

Do not introduce general-purpose language features unless they solve a real Lux use case.

Prefer a small coherent language over a large inconsistent one.

For significant language/runtime design changes, add or update an RFC under:

```text
docs/rfcs/
```

---

## Definition of done

A change is not complete until:

- workspace compiles;
- relevant tests pass;
- `cargo fmt` passes;
- `cargo clippy` introduces no unjustified warnings;
- architectural boundaries remain respected;
- user-facing language behavior has useful diagnostics.
