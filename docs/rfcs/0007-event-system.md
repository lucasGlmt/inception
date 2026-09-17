# RFC 0007: interactive events — `on` and the Launchpad X

Status: implemented (V1 — Launchpad X pad press/release only; see
Non-goals).

## Summary

Lux gains its first interactive-event system, with the Novation
Launchpad X as the first real input device:

```lux
on launchpad.pad(1, 1).press {
    Front.intensity -> 100% over 200ms;
}

on launchpad.pad(1, 1).release {
    Front.intensity -> 10% over 300ms;
}

on launchpad.pad(1, 2).press {
    Front.color = red;
}
```

Pipeline: `Launchpad X --MIDI--> inception-driver-launchpad --InputEvent-->
EventRouter --FunctionId--> Vm::run_event_handler --> LightingState -->
Renderer --> DMX`. Lux source never sees a MIDI note number, velocity,
channel, or SysEx — only the semantic vocabulary above.

## Motivation

Every Lux program to date runs open-loop from `scene main`: nothing lets
a show react to anything. `docs/rfcs/0006-signal-spread.md` already named
"events, Launchpad integration" as the deliberately deferred next
milestone, and `AGENTS.md`'s own crate docs anticipate it (`inception-vm`:
"fibers/tasks and logical scheduling"; `inception-runtime`: "event
sources"; "Prefer traits/interfaces for clocks, outputs and inputs").
This RFC is that milestone.

## Design

### 1. The event model — `inception-core`

```rust
pub struct DeviceId(pub u32);
pub enum InputControl { Pad { x: u8, y: u8 } }
pub enum InputAction { Press, Release }
pub struct InputEvent { pub device: DeviceId, pub control: InputControl, pub action: InputAction }
```

Lives in `inception-core` (not a higher crate) because it's genuinely
protocol-agnostic — zero MIDI/USB content — matching that crate's "no
USB, MIDI, filesystem, networking" rule. `InputControl` is an
open-ended enum with one variant today so a future generic-MIDI/
keyboard/OSC controller can add its own variant without disturbing
`Pad`-based matching anywhere else.

### 2. Syntax — a closed grammar, not a general event-expression

```lux
on launchpad.pad(1, 1).press { ... }
```

`.press`/`.release` are bare, paren-less accessors. Lux's expression
grammar has no field-access concept at all (confirmed in
`lux-syntax::parser`: `parse_postfix_expression` only chains a method
call when `(` follows), so reusing `Expression` for the pattern would
mean inventing general dot-field-access as a language feature just to
parse this. Instead `on` gets its own fixed-shape parse function
(`Parser::parse_event_handler_decl`): `on` IDENT `.` IDENT `(` INT `,`
INT `)` `.` IDENT, then the ordinary `Block` body. `device`/`control`/
`action` stay raw `Identifier`s in the AST — validated by `lux-hir`, not
the parser, the same split `AssignStatement::attribute` already uses.
`launchpad`/`pad`/`press`/`release` are deliberately **not** reserved
keywords, matching this codebase's existing philosophy (`lux-syntax`'s
own docs: only structural keywords are reserved) — only `on` itself is.

This also makes pad coordinates *structurally* always integer literals,
so no "is this a compile-time constant" analysis is ever needed
downstream (item 4 of the task brief is satisfied entirely by parser
shape plus one `lux-hir` range check).

### 3. Semantic validation — in `lux-hir`, not `lux-typeck`

