use inception_core::Duration;

pub const DEFAULT_OUTPUT_FREQUENCY_HZ: u32 = 40;
pub const MAX_OUTPUT_FREQUENCY_HZ: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    output_frequency_hz: u32,
}

impl RuntimeConfig {
    pub fn new(output_frequency_hz: u32) -> Result<Self, RuntimeConfigError> {
        if !(1..=MAX_OUTPUT_FREQUENCY_HZ).contains(&output_frequency_hz) {
            return Err(RuntimeConfigError::InvalidOutputFrequency {
                requested_hz: output_frequency_hz,
                maximum_hz: MAX_OUTPUT_FREQUENCY_HZ,
            });
        }
        Ok(Self {
            output_frequency_hz,
        })
    }

    pub const fn output_frequency_hz(self) -> u32 {
        self.output_frequency_hz
    }

    pub fn frame_period(self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.output_frequency_hz))
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            output_frequency_hz: DEFAULT_OUTPUT_FREQUENCY_HZ,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeConfigError {
    InvalidOutputFrequency { requested_hz: u32, maximum_hz: u32 },
}

impl std::fmt::Display for RuntimeConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOutputFrequency {
                requested_hz,
                maximum_hz,
            } => write!(
                formatter,
                "invalid output frequency {requested_hz} Hz; expected 1..={maximum_hz} Hz"
            ),
        }
    }
}

impl std::error::Error for RuntimeConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_forty_hertz() {
        let config = RuntimeConfig::default();
        assert_eq!(config.output_frequency_hz(), 40);
        assert_eq!(config.frame_period(), Duration::from_millis(25));
    }

    #[test]
    fn rejects_zero_and_unreasonably_high_frequencies() {
        assert!(RuntimeConfig::new(0).is_err());
        assert!(RuntimeConfig::new(MAX_OUTPUT_FREQUENCY_HZ + 1).is_err());
    }
}
