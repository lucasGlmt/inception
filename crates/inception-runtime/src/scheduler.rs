use inception_core::{Duration, Timestamp};

use crate::RuntimeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDeadline {
    pub scheduled_at: Timestamp,
    pub actual_at: Timestamp,
    pub lateness: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeTimingStats {
    pub frames: u64,
    pub late_frames: u64,
    pub max_lateness: Duration,
    pub runtime_duration: Duration,
}

#[derive(Debug, Clone)]
pub struct FrameScheduler {
    origin: Timestamp,
    period: Duration,
    next_deadline: Timestamp,
    stats: RuntimeTimingStats,
}

impl FrameScheduler {
    pub fn new(config: RuntimeConfig, origin: Timestamp) -> Self {
        Self {
            origin,
            period: config.frame_period(),
            next_deadline: origin,
            stats: RuntimeTimingStats::default(),
        }
    }

    pub const fn next_deadline(&self) -> Timestamp {
        self.next_deadline
    }

    pub const fn stats(&self) -> RuntimeTimingStats {
        self.stats
    }

    /// Consumes at most one due frame and jumps to the first future absolute
    /// deadline. Missed deadlines are never replayed in a burst.
    pub fn take_due(&mut self, now: Timestamp) -> Option<FrameDeadline> {
        if now < self.next_deadline {
            return None;
        }

        let scheduled_at = self.next_deadline;
        let lateness = Duration::from_nanos(now.0.saturating_sub(scheduled_at.0));
        self.stats.frames = self.stats.frames.saturating_add(1);
        if lateness > Duration::ZERO {
            self.stats.late_frames = self.stats.late_frames.saturating_add(1);
            self.stats.max_lateness = self.stats.max_lateness.max(lateness);
        }
        self.stats.runtime_duration = Duration::from_nanos(now.0.saturating_sub(self.origin.0));

        let elapsed = now.0.saturating_sub(self.origin.0);
        let next_index = elapsed / self.period.0 + 1;
        self.next_deadline = Timestamp(
            self.origin
                .0
                .saturating_add(next_index.saturating_mul(self.period.0)),
        );

        Some(FrameDeadline {
            scheduled_at,
            actual_at: now,
            lateness,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forty_hertz_uses_absolute_twenty_five_millisecond_deadlines() {
        let mut scheduler = FrameScheduler::new(RuntimeConfig::default(), Timestamp::ZERO);
        for millis in [0, 25, 50, 75, 100] {
            assert_eq!(scheduler.next_deadline(), Timestamp::from_millis(millis));
            assert!(scheduler.take_due(Timestamp::from_millis(millis)).is_some());
        }
    }

    #[test]
    fn processing_cost_does_not_shift_the_next_deadline() {
        let mut scheduler = FrameScheduler::new(RuntimeConfig::default(), Timestamp::ZERO);
        scheduler.take_due(Timestamp::ZERO).unwrap();
        scheduler.take_due(Timestamp::from_millis(25)).unwrap();
        assert_eq!(scheduler.next_deadline(), Timestamp::from_millis(50));
        assert!(scheduler.take_due(Timestamp::from_millis(30)).is_none());
        assert_eq!(scheduler.next_deadline(), Timestamp::from_millis(50));
    }

    #[test]
    fn late_frame_skips_missed_deadlines_without_catch_up() {
        let mut scheduler = FrameScheduler::new(RuntimeConfig::default(), Timestamp::ZERO);
        scheduler.take_due(Timestamp::ZERO).unwrap();
        scheduler.take_due(Timestamp::from_millis(25)).unwrap();
        let frame = scheduler.take_due(Timestamp::from_millis(87)).unwrap();
        assert_eq!(frame.scheduled_at, Timestamp::from_millis(50));
        assert_eq!(frame.actual_at, Timestamp::from_millis(87));
        assert_eq!(frame.lateness, Duration::from_millis(37));
        assert_eq!(scheduler.next_deadline(), Timestamp::from_millis(100));
        assert!(scheduler.take_due(Timestamp::from_millis(87)).is_none());
        assert_eq!(scheduler.stats().late_frames, 1);
    }

    #[test]
    fn one_virtual_hour_has_no_deadline_drift() {
        let mut scheduler = FrameScheduler::new(RuntimeConfig::default(), Timestamp::ZERO);
        for frame in 0..=144_000_u64 {
            let timestamp = Timestamp::from_millis(frame * 25);
            assert!(scheduler.take_due(timestamp).is_some());
        }
        assert_eq!(scheduler.stats().frames, 144_001);
        assert_eq!(scheduler.stats().late_frames, 0);
        assert_eq!(scheduler.next_deadline(), Timestamp::from_millis(3_600_025));
    }
}
