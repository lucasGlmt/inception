//! Raw Open DMX USB output.
//!
//! Unlike the ENTTEC DMX USB Pro, an Open DMX USB widget has no onboard
//! microcontroller to frame packets: the host must generate the DMX512
//! line signal itself — a break, a Mark After Break (MAB), then the start
//! code and up to 512 channel bytes at 250,000 baud.
//!
//! The break/MAB timing and the serial handling below mirror a proven,
//! field-tested implementation for this exact class of widget (FTDI
//! FT232R-based Open DMX USB adapters), rather than the first two attempts
//! at this driver, both of which failed on real hardware:
//!
//! - Generating the break via a baud-rate switch (drop to 50,000 baud,
//!   write a single `0x00` byte, switch back) wedged the port after the
//!   first frame — every subsequent write timed out.
//! - Generating the break via `set_break`/`clear_break` but draining with
//!   `flush`/`tcdrain` also wedged after the first frame: `tcdrain` never
//!   reliably reported completion and blocked until the port's read/write
//!   timeout.
//!
//! Two things the working reference does that this driver was missing:
//!
//! - **RTS is explicitly deasserted at open** (`write_request_to_send`).
//!   Many Open DMX widgets tie their RS-485 driver-enable pin to the
//!   FTDI chip's RTS line; leaving it at whatever the OS driver defaults
//!   to can intermittently gate transmission — a very plausible explanation
//!   for "random flicker" with no other change.
//! - **Draining uses `bytes_to_write` polling, not `tcdrain`/`flush`.**
//!   `bytes_to_write` reads the OS software queue depth (non-blocking,
//!   effectively `TIOCOUTQ`) instead of asking the driver to block until
//!   physical transmission completes, which is exactly what was unreliable
//!   above. Once the queue reports empty, a fixed sleep sized from the
//!   frame's own wire time (513 slots * 11 bits / 250,000 baud ≈ 22.6ms)
//!   covers the gap between "queued" and "actually on the wire".

use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use inception_core::UniverseId;
use inception_renderer::UniverseFrame;

use crate::DmxOutput;
use crate::transport::{DmxTransport, TransportError, classify_open_error, classify_write_error};

const OPEN_DMX_BAUD_RATE: u32 = 250_000;
const DMX_PACKET_LENGTH: usize = 513;

/// DMX512-A requires a break >= 92us and a MAB >= 12us, with no defined
/// upper bound on either.
const DEFAULT_BREAK_DURATION: Duration = Duration::from_micros(110);
const DEFAULT_MAB_DURATION: Duration = Duration::from_micros(16);

/// 513 slots * 11 bits (8 data + 2 stop, matching `StopBits::Two`) / 250,000
/// baud = 22.572ms. Applied *after* the OS output queue reports empty, to
/// cover the gap between "queued" and "physically clocked out".
const WIRE_TIME: Duration = Duration::from_millis(23);

const DEFAULT_IO_TIMEOUT: Duration = Duration::from_millis(250);

pub struct OpenDmxTransport {
    port: Box<dyn serialport::SerialPort>,
    break_duration: Duration,
    mab_duration: Duration,
    io_timeout: Duration,
}

impl std::fmt::Debug for OpenDmxTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenDmxTransport")
            .finish_non_exhaustive()
    }
}

