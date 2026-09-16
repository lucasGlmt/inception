//! DMX output drivers.
//!
//! Virtual outputs plus two serial implementations: the ENTTEC DMX USB Pro
//! protocol and raw Open DMX USB. Drivers only see numeric universes and
//! rendered frames; Lux/runtime concepts never cross this boundary.

pub mod enttec;
pub mod null;
pub mod open_dmx;
pub mod output;
pub mod recording;
mod transport;

pub use enttec::{EnttecDmxUsbPro, EnttecDmxUsbProConfig, RealDmxOutput, SerialTransport};
pub use null::NullDmxOutput;
pub use open_dmx::{OpenDmxConfig, OpenDmxOutput, OpenDmxTransport, RealOpenDmxOutput};
pub use output::DmxOutput;
pub use recording::RecordingDmxOutput;
pub use transport::{DmxTransport, TransportError};
