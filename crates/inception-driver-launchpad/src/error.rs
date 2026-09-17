//! Structured driver errors — mirrors `inception_driver_dmx::TransportError`'s
//! style for the output side.

use std::fmt;

#[derive(Debug)]
pub enum LaunchpadError {
    /// Failed to initialize the MIDI input backend itself, before any
    /// port was even listed.
    Init(midir::InitError),
    /// No MIDI input port whose name looks like a Launchpad X was found.
    DeviceNotFound,
    /// More than one candidate port was found; V1 has no configuration
    /// language to disambiguate (item 14 of the task brief).
    AmbiguousDevice(Vec<String>),
    /// The port was found but the connection itself failed.
    Connect(String),
    /// Connected, but switching the device into (or back out of)
    /// Programmer Mode failed — see `listener`'s module doc for why this
    /// step is required at all.
    ProgrammerMode(String),
}

impl fmt::Display for LaunchpadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchpadError::Init(error) => {
                write!(formatter, "failed to initialize MIDI input: {error}")
            }
            LaunchpadError::DeviceNotFound => {
                write!(
                    formatter,
                    "no Launchpad X found — connect one and try again"
                )
            }
            LaunchpadError::AmbiguousDevice(names) => write!(
                formatter,
                "multiple Launchpad X-like MIDI inputs found ({}); connect only one",
                names.join(", ")
            ),
            LaunchpadError::Connect(message) => {
                write!(formatter, "failed to connect to Launchpad X: {message}")
            }
            LaunchpadError::ProgrammerMode(message) => {
                write!(
                    formatter,
                    "failed to switch the Launchpad X into Programmer Mode: {message}"
                )
            }
        }
    }
}

impl std::error::Error for LaunchpadError {}
