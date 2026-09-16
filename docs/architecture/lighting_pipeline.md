# Lighting pipeline (V1)

The first end-to-end lighting pipeline: a Lux program can assign a
semantic attribute and produce concrete DMX bytes, with no hardware and
no fixture DSL/linker yet. This is a summary — the authoritative
documentation lives as rustdoc on `inception_core::lighting_state`,
`inception_renderer` (crate-level docs), and `inception_driver_dmx`.

```text
VM (inception-vm)
 ↓  SET_ATTRIBUTE -> LightingState::set_target_attribute
LightingState (inception-core)         — semantic, no DMX knowledge
 ↓  render(state, &ResolvedRig, &mut frames)
Renderer (inception-renderer)
 ↓
UniverseFrame (one per universe)
 ↓
DmxOutput (inception-driver-dmx)       — Null or Recording, no hardware
```

## Semantic lighting state

`inception_core::LightingState` holds the *current commanded value* per
fixture attribute — `Intensity` (`0..=65535` internally, `0%..=100%` at
the edges) and `Color` (`Rgb`, `u16` per channel). It knows only
`FixtureId`/`TargetId`; it has never heard of a DMX channel. A fixture
that was never set reads back as `Intensity::ZERO`/`Rgb::BLACK` — an
explicit default, not an "uninitialized" state.

`set_target_attribute` expands a `TargetId` to every `FixtureId` its
`ResolvedTarget` names (declared once via `define_target`) and applies
the value to each. There is no rig/patch/linker yet: `ResolvedTarget`s
are built by hand (in tests, or wherever a caller sets one up), the same
temporary arrangement `lux_hir::TargetEnvironment` uses on the compiler
side for target *names* — see that type's docs.

## Resolved physical mapping

`inception_renderer::ResolvedRig` is a `Vec<ResolvedFixture>`, each
mapping a fixture's `intensity`/`color` to a `DmxChannelMapping` /
`RgbChannelMapping` (a `UniverseId` plus one or three `DmxChannel`s).
Also hand-built for now — `inception-linker` is meant to produce this
later without the renderer's API needing to change. The renderer never
parses, resolves names, or validates for DMX address collisions (that's
the linker's job); it only reads an already-resolved mapping.

`DmxChannel` is the one place the DMX `1..=512` convention exists;
everywhere else (including `UniverseFrame`'s `[u8; 512]` buffer) is
plain `0`-based.

## Renderer

`render(state, rig, &mut HashMap<UniverseId, UniverseFrame>)` walks
`rig.fixtures` in order, reads each mapped attribute from `state`,
converts it to DMX8, and writes it into the right frame slot. Pure and
deterministic: same `state` + `rig` always produces the same bytes, and
the output `HashMap`'s iteration order never matters since every write is
a keyed lookup driven by the rig's fixed `Vec` order.

### `Intensity`/`Rgb` channel -> DMX8

Integer-only round-to-nearest: `((value as u32 + 128) >> 8).min(255)`.
`0 -> 0`, `65535 -> 255`, `50% (32767) -> 128`.

## DMX output

`inception_driver_dmx::DmxOutput` is a trait (`send(universe, &frame)`).
`NullDmxOutput` discards everything; `RecordingDmxOutput` remembers the
last frame per universe (`last_frame(universe)`), which is how tests
observe rendered output without hardware. No real driver (Enttec,
Art-Net, sACN, ...) exists yet.
