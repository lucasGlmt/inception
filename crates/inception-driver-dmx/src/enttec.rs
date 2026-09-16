//! ENTTEC DMX USB Pro output protocol over a replaceable byte transport.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use inception_core::UniverseId;
use inception_renderer::UniverseFrame;

use crate::DmxOutput;

const START_MESSAGE: u8 = 0x7e;
const SEND_DMX_LABEL: u8 = 6;
const END_MESSAGE: u8 = 0xe7;
const DMX_PAYLOAD_LENGTH: usize = 513;
const PACKET_LENGTH: usize = 518;

#[derive(Debug)]
pub enum TransportError {
    DeviceNotFound {
        path: PathBuf,
    },
    OpenFailed {
        path: PathBuf,
        source: serialport::Error,
    },
    PermissionDenied {
        path: PathBuf,
    },
    WriteFailed(std::io::Error),
    Timeout,
    Disconnected,
    CloseFailed(std::io::Error),
    UnsupportedUniverse {
        configured: UniverseId,
        requested: UniverseId,
    },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceNotFound { path } => {
                write!(formatter, "DMX device not found: {}", path.display())
            }
            Self::OpenFailed { path, source } => {
                write!(
                    formatter,
                    "failed to open DMX device {}: {source}",
                    path.display()
                )
            }
            Self::PermissionDenied { path } => {
                write!(
                    formatter,
                    "permission denied opening DMX device {}",
                    path.display()
                )
            }
            Self::WriteFailed(error) => write!(formatter, "DMX write failed: {error}"),
            Self::Timeout => write!(formatter, "DMX write timed out"),
            Self::Disconnected => write!(formatter, "DMX device disconnected"),
            Self::CloseFailed(error) => write!(formatter, "failed to flush DMX device: {error}"),
            Self::UnsupportedUniverse {
                configured,
                requested,
            } => write!(
                formatter,
                "DMX device is configured for universe {}, not {}",
                configured.0, requested.0
            ),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenFailed { source, .. } => Some(source),
            Self::WriteFailed(source) | Self::CloseFailed(source) => Some(source),
            _ => None,
        }
    }
}

pub trait DmxTransport {
    fn write_packet(&mut self, packet: &[u8]) -> Result<(), TransportError>;

    fn close(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl std::fmt::Debug for SerialTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SerialTransport")
            .finish_non_exhaustive()
    }
}

impl SerialTransport {
    pub fn open(path: &Path, timeout: Duration) -> Result<Self, TransportError> {
        if !path.exists() {
            return Err(TransportError::DeviceNotFound {
                path: path.to_path_buf(),
            });
        }
        let port = serialport::new(path.to_string_lossy(), 57_600)
            .timeout(timeout)
            .data_bits(serialport::DataBits::Eight)
            .flow_control(serialport::FlowControl::None)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .open()
            .map_err(|source| {
                if matches!(
                    source.kind(),
                    serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)
                ) {
                    TransportError::PermissionDenied {
                        path: path.to_path_buf(),
                    }
                } else {
                    TransportError::OpenFailed {
                        path: path.to_path_buf(),
                        source,
                    }
                }
            })?;
        Ok(Self { port })
    }
}

impl DmxTransport for SerialTransport {
    fn write_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        self.port.write_all(packet).map_err(classify_write_error)
    }

    fn close(&mut self) -> Result<(), TransportError> {
        self.port.flush().map_err(TransportError::CloseFailed)
    }
}

fn classify_write_error(error: std::io::Error) -> TransportError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => TransportError::Timeout,
        std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::UnexpectedEof => TransportError::Disconnected,
        _ => TransportError::WriteFailed(error),
    }
}

#[derive(Debug, Clone)]
pub struct EnttecDmxUsbProConfig {
    pub device_path: PathBuf,
    pub universe: UniverseId,
    pub timeout: Duration,
}

impl EnttecDmxUsbProConfig {
    pub fn new(device_path: impl Into<PathBuf>, universe: UniverseId) -> Self {
        Self {
            device_path: device_path.into(),
            universe,
            timeout: Duration::from_millis(100),
        }
    }
}

