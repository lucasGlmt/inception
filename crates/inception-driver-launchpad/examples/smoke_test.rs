//! Manual Launchpad X smoke test. Requires a physical Launchpad X
//! connected over USB; never run by CI (see `AGENTS.md`'s testing rules
//! and item 23 of the task brief).
//!
//! Opens the device (auto-detected) and prints every semantic
//! `InputEvent` it receives — no MIDI note numbers, velocity, or channel
//! are ever printed, only what the runtime itself would see. Press
//! Ctrl-C to exit.

use std::sync::mpsc;
use std::time::Duration;

use inception_driver_launchpad::LaunchpadListener;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::channel();
    let _listener = LaunchpadListener::open(sender)?;
    println!("Launchpad X connected — press pads (Ctrl-C to exit).");

    loop {
        match receiver.recv_timeout(Duration::from_secs(3600)) {
            Ok(event) => println!("{event:?}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
