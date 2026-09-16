# RFC 0001: Modules, imports, and a first standard library

Status: implemented (V1). First RFC in this repository — `docs/rfcs/` had
no prior entries, so this one also establishes the format: summary,
motivation, design, and explicit non-goals.

## Summary

Lux gains its first module/import system and an official standard
library, `std.Math` and `std.Color`:

```lux
import std.Math;
import std.Color;

scene main {
    let wave_point = Math.sin(90deg);
    let orange = Color.rgb(255, 120, 20);

    Washes.color = orange;
}
```

This is the first time a Lux program can call a function at all — before
this change, every call expression (`foo()`) was unconditionally an
`unknown function` error; there was no callable namespace of any kind.

## Motivation

Lux show code needed a path from "a fixed set of literals and operators"
towards real composition: computed angles, derived colors, eventually
(in a later milestone) time-varying signals. Bolting ad hoc builtins onto
the parser (`if function_name == "sin" ...`) would violate this
project's "single source of truth" principle (`AGENTS.md`) and make the
LSP unable to know what completions/signatures exist without duplicating
compiler logic. The goal here was to build the smallest real module
system that supports both an official stdlib and, in principle, user
modules — without prematurely designing `std.Effects` or `Signal<T>`,
which are an explicitly separate, later milestone.

## Design

### Import syntax

```lux
import std.Math;
import show.Helpers;
```

`import` <dotted path> `;` — no wildcards (`import std.Math.*;` is a
syntax error) and no aliasing. After import, calls use the *last*
segment as the qualifier: `Math.sin(...)`, never `std.Math.sin(...)`.

### Call syntax

`Math.sin(90deg)` is a qualified call. `lux-syntax::ast::CallExpr.callee`
is now a `CallPath { qualifier: Option<Identifier>, name: Identifier }`
rather than a bare `Identifier` — `qualifier: None` for today's
(always-invalid) unqualified calls, `Some(qualifier)` for a stdlib/module
call. The parser disambiguates a qualified call from an attribute
assignment (`Washes.color = ...`) by checking whether `(` immediately
follows the second identifier.

### Resolution: std vs. user modules

Two independent layers:

1. **`std.*` resolution** — no filesystem access, entirely inside
   `lux-hir::resolve` against `lux_stdlib`'s registry.
2. **User-module file resolution** — filesystem access, entirely inside
   `lux-project` (the only crate allowed to touch disk for this). It
   hands `lux-hir` only a [`lux_hir::UserModuleEnvironment`]: a yes/no
   answer to "does this dotted path resolve to a real file?" — never a
   path, an AST, or file contents. `lux-hir`/`lux-typeck` remain
   filesystem-free.

**Duplicate imports** of the exact same path (`import std.Math;` twice)
are an idempotent no-op, not an error — least surprising, and avoids
penalizing generated or merged show code. Two *different* full paths
importing to the same short name (`std.Math` and `hypothetical.Math`) is
an ambiguity error.

**Diagnostics** (all with precise spans, never a panic):
- `unknown module \`std.Foo\`` for a bad import path.
- `module \`Math\` is not imported` (+ a `did you mean` help naming the
  right import) when a real stdlib module's short name is used without
  ever being imported.
- `module \`Math\` has no member \`foo\`` (+ a "did you mean" suggestion
  when a real member is within edit-distance 2) for an unknown member on
  an imported std module.
- `module \`Helpers\` has no member \`foo\`` for *any* member access on a
  resolved user module — see "User modules" below for why this is
  currently unconditional.

### The stdlib signature model

A new, dependency-free crate, `lux-stdlib`, is the single source of truth
for every stdlib module and function:

```rust
pub struct Signature {
    pub module_path: &'static [&'static str],
    pub name: &'static str,
    pub params: &'static [Param],
    pub return_ty: ParamType,
    pub intrinsic: IntrinsicId,
    pub pure: bool,
    pub doc: &'static str,
}
```

This is real, structured data — parameters, types, documentation,
purity — not an opaque callback. `lux-hir` (import/call resolution),
`lux-typeck` (type checking, overload resolution) and `lux-lsp`
(completion, hover, signature help) all consult this one registry
directly; none of them hardcodes its own function list.

### Why `lux-bytecode`/`inception-vm` do *not* depend on `lux-stdlib`

