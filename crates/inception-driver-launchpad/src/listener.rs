//! Real Launchpad X input: MIDI raw -> semantic `InputEvent`. Everything
//! above this module — the runtime, `EventRouter`, Lux itself — never
//! sees a note number, velocity, MIDI channel or SysEx (items 12/17/18
//! of the task brief; see AGENTS.md's "do not leak protocol-specific
//! types into `inception-core`" driver rule).
//!
//! ## Programmer Mode
//!
//! The Launchpad X's default ("Live") state runs whatever layout is
//! currently selected on the device itself (Session, Note, Custom 1-4),
//! each with its own note numbering and pad behavior — not the fixed,
//! predictable 8x8 grid `crate::mapping` assumes. **Programmer Mode** is
//! the mode Novation's own documentation recommends for exactly this use
//! case (a host application driving the grid directly): entering it
//! fixes the note numbering to the standard layout (`11..=88`, see
//! `mapping`'s docs) regardless of whatever layout was selected on the
//! device, and is switched with a dedicated SysEx message (Novation,
//! "Launchpad X — Programmer's Reference Manual", "Programmer / Live
//! mode switch": `F0h 00h 20h 29h 02h 0Ch 0Eh <mode> F7h`, mode `01h` for
//! Programmer, `00h` for Live). `LaunchpadListener::open` sends the
//! Programmer variant on connect; its `Drop` impl sends the Live variant
//! back on disconnect, exactly as that manual recommends ("Remember to
//! switch the device back to Live mode once done").
//!
//! ## Two MIDI interfaces
//!
//! A Launchpad X exposes *two* MIDI interfaces over USB: "LPX DAW" (used
//! by DAW software to drive Session mode) and "LPX MIDI" (grid notes,
//! external MIDI input, and — critically — Programmer Mode/Lighting
//! SysEx). Both port names contain "Launchpad X" on every backend this
//! crate has been checked against, so [`is_launchpad_x_port_name`] must
//! also exclude the DAW interface, or a real device would always resolve
//! as [`LaunchpadError::AmbiguousDevice`] instead of a single match.

use std::sync::mpsc::Sender;

use inception_core::{DeviceId, InputAction, InputControl, InputEvent};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection, PortInfoError};

use crate::error::LaunchpadError;
use crate::mapping::note_to_pad;

/// V1 supports exactly one device kind and exactly one connected
/// instance (item 14 of the task brief), so this is a fixed constant
/// rather than anything discovered per-connection. A `device` field will
/// only need to vary once a second kind of device exists.
pub const LAUNCHPAD_DEVICE_ID: DeviceId = DeviceId(1);

/// `F0h 00h 20h 29h 02h 0Ch 0Eh 01h F7h` — see this module's docs.
const ENTER_PROGRAMMER_MODE: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7];
/// Same message, mode byte `00h` (Live) — sent on disconnect.
const ENTER_LIVE_MODE: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x00, 0xF7];

/// A live connection to a Launchpad X's MIDI input, already switched
/// into Programmer Mode. Dropping this switches the device back to Live
/// mode and closes both the input and output connections.
pub struct LaunchpadListener {
    output: MidiOutputConnection,
    _input: MidiInputConnection<()>,
}

impl LaunchpadListener {
    /// Auto-detects a single connected Launchpad X, switches it into
    /// Programmer Mode (see this module's docs), and starts listening.
    /// Errors clearly if none is found, or if more than one candidate
    /// port makes the choice ambiguous (item 14).
    ///
    /// Every received MIDI message is translated to a semantic
    /// `InputEvent` and pushed onto `sender` from `midir`'s own callback
    /// thread. The callback does nothing else — never compiles, links,
    /// renders or runs VM code (items 17/18): it stays a pure, cheap
    /// translation followed by a non-blocking channel send.
    pub fn open(sender: Sender<InputEvent>) -> Result<Self, LaunchpadError> {
        let midi_out =
            MidiOutput::new("inception-driver-launchpad").map_err(LaunchpadError::Init)?;
        let output_port = find_launchpad_x_port(midi_out.ports(), |port| midi_out.port_name(port))?;
        let mut output = midi_out
            .connect(&output_port, "inception-launchpad-output")
            .map_err(|error| LaunchpadError::Connect(error.to_string()))?;
        output
            .send(&ENTER_PROGRAMMER_MODE)
            .map_err(|error| LaunchpadError::ProgrammerMode(error.to_string()))?;

        let midi_in = MidiInput::new("inception-driver-launchpad").map_err(LaunchpadError::Init)?;
        let input_port = find_launchpad_x_port(midi_in.ports(), |port| midi_in.port_name(port))?;
        let input = midi_in
            .connect(
                &input_port,
                "inception-launchpad-input",
                move |_timestamp_us, message, _| {
                    if let Some(event) = translate_message(message) {
                        // A full/disconnected channel means the runtime
                        // isn't draining (or no longer exists); dropping
                        // the event here rather than blocking is the
                        // right tradeoff for a hardware callback thread.
                        let _ = sender.send(event);
                    }
                },
                (),
            )
            .map_err(|error| LaunchpadError::Connect(error.to_string()))?;

        Ok(Self {
            output,
            _input: input,
        })
    }
}

