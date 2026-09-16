//! Manual ENTTEC DMX USB Pro smoke test. Never run by CI.

use std::path::PathBuf;
use std::time::Duration;

use inception_core::UniverseId;
use inception_driver_dmx::{DmxOutput, EnttecDmxUsbProConfig, RealDmxOutput};
use inception_renderer::{DmxChannel, UniverseFrame};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let device = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: dmx_smoke_test <device-path> <channel 1..512> <value 0..32>")?;
    let channel: u16 = arguments.next().ok_or("missing channel")?.parse()?;
    let value: u8 = arguments.next().ok_or("missing value")?.parse()?;
    if value > 32 {
        return Err("the smoke test intentionally limits values to 0..32".into());
    }
    let channel = DmxChannel::new(channel).ok_or("channel must be in 1..=512")?;
    let universe = UniverseId(1);
    let mut output = RealDmxOutput::open(EnttecDmxUsbProConfig::new(device, universe))?;

    let mut frame = UniverseFrame::black();
    frame.set(channel, value);
    let send_result = output.send(universe, &frame);
    if send_result.is_ok() {
        std::thread::sleep(Duration::from_millis(500));
    }
    let blackout_result = output.send(universe, &UniverseFrame::black());
    let close_result = output.close();
    send_result?;
    blackout_result?;
    close_result?;
    Ok(())
}
