# Compile and link architecture

Inception deliberately separates a portable show from a venue-specific runtime
image:

```text
Lux Show
   ↓ compile
Portable Bytecode
   +
Rig/Patch/Fixtures
   ↓ link
RuntimeImage
   ↓
Inception Runtime
```

The Lux scene refers only to typed role names. Fixture model names, fixture
instance names, universes, and DMX addresses do not appear in show logic.

## Portable compile output

A Lux `rig contract` declares the roles the show needs:

```lux
rig contract DemoRig {
    role Washes: Group<Color + Intensity>;
}
```

Compilation assigns deterministic `RoleId` and `TargetId` values in source
order. The resulting `BytecodeModule` contains a `PortableRigContract` with
debug/link metadata (contract and role names), required `CapabilitySet`s, and
`GroupNonEmpty` cardinality. Instructions already use numeric `TargetId`s.

The portable module has no venue, fixture definition, universe, address, or
DMX channel mapping. `lux_compiler::compile_portable` therefore compiles once
without a manually injected target environment.

## FixtureDefinition

A `FixtureDefinition` is a model, not a physical fixture. It contains:

- a debug/configuration name;
- a validated footprint;
- an explicit `CapabilitySet` (`Intensity`, `Color` in V1);
- zero-based channel offsets for intensity and RGB relative to a fixture's
  one-based base address.

Construction validates that declared capabilities and mappings agree, offsets
fit inside the footprint, and semantic channels do not reuse an offset.

## Patch and physical resolution

A `Patch` contains fixture instances. Each instance names its fixture
definition and carries a `UniverseId` and safe one-based `DmxAddress`. Instance
order deterministically assigns numeric `FixtureId`s.

Phase A, `resolve_patch`, validates definitions, duplicate instances, universe
overflow, and overlapping DMX ranges. It produces `ResolvedPhysicalFixture`s
and the renderer's numeric `ResolvedRig`. Collision errors retain both fixture
names, both occupied ranges, the universe, and the exact overlap.

## RigContract and RigBinding

The compiled `PortableRigContract` expresses requirements. A `RigBinding`
connects each role name to a non-empty list of patch fixture names. Phase B
validates unknown, missing, duplicate, and empty bindings, fixture existence,
and every fixture's capabilities. A fixture may intentionally appear in more
than one role in V1.

Successful bindings become `ResolvedTarget { fixtures: Vec<FixtureId> }`
entries indexed directly by the bytecode's `TargetId`.

## RuntimeImage

`RuntimeImage` owns:

- verified portable bytecode;
- numeric resolved targets;
- the renderer-ready numeric `ResolvedRig`.

`inception-runtime::Runtime` consumes this image directly. It creates the
`LightingState`, runs the VM and transition engine, and renders through the
resolved rig. Runtime execution performs no name lookup, capability checking,
collision detection, patch parsing, or role resolution. Strings retained in
the bytecode contract are debug/link metadata only and are never read by the
VM hot path.

## V1 configuration boundary

Only the rig contract syntax is part of Lux in this milestone. Fixture
definitions, patches, and bindings use validated Rust configuration structures.
This keeps the linker architecture stable without prematurely freezing three
additional textual DSLs. Parsers or serialization formats can be added later
without changing `RuntimeImage`, the VM, or the renderer.