impl OpenDmxTransport {
    pub fn open(
        path: &Path,
        io_timeout: Duration,
        break_duration: Duration,
        mab_duration: Duration,
    ) -> Result<Self, TransportError> {
        if !path.exists() {
            return Err(TransportError::DeviceNotFound {
                path: path.to_path_buf(),
            });
        }
        let mut port = serialport::new(path.to_string_lossy(), OPEN_DMX_BAUD_RATE)
            .timeout(io_timeout)
            .data_bits(serialport::DataBits::Eight)
            .flow_control(serialport::FlowControl::None)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::Two)
            .open()
            .map_err(|source| classify_open_error(path, source))?;
        // Many Open DMX widgets tie their RS-485 driver-enable pin to RTS;
        // pin it to a known level instead of leaving it at the driver's
        // default, which can intermittently gate transmission.
        port.write_request_to_send(false)
            .map_err(|error| classify_write_error(error.into()))?;
        port.clear_break()
            .map_err(|error| classify_write_error(error.into()))?;
        port.clear(serialport::ClearBuffer::All)
            .map_err(|error| classify_write_error(error.into()))?;
        Ok(Self {
            port,
            break_duration,
            mab_duration,
            io_timeout,
        })
    }

    fn write_frame(&mut self, mut remaining: &[u8]) -> Result<(), TransportError> {
        let deadline = Instant::now() + self.io_timeout;
        while !remaining.is_empty() {
            if Instant::now() >= deadline {
                return Err(TransportError::Timeout);
            }
            match self.port.write(remaining) {
                Ok(0) => return Err(TransportError::Disconnected),
                Ok(count) => remaining = &remaining[count..],
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(classify_write_error(error)),
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), TransportError> {
        let deadline = Instant::now() + self.io_timeout;
        loop {
            let pending = self
                .port
                .bytes_to_write()
                .map_err(|error| classify_write_error(error.into()))?;
            if pending == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(TransportError::Timeout);
            }
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(WIRE_TIME);
        Ok(())
    }
}

impl DmxTransport for OpenDmxTransport {
    fn write_packet(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        self.port
            .set_break()
            .map_err(|error| classify_write_error(error.into()))?;
        thread::sleep(self.break_duration);
        self.port
            .clear_break()
            .map_err(|error| classify_write_error(error.into()))?;
        thread::sleep(self.mab_duration);
        self.write_frame(packet)?;
        self.drain()
    }

    fn close(&mut self) -> Result<(), TransportError> {
        // Best effort: do not leave BREAK asserted on the way out.
        let _ = self.port.clear_break();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct OpenDmxConfig {
    pub device_path: PathBuf,
    pub universe: UniverseId,
    pub timeout: Duration,
    pub break_duration: Duration,
    pub mab_duration: Duration,
}

impl OpenDmxConfig {
    pub fn new(device_path: impl Into<PathBuf>, universe: UniverseId) -> Self {
        Self {
            device_path: device_path.into(),
            universe,
            timeout: DEFAULT_IO_TIMEOUT,
            break_duration: DEFAULT_BREAK_DURATION,
            mab_duration: DEFAULT_MAB_DURATION,
        }
    }
}

#[derive(Debug)]
pub struct OpenDmxOutput<T> {
    transport: T,
    universe: UniverseId,
    packet: [u8; DMX_PACKET_LENGTH],
}

pub type RealOpenDmxOutput = OpenDmxOutput<OpenDmxTransport>;

impl OpenDmxOutput<OpenDmxTransport> {
    pub fn open(config: OpenDmxConfig) -> Result<Self, TransportError> {
        let transport = OpenDmxTransport::open(
            &config.device_path,
            config.timeout,
            config.break_duration,
            config.mab_duration,
        )?;
        Ok(Self::with_transport(transport, config.universe))
    }
}

impl<T> OpenDmxOutput<T> {
    pub fn with_transport(transport: T, universe: UniverseId) -> Self {
        Self {
            transport,
            universe,
            // packet[0] is the DMX start code (0 for standard dimmer data);
            // channels fill packet[1..513].
            packet: [0; DMX_PACKET_LENGTH],
        }
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: DmxTransport> DmxOutput for OpenDmxOutput<T> {
    type Error = TransportError;

    fn send(&mut self, universe: UniverseId, frame: &UniverseFrame) -> Result<(), Self::Error> {
        if universe != self.universe {
            return Err(TransportError::UnsupportedUniverse {
                configured: self.universe,
                requested: universe,
            });
        }
        self.packet[1..].copy_from_slice(frame.as_slice());
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
    fn encodes_start_code_and_channels_without_hardware() {
        let mut output =
            OpenDmxOutput::with_transport(RecordingTransport::default(), UniverseId(1));
        let mut frame = UniverseFrame::black();
        frame.set(DmxChannel::new(1).unwrap(), 10);
        frame.set(DmxChannel::new(512).unwrap(), 200);
        output.send(UniverseId(1), &frame).unwrap();
        let transport = output.into_transport();
        let packet = &transport.packets[0];
        assert_eq!(packet.len(), 513);
        assert_eq!(packet[0], 0, "DMX start code must be 0");
        assert_eq!(packet[1], 10);
        assert_eq!(packet[512], 200);
    }

    #[test]
    fn rejects_unconfigured_universe_without_writing() {
        let mut output =
            OpenDmxOutput::with_transport(RecordingTransport::default(), UniverseId(1));
        assert!(matches!(
            output.send(UniverseId(2), &UniverseFrame::black()),
            Err(TransportError::UnsupportedUniverse { .. })
        ));
        assert!(output.into_transport().packets.is_empty());
    }

    #[test]
    fn close_is_forwarded_to_transport() {
        let mut output =
            OpenDmxOutput::with_transport(RecordingTransport::default(), UniverseId(1));
        output.close().unwrap();
        assert!(output.into_transport().closed);
    }

    #[test]
    fn missing_device_is_a_structured_error() {
        let path = Path::new("/definitely/not/a/dmx/device");
        assert!(matches!(
            OpenDmxTransport::open(
                path,
                Duration::from_millis(1),
                DEFAULT_BREAK_DURATION,
                DEFAULT_MAB_DURATION
            ),
            Err(TransportError::DeviceNotFound { .. })
        ));
    }
}