impl Drop for LaunchpadListener {
    fn drop(&mut self) {
        // Best-effort: a device that's already gone (e.g. unplugged)
        // can't be told anything, and there's no one left here to report
        // a failure to.
        let _ = self.output.send(&ENTER_LIVE_MODE);
    }
}

/// Finds the single MIDI port (input or output — `ports`/`port_name`
/// generalize over `midir`'s separate `MidiInputPort`/`MidiOutputPort`
/// types) whose name identifies it as a Launchpad X's "LPX MIDI"
/// interface, never its "LPX DAW" interface (see this module's docs).
fn find_launchpad_x_port<P>(
    ports: Vec<P>,
    port_name: impl Fn(&P) -> Result<String, PortInfoError>,
) -> Result<P, LaunchpadError> {
    let mut candidates: Vec<(P, String)> = ports
        .into_iter()
        .filter_map(|port| {
            port_name(&port)
                .ok()
                .filter(|name| is_launchpad_x_port_name(name))
                .map(|name| (port, name))
        })
        .collect();

    match candidates.len() {
        0 => Err(LaunchpadError::DeviceNotFound),
        1 => Ok(candidates.pop().expect("checked len == 1").0),
        _ => Err(LaunchpadError::AmbiguousDevice(
            candidates.into_iter().map(|(_, name)| name).collect(),
        )),
    }
}

/// The "LPX MIDI" interface only — excludes "LPX DAW", which also
/// contains "Launchpad X" in its port name but carries Session-mode DAW
/// control, not grid notes or Programmer Mode SysEx.
fn is_launchpad_x_port_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("launchpad x") && !lower.contains("daw")
}

/// Raw MIDI -> semantic `InputEvent`. Self-contained (no receiver state)
/// so it's unit-testable without a real MIDI connection. Handles both a
/// genuine Note Off and the common "Note On, velocity 0" running-status
/// convention as `Release` (item 16) — never synthesizes an extra event
/// per raw message, and never reaches the runtime for anything outside
/// the main 8x8 grid (top-row/side buttons, SysEx, CC messages).
fn translate_message(message: &[u8]) -> Option<InputEvent> {
    let &[status, note, velocity] = message else {
        return None;
    };
    // Programmer Mode sends the main grid on MIDI channel 1 (channel
    // nibble 0) — see this module's docs.
    if status & 0x0F != 0 {
        return None;
    }
    let (x, y) = note_to_pad(note)?;
    let action = match status & 0xF0 {
        0x90 if velocity > 0 => InputAction::Press,
        0x90 | 0x80 => InputAction::Release,
        _ => return None,
    };
    Some(InputEvent {
        device: LAUNCHPAD_DEVICE_ID,
        control: InputControl::Pad { x, y },
        action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_with_velocity_is_a_press() {
        let event = translate_message(&[0x90, 81, 127]).unwrap();
        assert_eq!(event.action, InputAction::Press);
        assert_eq!(event.control, InputControl::Pad { x: 1, y: 1 });
        assert_eq!(event.device, LAUNCHPAD_DEVICE_ID);
    }

    #[test]
    fn note_on_with_zero_velocity_is_a_release() {
        let event = translate_message(&[0x90, 81, 0]).unwrap();
        assert_eq!(event.action, InputAction::Release);
    }

    #[test]
    fn note_off_is_a_release() {
        let event = translate_message(&[0x80, 81, 64]).unwrap();
        assert_eq!(event.action, InputAction::Release);
    }

    #[test]
    fn a_message_on_a_non_zero_channel_is_ignored() {
        assert_eq!(translate_message(&[0x91, 81, 127]), None);
    }

    #[test]
    fn a_note_outside_the_main_grid_is_ignored() {
        assert_eq!(translate_message(&[0x90, 19, 127]), None);
        assert_eq!(translate_message(&[0x90, 104, 127]), None);
    }

    #[test]
    fn a_malformed_message_does_not_panic() {
        assert_eq!(translate_message(&[]), None);
        assert_eq!(translate_message(&[0x90]), None);
        assert_eq!(translate_message(&[0x90, 81]), None);
        assert_eq!(translate_message(&[0x90, 81, 0, 0]), None);
    }

    #[test]
    fn an_unrelated_status_byte_is_ignored() {
        // Control Change (e.g. top-row/side buttons) — out of scope for V1.
        assert_eq!(translate_message(&[0xB0, 91, 127]), None);
    }

    #[test]
    fn launchpad_x_midi_port_is_recognized_but_not_the_daw_port() {
        assert!(is_launchpad_x_port_name("Launchpad X LPX MIDI"));
        assert!(is_launchpad_x_port_name("launchpad x"));
        assert!(!is_launchpad_x_port_name("Launchpad X LPX DAW"));
        assert!(!is_launchpad_x_port_name("Launchpad X DAW"));
        assert!(!is_launchpad_x_port_name("Some Other Device"));
    }
}