`Lowering::lower_event_handler` rejects an unknown device/control/action
word and an out-of-range pad coordinate (`1..=8`), reusing the
`HirError::new(...).with_help(...)` builder already used throughout that
file. This mirrors the existing precedent of rig-contract capability
validation (`lower_rig_contract`'s "unknown capability" diagnostic) —
both are closed-vocabulary checks on a declarative construct, not a
value/type inference rule, so `lux-typeck` never needs to know the event
vocabulary at all. `lux-typeck`'s only new job is type-checking handler
bodies exactly like scene bodies, plus one extra rule (next section).

### 4. `wait` is forbidden inside a handler

`inception-vm::Vm` is single-fiber: one `stack`/`frame`/`call_stack` per
`Vm`. There is no sound way for a handler's own `wait` to suspend
independently of whatever the entry scene's execution is doing. Per the
task brief's own explicit instruction, this is not hacked around: V1
forbids `wait` inside an `on { ... }` body, at compile time, with a
clear diagnostic (`lux-typeck`'s `check_body(... forbid_wait: true)`):

> `` `wait` is not allowed inside an event handler `` — help: "event
> handlers run to completion synchronously in V1 (only the entry scene
> may be suspended) — start a transition (`->`) or signal binding (`<-`)
> instead, which keeps running after the handler returns"

Full multi-fiber support is an explicit, separate future milestone (see
Non-goals).

Crucially, **this restriction costs nothing for the common case**: a
transition or signal binding a handler starts does *not* need the
handler itself to stay suspended. `TransitionEngine`/`SignalBindingStore`
are already sampled every runtime tick independent of VM state
(`inception-runtime::engine::LoadedProgram::advance`) — a handler that
does `Front.intensity -> 100% over 1s;` and returns immediately behaves
exactly like a scene doing the same thing, with zero VM changes needed
for that part (item 11 of the task brief).

### 5. HIR → MIR → bytecode: handlers are ordinary, non-entry functions

MIR and bytecode were already function-table based before this RFC:
`MirModule.functions: Vec<MirFunction>` / `BytecodeModule.functions:
Vec<Function>`, with `entry` just one designated `FunctionId`. A scene
that isn't named `main` is already an ordinary non-entry function in
that table. An event handler is simply *another* one:

```rust
// lux-mir
pub struct MirEventBinding { pub pattern: MirEventPattern, pub handler: FunctionId }
pub struct MirModule { .. pub event_bindings: Vec<MirEventBinding> }

// lux-bytecode
pub enum EventPattern { LaunchpadPad { x: u8, y: u8, action: EventAction } }
pub struct EventBinding { pub pattern: EventPattern, pub handler: FunctionId }
pub struct BytecodeModule { .. pub event_bindings: Vec<EventBinding> }
```

`lux-mir::lower` numbers handler `FunctionId`s right after every scene
(`handler_base = hir.scenes.len()`), lowers each handler's body
identically to `lower_scene` (locals + statements + one `BasicBlock`
ending in `Terminator::Return`), and emits one `MirEventBinding` per
handler. No bytecode-format change was needed at all — only the new
top-level `event_bindings` table, verified defensively by
`lux_bytecode::verify` (`InvalidEventHandler`, re-checking a
`FunctionId` resolves; `InvalidPadCoordinate`, re-checking `1..=8` even
though `lux-hir` already guarantees it, matching this crate's "never
trust blindly" policy).

### 6. The VM: running a handler without a second `Vm`

The task brief is explicit: reuse the existing engine, don't build a
second dedicated VM for events. The chosen design is `Vm::run_event_handler`:

```rust
pub fn run_event_handler<C: Clock>(
    &mut self,
    handler: FunctionId,
    clock: &C,
    lighting: &mut LightingState,
    transitions: &mut TransitionEngine,
) -> Result<(), VmError>
```

It shares `self.module`/`signals`/`bindings`/`sequences` — the same
state a scene's own execution already reads and writes — plus the
caller's `LightingState`/`TransitionEngine`, so `SetAttribute`/
`TransitionAttribute`/`BindSignal` inside a handler behave exactly as
they would in a scene.

**Rejected alternative**: thread an explicit `Fiber { stack, frame,
call_stack }` argument through every `exec_*` method, called as
`self.exec_foo(&mut fiber, ...)`. This is what a literal "extract a
`Fiber` type" reading of the design would suggest, but it doesn't
compile as written — `self.exec_foo(&mut self.some_field, ...)` borrows
all of `self` for the receiver while also borrowing part of it as an
argument, which the borrow checker rejects. Splitting every `exec_*`
into a free function taking `module`/`signals`/`bindings`/`sequences`/
`fiber` as separate parameters would work, but touches essentially
every method in `vm.rs` for no behavioral gain.

**What's implemented instead**: `run_event_handler` temporarily swaps
`self.stack`/`self.frame`/`self.call_stack` (the entry fiber's state)
out for a fresh, empty set via `std::mem::take`/`Option::take`, runs the
ordinary `execute_one` loop completely unchanged, then unconditionally
swaps the entry fiber's original state back before returning — on
success, on fault, or on the handler-only `Wait` backstop below. Every
existing `exec_*` method is untouched: it has no idea this happened, it
just keeps operating on "whichever execution state currently lives in
`self.stack`/`self.frame`/`self.call_stack`". This is a manual
coroutine yield/resume, achieving the same isolation guarantee (entry
fiber untouched, including mid-`WaitingUntil`) with a far smaller diff.
A real `Fiber` type — if/when true concurrent suspension is built — will
replace this swap wholesale; it isn't introduced prematurely for a V1
where every handler invocation is a single bounded, uninterruptible
call.

A `Wait` instruction reached inside a handler is rejected as
`VmErrorKind::WaitNotAllowedInHandler` — defense in depth for hand-built
bytecode; `lux-typeck` already rejects `wait` in a handler body
statically, so real compiled Lux never reaches this. A fault inside a
handler is returned to the caller, never stored on `self.state` and
never able to corrupt the entry fiber: a bad button press must not take
down the rest of the show.

### 7. `EventRouter` — `inception-runtime`

```rust
pub struct EventRouter { .. }
impl EventRouter {
    pub fn new(bindings: Vec<EventBinding>) -> Self;
    pub fn route(&self, event: InputEvent) -> Vec<FunctionId>;
}
```

Lives in `inception-runtime` — the natural boundary crossing point
between `inception_core::InputEvent` (driver-facing) and
`lux_bytecode::EventPattern` (compiler-facing). V1 matches on
`(control, action)` only, ignoring `InputEvent::device` (exactly one
device kind and instance exist); every matching binding fires, in
declaration order — deterministic, no priority system.

`LoadedProgram` owns one `EventRouter`, rebuilt fresh from
`RuntimeImage.event_bindings` on every load/reload, exactly parallel to
`vm`/`lighting`/`transitions`. `LoadedProgram::dispatch_input_event`
routes an event and runs every matched handler via
`Vm::run_event_handler` against this program's own `lighting`/
`transitions`, collecting (never propagating) faults into an
`EventDispatchReport`.

**Rejected alternative**: an `InputSource` trait mirroring `DmxOutput`,
generic over `RuntimeHost<O>`. `DmxOutput` is trait-based because output
is *pulled* once per frame; input here is *pushed* through an
`mpsc::Sender<InputEvent>` from a driver-owned thread — there's no
per-frame pull to abstract, so no generic parameter was added. Tests
inject plain `InputEvent` values directly.

**Linker**: `inception-linker::RuntimeImage.event_bindings` is a
straight `.clone()` of `BytecodeModule.event_bindings` — a pad pattern
is already fully numeric at compile time, unlike a fixture role, so
there is no physical/patch resolution step for the linker to perform.

**Hot reload needed zero new code.** `RuntimeHost::reload` already does
`candidate.start()` then one atomic `self.program = candidate` swap, and
the CLI only calls it once a rebuild is fully validated — the old
`LoadedProgram`, and the `EventRouter` bundled inside it, stay live and
functional for the entire duration of an invalid build attempt, for
free.

### 8. The driver — `inception-driver-launchpad`

`midir` (pure-Rust, cross-platform CoreMIDI/ALSA/WinMM) is the new
dependency — no MIDI crate existed anywhere in the workspace before
this. `mapping.rs` centralizes *all* MIDI note ↔ `pad(x, y)` conversion:
Programmer Mode's fixed grid numbering (note `11` = bottom-left, `88` =
top-right, `+1` per column, `+10` per hardware row, notes ending in
`9`/`0` unused — confirmed against Novation's own "Launchpad X —
Programmer's Reference Manual" worked examples); this crate's `y = 1` is
the *top* row, so `pad_to_note`/`note_to_pad` convert through
`hardware_row = 9 - y`. Note-On-velocity-0 and a genuine Note-Off both
map to `InputAction::Release`; only MIDI channel 1 (Programmer Mode's
main-grid channel) is honored.

**Programmer Mode.** The Launchpad X's default ("Live") state runs
whichever layout (Session, Note, Custom 1-4) is currently selected *on
the device itself*, each with its own note numbering and pad behavior —
not the fixed grid `mapping.rs` assumes. `LaunchpadListener::open`
therefore opens a second connection, to the device's MIDI *output*, and
sends Novation's documented "Programmer / Live mode switch" SysEx
(`F0h 00h 20h 29h 02h 0Ch 0Eh 01h F7h`) before it starts listening; its
`Drop` impl sends the inverse (`... 00h F7h`) to restore Live mode on
disconnect, exactly as that manual recommends. Without this, a real
device's note numbering (and thus `mapping.rs`'s coordinate conversion)
is only correct by accident, whenever Session layout happens to already
be selected on the hardware.

**Two MIDI interfaces.** A Launchpad X exposes *two* separate MIDI
interfaces over USB — "LPX DAW" (Session-mode DAW control) and "LPX
MIDI" (grid notes, external input, and Programmer Mode/Lighting SysEx)
— and both port names contain "Launchpad X". `is_launchpad_x_port_name`
therefore also excludes any port name containing "DAW"; without that
exclusion, `find_launchpad_x_port` would resolve two matching ports on
real hardware and always fail as `AmbiguousDevice`, even with exactly
one Launchpad X connected. `listener.rs`'s `midir` input callback still
does nothing but call the pure `translate_message(&[u8]) ->
Option<InputEvent>` and push onto a channel — never compiles, links,
renders, or runs VM code.

### 9. `lux-cli` / `lux dev`

Mirrors the existing `spawn_build_worker`/`spawn_stdin_commands` pattern
exactly: an `mpsc::channel::<InputEvent>()` is opened once, at startup,
only if the *first* successful build's `event_bindings` is non-empty; a
`LaunchpadListener` connects on a dedicated thread (or logs a non-fatal
warning if none is found); the main poll loop gains a fifth non-blocking
`try_recv()` drain, alongside the filesystem-watcher/stdin-command/
build-result channels, calling `host.dispatch_input_event` per event.
The listener handle is held for the whole `dev_command` call and is
never touched by the reload branch — a rebuild only ever calls
`host.reload(...)`, so the Launchpad connection survives every hot
reload. A program that starts with *no* handlers and gains one via a
later edit requires restarting `lux dev` once — a documented V1
limitation, not silently patched around.

### 10. `lux-lsp`

`diagnostics()` needed no new code at all: it already calls
`lux_compiler::check` directly, so every diagnostic from sections 3/4
above flows through with correct spans automatically (`AGENTS.md`'s "LSP
never duplicates language semantics" rule). Completion after `on
launchpad.` (`pad`) and `on launchpad.pad(x, y).` (`press`/`release`) is
a small textual scan (`event_header_completion`), mirroring
`import_path_segments`'s existing style rather than a real parser — good
enough while the document may be mid-edit and unparseable, exactly like
every other completion heuristic in this file. `document_symbols`/
`semantic_tokens` gained one match arm each for `Item::EventHandler`
(the compiler found every call site that needed one, since neither
function had a wildcard arm). Hovering `launchpad`/`pad`/`press`/
`release` inside an `on` header reports a short description, gated on
the hovered span exactly matching that handler's own device/control/
action span so an unrelated local named e.g. `pad` is never shadowed.

## Non-goals

Carried over from the task brief, unchanged:

- LED feedback, pad colors, LED animations on the Launchpad.
- Generic MIDI-in-Lux (knobs, faders, arbitrary CC/note mapping exposed
  to source).
- OSC, tempo/BPM sync.
- `parallel` and full multi-fiber scheduling — `wait` inside a handler
  remains a compile-time error until that milestone.
- Event priorities.
- Chords, double-tap, long-press.
- Dynamic (runtime) event subscriptions — the event table is fully
  static, resolved at compile/link time.
- A general event-expression grammar — `on` only ever parses the one
  closed `launchpad.pad(x, y).<press|release>` shape.
- Explicit device configuration — V1 is single-device auto-discovery
  only (`DeviceNotFound`/`AmbiguousDevice` are the only outcomes besides
  success).

## Testing

`inception-core` (no dedicated tests needed — plain data types).
`lux-bytecode` (`verify` rejects an out-of-range/invalid-handler
`EventBinding`). `lux-syntax` (parses the brief's exact snippet;
recovery on missing `.`/parens/non-literal coordinate/missing action).
`lux-hir` (out-of-range coordinates and unknown device/control/action
words each rejected with the documented diagnostic). `lux-typeck`
(`wait` inside a handler rejected with the documented message; `=`/`->`/
`<-` all type-check inside a handler identically to a scene). `lux-mir`
(handler `FunctionId`s correctly offset past every scene;
`event_bindings` populated in source order). `inception-vm`
(`run_event_handler` completes while the entry fiber is mid-
`WaitingUntil`, leaving it untouched; a hand-built `Wait` in a handler
is rejected; a faulting handler doesn't corrupt the entry fiber; a
handler's `SetAttribute`/`TransitionAttribute`/`BindSignal` mutates the
shared `LightingState`/`TransitionEngine`/`SignalStore`).
`inception-runtime` (`EventRouter` routing/ordering tests against
hand-built bindings; a full synthetic-`InputEvent` E2E test through
`RecordingDmxOutput`; a `VirtualClock` transition-progression test after
a handler-triggered `->`; a "stale build stays live" reload test).
`inception-driver-launchpad` (`mapping`'s full 64-pad round trip plus
bounds/gap rejection; `translate_message`'s press/release/channel-filter/
malformed-input cases; `is_launchpad_x_port_name` accepts "LPX MIDI"-style
names and rejects "LPX DAW"-style ones — no test ever opens a real MIDI
port or sends real SysEx).
`lux-compiler` (an end-to-end `compile_portable` test asserting a
non-empty, correctly-ordered `event_bindings` table; compile-pass/
compile-fail `.lux` fixtures under `tests/compiler/{pass,fail}`).
`lux-lsp` (completion at both dotted-path positions; diagnostics for an
invalid coordinate and for `wait` in a handler; hover on each header
word). None of the above require physical hardware.

A manual, non-CI hardware smoke test (`cargo run -p
inception-driver-launchpad --example smoke_test`) logs every semantic
`InputEvent` while physically pressing a connected Launchpad X's pads —
never printing a raw MIDI byte. A full `lux dev` walkthrough with this
RFC's own `Summary` example, run against real DMX output and a real
Launchpad X, is the final manual acceptance check: pressing pad `(1,
1)` should fade the fixture up over 200ms, releasing it should fade
back down over 300ms, and editing/saving the `.lux` file should
hot-reload without dropping the MIDI connection.
