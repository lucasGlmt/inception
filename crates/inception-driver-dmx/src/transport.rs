//! Shared error type and byte-transport trait for serial DMX outputs.
//!
//! Both the ENTTEC DMX USB Pro protocol (framed packets over a chatty
//! microcontroller) and raw Open DMX USB (host-timed break/MAB/data) end up
//! writing bytes to a serial port and can fail the same ways, so they share
//! this vocabulary instead of duplicating it per driver.

use std::path::PathBuf;

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
        configured: inception_core::UniverseId,
        requested: inception_core::UniverseId,
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

pub(crate) fn classify_write_error(error: std::io::Error) -> TransportError {
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

pub(crate) fn classify_open_error(
    path: &std::path::Path,
    source: serialport::Error,
) -> TransportError {
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
}