#[derive(Debug)]
pub struct EnttecDmxUsbPro<T> {
    transport: T,
    universe: UniverseId,
    packet: [u8; PACKET_LENGTH],
}

pub type RealDmxOutput = EnttecDmxUsbPro<SerialTransport>;

impl EnttecDmxUsbPro<SerialTransport> {
    pub fn open(config: EnttecDmxUsbProConfig) -> Result<Self, TransportError> {
        let transport = SerialTransport::open(&config.device_path, config.timeout)?;
        Ok(Self::with_transport(transport, config.universe))
    }
}

impl<T> EnttecDmxUsbPro<T> {
    pub fn with_transport(transport: T, universe: UniverseId) -> Self {
        let mut packet = [0; PACKET_LENGTH];
        packet[0] = START_MESSAGE;
        packet[1] = SEND_DMX_LABEL;
        packet[2] = (DMX_PAYLOAD_LENGTH & 0xff) as u8;
        packet[3] = (DMX_PAYLOAD_LENGTH >> 8) as u8;
        packet[4] = 0; // DMX start code
        packet[PACKET_LENGTH - 1] = END_MESSAGE;
        Self {
            transport,
            universe,
            packet,
        }
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: DmxTransport> DmxOutput for EnttecDmxUsbPro<T> {
    type Error = TransportError;

    fn send(&mut self, universe: UniverseId, frame: &UniverseFrame) -> Result<(), Self::Error> {
        if universe != self.universe {
            return Err(TransportError::UnsupportedUniverse {
                configured: self.universe,
                requested: universe,
            });
        }
        self.packet[5..517].copy_from_slice(frame.as_slice());
        self.transport.write_packet(&self.packet)
    }

    fn close(&mut self) -> Result<(), Self::Error> {
        self.transport.close()
    }
}

#[cfg(test)]
mod tests {
    use inception_renderer::DmxChannel;

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingTransport {
        packets: Vec<Vec<u8>>,
        closed: bool,
    }

    impl DmxTransport for RecordingTransport {
        fn write_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
            self.packets.push(packet.to_vec());
            Ok(())
        }

        fn close(&mut self) -> Result<(), TransportError> {
            self.closed = true;
            Ok(())
        }
    }

    #[test]
    fn encodes_usb_pro_packet_without_hardware() {
        let mut output =
            EnttecDmxUsbPro::with_transport(RecordingTransport::default(), UniverseId(1));
        let mut frame = UniverseFrame::black();
        frame.set(DmxChannel::new(1).unwrap(), 10);
        frame.set(DmxChannel::new(512).unwrap(), 200);
        output.send(UniverseId(1), &frame).unwrap();
        let transport = output.into_transport();
        let packet = &transport.packets[0];
        assert_eq!(packet.len(), 518);
        assert_eq!(&packet[..5], &[0x7e, 6, 1, 2, 0]);
        assert_eq!(packet[5], 10);
        assert_eq!(packet[516], 200);
        assert_eq!(packet[517], 0xe7);
    }

    #[test]
    fn rejects_unconfigured_universe_without_writing() {
        let mut output =
            EnttecDmxUsbPro::with_transport(RecordingTransport::default(), UniverseId(1));
        assert!(matches!(
            output.send(UniverseId(2), &UniverseFrame::black()),
            Err(TransportError::UnsupportedUniverse { .. })
        ));
        assert!(output.into_transport().packets.is_empty());
    }

    #[test]
    fn close_is_forwarded_to_transport() {
        let mut output =
            EnttecDmxUsbPro::with_transport(RecordingTransport::default(), UniverseId(1));
        output.close().unwrap();
        assert!(output.into_transport().closed);
    }

    #[test]
    fn missing_device_is_a_structured_error() {
        let path = Path::new("/definitely/not/a/dmx/device");
        assert!(matches!(
            SerialTransport::open(path, Duration::from_millis(1)),
            Err(TransportError::DeviceNotFound { .. })
        ));
    }
}
