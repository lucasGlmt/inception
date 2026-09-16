# `std.Math`

```lux
import std.Math;
```

Pure scalar math: trigonometry, min/max/abs/clamp, linear interpolation.
**Every function in this module is pure** — no clock, IO, randomness, DMX
or mutable global state. Same input always produces the same output,
including across different runs and different machines (see
`inception-vm/src/intrinsic.rs` for the exact, deterministic
implementation of each function below).

## `sin`

```lux
sin(angle: Angle) -> Float
```

Sine of `angle`. Converts internally from millidegrees to radians before
calling the standard `f64::sin`.

- `Math.sin(0deg) == 0.0`
- `Math.sin(90deg) == 1.0` (within float precision)
- `Math.sin(180deg) == 0.0` (within float precision)

## `cos`

```lux
cos(angle: Angle) -> Float
```

Cosine of `angle`, same conversion as `sin`.

- `Math.cos(0deg) == 1.0`
- `Math.cos(90deg) == 0.0` (within float precision)

## `abs`

```lux
abs(value: Int) -> Int
abs(value: Float) -> Float
```

Absolute value. Two overloads, resolved by argument type — no implicit
Int/Float conversion.

**Edge case**: `abs(Int)` uses saturating arithmetic
(`i64::saturating_abs`), not plain `abs`, so `Math.abs(i64::MIN)` returns
`i64::MAX` instead of panicking (`i64::MIN`'s magnitude doesn't fit in an
`i64`). This is the one place in `std.Math` where an extreme input
produces a saturated rather than mathematically exact result — documented
here rather than treated as an error, since the function must remain
total.

## `min` / `max`

```lux
min(a: Int, b: Int) -> Int
min(a: Float, b: Float) -> Float
max(a: Int, b: Int) -> Int
max(a: Float, b: Float) -> Float
```

The smaller/larger of the two arguments. Same overload rule as `abs`.

- `Math.min(10, 20) == 10`
- `Math.max(0.2, 0.8) == 0.8`

## `clamp`

```lux
clamp(value: Int, min: Int, max: Int) -> Int
clamp(value: Float, min: Float, max: Float) -> Float
```

Restricts `value` to `[min, max]`: `min <= result <= max` whenever
`min <= max`. Computed as `value.max(min).min(max)`, in that fixed order.

**Edge case — `min > max`**: this is *not* silently swapped. The fixed
evaluation order above means `clamp` deterministically returns `max` in
this case. A compile-time diagnostic catches this specific mistake when
`value`, `min` and `max` are all literal constants
(`Math.clamp(1, 5, 2)` → `clamp's min (5) is greater than its max (2);
this always returns 2`); for non-constant arguments where this can't be
checked ahead of time, the documented `max`-returning behavior applies
with no runtime error.

Examples:
- below range: `Math.clamp(-5, 0, 10) == 0`
- inside range: `Math.clamp(5, 0, 10) == 5`
- above range: `Math.clamp(15, 0, 10) == 10`
- `min > max`: `Math.clamp(1, 5, 2) == 2`

## `lerp`

```lux
lerp(a: Float, b: Float, t: Float) -> Float
```

Linear interpolation: `a + (b - a) * t`.

**Edge case — `t` is not clamped.** `t` outside `[0.0, 1.0]`
extrapolates past `a`/`b` rather than being restricted to the segment
between them. Use `Math.clamp(t, 0.0, 1.0)` first if a clamped
interpolation is what you want.

- `Math.lerp(0.0, 10.0, 0.0) == 0.0`
- `Math.lerp(0.0, 10.0, 0.5) == 5.0`
- `Math.lerp(0.0, 10.0, 1.0) == 10.0`
- `Math.lerp(0.0, 10.0, 1.5) == 15.0` (extrapolated)