`lux-stdlib` is unmistakably compiler-frontend shaped (`Signature`,
`ParamType`, string names, documentation). Per `AGENTS.md`, `lux-bytecode`
must not depend on the compiler frontend, and `inception-vm` must stay
usable standalone. This project's existing answer to exactly this tension
is controlled duplication with one narrow, exhaustive conversion function
at the boundary — see `lux_typeck::Type` vs. `lux_bytecode::ValueType`,
and `lux_typeck::Attribute` vs. `lux_bytecode::Attribute`. `IntrinsicId`
follows the same idiom: `lux-stdlib` and `lux-bytecode` each define their
own copy, and `lux-mir::codegen::to_bytecode_intrinsic` is the one
exhaustive match connecting them (adding an intrinsic anywhere forces
every layer's match to be updated — no wildcard arms exist in this path).

The result: the VM dispatches a `CallIntrinsic` purely by
`lux_bytecode::IntrinsicId` — never a string, never `"Math.sin"` compared
anywhere at runtime. Name resolution is finished by the time bytecode
exists.

### Overload resolution

`Math.abs`/`min`/`max`/`clamp` each have an `Int` and a `Float`
overload. Resolution is exact positional type matching, no implicit
conversions. Since Lux has no untyped numeric literals (`Int` and
`Float` are always lexically distinct), the two overload domains never
overlap in practice — `resolve_overload`'s `Ambiguous` case is real,
tested code, but currently unreachable by any valid V1 program.

### Determinism guarantees

Every `std.Math`/`std.Color` function is **pure**: no clock, IO,
randomness, DMX, or mutable global state (`Signature::pure` is `true` for
all of them, checked by a registry-wide unit test). Every intrinsic's VM
implementation is **total** — no `Result`, no panic, no runtime failure
path — because the bytecode verifier already guarantees argument
count/type correctness before a `CallIntrinsic` is ever executed. Where a
value could be considered "misused" at a non-constant call site:

- `Math.clamp(value, min, max)` with `min > max` deterministically
  returns `max` (`value.max(min).min(max)`, evaluated in that fixed
  order — not a special-cased branch).
- `Math.lerp(a, b, t)` never clamps `t`; `t > 1.0` extrapolates.
- `Color.rgb`'s channels and `Color.mix`'s ratio saturate/clamp to their
  valid range rather than trapping.

When every argument to `Color.rgb`/`Math.clamp` is a literal constant,
`lux-typeck` additionally reports a compile-time diagnostic for an
out-of-range channel or `min > max` — catching the mistake before runtime
for the common case, without requiring a general constant-folding engine.

### User modules: full plumbing, zero exports (a deliberate, temporary limitation)

`import show.Helpers;` really resolves a file: `lux-project` reads
`<directory containing source.entry>/show/Helpers.lux`, recursively
follows its own imports, and detects cycles with a three-color DFS (never
a stack overflow — bounded by file count, not program call depth). A
broken imported file fails the whole build, exactly like a broken entry
file would (`lux_compiler::check_program`/`compile_program`) —
"correctness before convenience" over a partially-working build.
`lux-cli`'s file watcher additionally watches every resolved user-module
file, so editing one hot-reloads exactly like editing the entry file.

However: **Lux has no function-declaration syntax yet.** A resolved user
module today can only contain the same top-level items any file can
(`scene`, `rig contract`, `import`) — there is nothing for it to *export*.
`Helpers.foo(...)` therefore always fails with `module \`Helpers\` has no
member \`foo\`` regardless of `foo`. This is intentional infrastructure
for a feature (user-defined functions) that doesn't exist yet, not a bug
— `lux_hir::HirCallee` is kept as an enum with only a `Std` variant today
specifically so a `User { .. }` variant is additive once real exports
exist.

## Explicit out-of-scope (do not implement against this RFC)

- `Signal<T>`, the `<-` binding operator, `std.Effects`, or any
  time-varying/oscillator/wave concept. `Math.sin(angle) -> Float` is an
  immediate value; a hypothetical future `Effects.sine(period) ->
  Signal<Float>` is a fundamentally different, time-dependent concept —
  this RFC deliberately does not conflate them, and the signature model
  (`Signature::return_ty` as a closed `ParamType`) is not yet extended to
  represent a `Signal<T>` return type. That extension is left to the
  milestone that actually introduces `Signal<T>`.
- `Math.random`/any `Random` module — determinism and seeding need their
  own design.
- Arrays/`Collections` — deferred entirely as an explicit, deliberate
  limitation of this milestone (not attempted, not partially built).
- A package manager (`lux add`, a registry, semver resolution). User
  modules here are files on disk in the same project, nothing more.
- A wildcard import (`import std.Math.*;`) or import aliasing.

## Migration notes

- `lux_syntax::ast::CallExpr.callee` changed from `Identifier` to
  `CallPath { qualifier: Option<Identifier>, name: Identifier, span }`.
  Every existing caller that only builds unqualified calls sets
  `qualifier: None`, matching prior behavior exactly.
- `lux_hir::lower_with_modules` is a new, more general sibling of
  `lux_hir::lower` (which now delegates to it with an empty
  `UserModuleEnvironment`) — no existing call site needed to change.
- `lux_compiler::Diagnostic` gained a `source: Option<PathBuf>` field
  (always `None` from the pre-existing single-file `check`/`compile`).
- `lux_project::BuiltProject` gained `user_module_paths: Vec<PathBuf>`.

## Testing

Compile-pass/compile-fail `.lux` fixtures under `tests/compiler/{pass,fail}`
cover import resolution, overload resolution, and every documented
diagnostic. Unit tests exist at every layer: `lux-stdlib` (overload
resolution), `lux-hir` (import table construction), `lux-typeck` (call
type checking + literal-constant diagnostics), `lux-bytecode` (verifier
rejection of malformed `CallIntrinsic`), `inception-vm` (`eval_intrinsic`
at documented boundary values), `lux-project` (cycle detection, diamond
imports), `lux-cli` (watch-path relevance), and `lux-lsp` (import/member
completion, hover, signature help). One end-to-end test
(`inception-vm/tests/e2e_lux.rs::stdlib_module_worked_example_runs_to_completion`)
compiles, links and runs this RFC's worked example through to a real
`LightingState` color.
