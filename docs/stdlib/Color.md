# `std.Color`

```lux
import std.Color;
```

Color composition: build colors from channels, blend them, or build from
hue/saturation/value. **Every function in this module is pure** — no
clock, IO, randomness, DMX or mutable global state.

The named color literals `red`, `green`, `blue`, `white`, `black` are
unaffected by this module — they remain plain literal keywords, usable
anywhere a `Color` is expected, including as arguments to `Color.mix`.

## `rgb`

```lux
rgb(r: Int, g: Int, b: Int) -> Color
```

Builds a color from three channels, each in `0..=255`.

**Bounds**: this is the one convention `std.Color` uses for channel
values — not `0..=1.0`, not `0..=100%`. When a channel argument is a
literal constant outside `0..=255`, this is a **compile-time error**
(`Color.rgb(255, 300, 0)` → `color channel \`300\` is out of range
0..=255`). When a channel isn't a compile-time constant, an out-of-range
value **saturates** to `0`/`255` at runtime rather than erroring — the
function stays total.

- `Color.rgb(0, 0, 0)` — black
- `Color.rgb(255, 255, 255)` — white
- `Color.rgb(255, 120, 20)` — orange

## `mix`

```lux
mix(a: Color, b: Color, ratio: Float) -> Color
```

Blends `a` and `b`. `ratio == 0.0` returns exactly `a`; `ratio == 1.0`
returns exactly `b`; values in between blend each channel linearly,
rounded to the nearest integer channel value.

**Edge case — `ratio` is clamped, never an error.** Unlike
`Math.lerp`'s `t`, `mix`'s `ratio` is clamped to `[0.0, 1.0]` before
blending — chosen for consistency with `Color.rgb`'s and `Math.clamp`'s
"saturate a non-constant value, never trap" policy elsewhere in this
module. A `ratio` outside `[0.0, 1.0]` therefore just returns `a` or `b`
exactly, rather than an out-of-gamut extrapolated color.

- `Color.mix(red, blue, 0.0)` — exactly `red`
- `Color.mix(red, blue, 0.5)` — `rgb(128, 0, 128)`
- `Color.mix(red, blue, 1.0)` — exactly `blue`

## `hsv`

```lux
hsv(hue: Angle, saturation: Intensity, value: Intensity) -> Color
```

Standard HSV → RGB conversion. `saturation` and `value` use the same
`Intensity` type (and the same `0%..=100%` literal syntax) as any other
percentage in Lux.

**Edge case — `hue` wraps.** `hue` is taken modulo 360deg before
conversion, so a negative or `>360deg` angle behaves the same as its
canonical `0..360` equivalent, rather than being an error.

Primary colors at full saturation/value:
- `Color.hsv(0deg, 100%, 100%)` — red
- `Color.hsv(120deg, 100%, 100%)` — green
- `Color.hsv(240deg, 100%, 100%)` — blue
