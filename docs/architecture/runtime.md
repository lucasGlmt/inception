# Real-time runtime and DMX output

## Boundary

The linked `RuntimeImage` is the only show/rig input consumed by the runtime:

```text
RuntimeImage
    ↓
RuntimeEngine
    ├─ VM
    ├─ TransitionEngine
    ├─ LightingState
    └─ Renderer
    ↓
UniverseFrame[] (UniverseId ascending)
    ↓
DmxOutput
    ↓
virtual output or hardware driver
```

`RuntimeEngine` owns semantic execution. One `tick(now)` resumes the VM,
samples transitions at exactly `now`, renders the already-resolved rig, and
sends one frame for every active universe. The renderer and output driver do
not know about Lux, roles, transitions, fixture names, or patch configuration.

`RuntimeLoop` owns timing only. Separating it from the engine makes the full
logical pipeline testable with `VirtualClock` and `RecordingDmxOutput`, without
sleeping or attaching hardware.

## Time and scheduling

Lighting semantics are time-based. Output cadence is only a sampling
frequency.

The default output frequency is 40 Hz (25 ms), configurable from 1 through
1000 Hz. The scheduler establishes an origin and derives absolute deadlines:

```text
T0, T0 + period, T0 + 2 × period, ...
```

It never sleeps for `period` after completing work. Processing time therefore
does not accumulate into deadline drift. Production uses `MonotonicClock`,
which is based on `std::time::Instant`; civil time and clock adjustments cannot
affect a show.

If execution wakes after its deadline, the engine renders once at the actual
current timestamp. The scheduler then advances directly to the first future
absolute deadline. It does not replay missed frames. This prevents catch-up
bursts and preserves the absolute-time semantics of transitions.

`RuntimeTimingStats` exposes rendered frame count, late-frame count, maximum
lateness, and runtime duration. `RuntimeEngine::frames_sent` separately counts
successful universe writes.

## Startup and shutdown

Starting the loop immediately executes and renders the program at `T0`. A
zero-valued initial state therefore sends an explicit black frame instead of
depending on the previous controller state.

Normal stop sends a final black frame to every active universe in ascending
order and then closes the output. Closing is attempted even if a blackout
write fails. Hardware errors remain structured as send, blackout, or close
failures and are never hidden.

`RuntimeLoop::run_while` accepts a caller-owned stop predicate (for example an
atomic flag set by signal handling), then applies that normal shutdown policy.

## Outputs and deterministic ordering

`DmxOutput` has only `send(universe, frame)` and `close()`. The same engine runs
with:

- `NullDmxOutput` for headless execution and benchmarks;
- `RecordingDmxOutput` for deterministic tests;
- `RealDmxOutput` for an ENTTEC DMX USB Pro serial interface.

Universe IDs are collected once from the resolved rig and sorted ascending.
No `HashMap` iteration order can affect physical send order.

## ENTTEC DMX USB Pro driver

The first real driver uses the documented ENTTEC USB Pro framed serial
protocol at 57,600 baud. It accepts an explicit device path and one
`UniverseId`, encodes label `6` packets with a DMX start code plus 512 slots,
and reuses its 518-byte packet buffer. A replaceable `DmxTransport` keeps packet
encoding testable without a device.

Errors distinguish device-not-found, permission/open failure, timeout,
disconnect, write failure, close/flush failure, and a universe that does not
match the configured device. The protocol is output-only here, so there is no
response to parse or validate. One USB Pro instance exposes one universe; a
multi-device aggregate output is intentionally left for a later milestone.

Hardware selection belongs to the caller (eventually the CLI): instantiate
`RuntimeEngine<NullDmxOutput>` for virtual mode or
`RuntimeEngine<RealDmxOutput>` for the serial device. The runtime core contains
no hardware branch.

## Manual hardware smoke test

Only run this against a channel whose fixture meaning is known and safe. The
tool deliberately caps the requested value at 32, holds it for 500 ms, then
blackouts and closes the device:

```bash
cargo run -p inception-driver-dmx --example dmx_smoke_test -- \
  /dev/cu.usbserial-XXXXXXXX 1 16
```

This is a manual tool and is not part of CI. If the first write fails, it still
attempts blackout and close.

## Performance baseline

The dependency-free baseline measures logical `sample + render + null output`
for 1 universe/100 fixtures, 4 universes/500 fixtures, and 10 universes/1000
fixtures. It does not benchmark sleeping or hardware I/O:

```bash
cargo run --release -p inception-runtime --example runtime_benchmark
```

The goal is to remain comfortably below the 25 ms budget at 40 Hz, not to
micro-optimize the linker or semantic model.
