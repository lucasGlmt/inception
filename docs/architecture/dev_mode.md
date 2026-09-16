# Lux project model and dev mode

## Project model

A Lux project is rooted by `lux.toml`. Commands begin in the current
directory and walk through its parents until they find that file. Discovery
stops at the filesystem root. Once loaded, every configured path is resolved
relative to the directory containing `lux.toml`; later changes to the process
working directory have no effect.

The V1 manifest is intentionally small:

```toml
[project]
name = "demo-show"

[source]
entry = "src/main.lux"

[rig]
patch = "rig/patch.lux"
bindings = "rig/rig.lux"

[fixtures]
directory = "fixtures"

[runtime]
frequency = 40

[output]
driver = "null" # null, recording/dev, or dmx/enttec
# device = "devices/tty.usbserial" # required for dmx/enttec
# universe = 1
```

`rig.patch` and `rig.bindings` contain TOML configuration in V1, despite the
`.lux` extension used by the current project convention. A patch contains a
`[patch]` table and zero or more `[[fixtures]]` entries. A binding file has a
`[rig]` table and zero or more `[[bindings]]` entries. Fixture definitions are
individual `*.toml` files under `fixtures.directory`:

```toml
[fixture]
name = "RGB Dimmer"
footprint = 4
capabilities = ["Intensity", "Color"]

[mapping]
intensity = 0
red = 1
green = 2
blue = 3
```

This filesystem/configuration work belongs to `lux-project`. `lux-compiler`
continues to accept in-memory source and contains the single implementation of
parse, resolve, typecheck, MIR lowering, bytecode generation, and verification.

## Dev mode

```text
Filesystem
    ↓
Watcher
    ↓
Compiler + Linker
    ↓
Candidate RuntimeImage
    ↓ validate
Atomic Program Swap
    ↓
RuntimeHost
    ↓
DMX hardware
```

`RuntimeHost` owns long-lived resources: the DMX output connection, output
buffers, universe history, and frame count. The runtime loop and its monotonic
schedule also remain outside the replaceable program. `LoadedProgram` owns one
build's verified VM, semantic `LightingState`, active transitions, resolved
rig, and stable fixture metadata.

The filesystem watcher uses the operating-system-backed `notify` crate and
watches the project root recursively. It filters events to `lux.toml`, the
configured entry/rig/fixture paths, and `src/**/*.lux`. A 100 ms trailing-edge
debounce sorts and deduplicates each event burst. A capacity-one rebuild queue
coalesces changes during a build: the current build finishes and at most one
dirty build follows it. Compilation and linking run on a worker; they never run
while holding or mutating the current program.

## Transactional reload

A candidate passes manifest/config loading, parsing, resolution, typechecking,
bytecode generation and verification, linking, and VM construction before the
runtime sees it. The short commit operation then:

1. prepares the new VM;
2. samples old transitions at the current monotonic timestamp;
3. copies compatible effective fixture state;
4. replaces the old `LoadedProgram` in one assignment.

Old VM frames, locals, call stack, and transitions are deliberately discarded.
The new VM starts from its entry point. The old program is dropped after the
swap and is not retained by a reload history.

> Compilation errors never replace the last valid running program.

Link and runtime-candidate validation errors follow the same rule. The runtime
loop and DMX connection continue to operate with the previous build, and a
blackout command remains available independently of build success.

## State preservation

The linker records each physical fixture's patch name next to its resolved
numeric mapping. Reload joins old and new fixtures by that stable name, never
by `FixtureId`, because relinking may reorder numeric IDs.

- A stable fixture preserves intensity only when both mappings support
  intensity, and preserves color only when both support color.
- A new or incompatible fixture starts at the explicit semantic defaults
  (zero intensity and black).
- A removed fixture is absent from the new render. `RuntimeHost` retains every
  universe it has seen and starts each frame black, so removed DMX channels are
  actively cleared.
- Active transitions are sampled immediately before the swap, their effective
  values are migrated, and the transitions themselves are cleared.

Changing fixture order or numeric IDs is therefore safe. Renaming a fixture is
treated as removing one fixture and adding another.

## V1 configuration reload limits

Source, patch, binding, and fixture changes can hot reload. A runtime frequency
change or output configuration change is validated but rejected with a
`restart required` message; the last build remains active. V1 does not migrate
locals, patch individual functions, incrementally compile, or hot-swap a
hardware output device.
