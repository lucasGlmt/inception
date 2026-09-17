//! Device-agnostic input-event model. This is the *only* vocabulary a
//! driver may hand the runtime — no MIDI note numbers, velocities,
//! channels or SysEx cross this boundary (see `AGENTS.md`'s "do not leak
//! protocol-specific types into `inception-core`" rule for drivers).
//! `InputControl` is deliberately an open-ended enum with one variant
//! today (`Pad`) so a future generic-MIDI/keyboard/OSC controller can add
//! its own variant without disturbing `Pad`-based matching elsewhere.

/// A connected input device, identified numerically. Like [`crate::FixtureId`],
/// this is an opaque handle — V1 has exactly one device kind (a Launchpad
/// X) and no configuration language for naming/selecting devices, so a
/// `DeviceId` is just whatever the owning driver assigns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub u32);

/// A physical control on a device, in device-space coordinates —
/// never in the protocol's own wire representation (e.g. a MIDI note
/// number). `x`/`y` are `1`-based; a driver is responsible for rejecting
/// or clamping anything outside the coordinate range it advertises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputControl {
    Pad { x: u8, y: u8 },
}

/// What happened to a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputAction {
    Press,
    Release,
}

/// One semantic input event, as a driver reports it and the runtime's
/// `EventRouter` matches it against compiled `EventPattern`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputEvent {
    pub device: DeviceId,
    pub control: InputControl,
    pub action: InputAction,
}
