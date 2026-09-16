//! DMX output drivers.
//!
//! Virtual outputs plus a serial ENTTEC DMX USB Pro implementation. Drivers
//! only see numeric universes and rendered frames; Lux/runtime concepts never
//! cross this boundary.

pub mod enttec;
pub mod null;
pub mod output;
pub mod recording;

pub use enttec::{
    DmxTransport, EnttecDmxUsbPro, EnttecDmxUsbProConfig, RealDmxOutput, SerialTransport,
    TransportError,
};
pub use null::NullDmxOutput;
pub use output::DmxOutput;
pub use recording::RecordingDmxOutput;
