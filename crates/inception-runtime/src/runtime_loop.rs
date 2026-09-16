use inception_core::{Clock, MonotonicClock, Timestamp};
use inception_driver_dmx::DmxOutput;

use crate::{FrameScheduler, RuntimeConfig, RuntimeEngine, RuntimeLoopError, RuntimeTimingStats};

pub trait Sleeper<C: Clock> {
    type Error;

    fn sleep_until(&mut self, clock: &C, deadline: Timestamp) -> Result<(), Self::Error>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StdSleeper;

impl Sleeper<MonotonicClock> for StdSleeper {
    type Error = std::convert::Infallible;

    fn sleep_until(
        &mut self,
        clock: &MonotonicClock,
        deadline: Timestamp,
    ) -> Result<(), Self::Error> {
        let now = clock.now();
        if deadline > now {
            std::thread::sleep(std::time::Duration::from_nanos(deadline.0 - now.0));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct RuntimeLoop<C, S> {
    clock: C,
    sleeper: S,
    config: RuntimeConfig,
    scheduler: Option<FrameScheduler>,
}

impl<C: Clock, S: Sleeper<C>> RuntimeLoop<C, S> {
    pub fn new(config: RuntimeConfig, clock: C, sleeper: S) -> Self {
        Self {
            clock,
            sleeper,
            config,
            scheduler: None,
        }
    }

    pub fn start<O: DmxOutput>(
        &mut self,
        engine: &mut RuntimeEngine<O>,
    ) -> Result<(), RuntimeLoopError<O::Error, S::Error>> {
        let now = self.clock.now();
        let mut scheduler = FrameScheduler::new(self.config, now);
        scheduler.take_due(now);
        engine.start(now).map_err(RuntimeLoopError::Engine)?;
        self.scheduler = Some(scheduler);
        Ok(())
    }

    /// Waits for one absolute deadline and emits at most one frame. If the
    /// process wakes late, the scheduler skips directly to a future deadline.
    pub fn run_next_frame<O: DmxOutput>(
        &mut self,
        engine: &mut RuntimeEngine<O>,
    ) -> Result<(), RuntimeLoopError<O::Error, S::Error>> {
        let scheduler = self
            .scheduler
            .as_mut()
            .ok_or(RuntimeLoopError::NotStarted)?;
        self.sleeper
            .sleep_until(&self.clock, scheduler.next_deadline())
            .map_err(RuntimeLoopError::Sleep)?;
        let now = self.clock.now();
        if scheduler.take_due(now).is_some() {
            engine.tick(now).map_err(RuntimeLoopError::Engine)?;
        }
        Ok(())
    }

    /// Runs until `keep_running` returns false, then performs the normal
    /// blackout-and-close shutdown. A caller can capture an atomic stop flag
    /// in the closure without coupling the runtime to a signal library.
    pub fn run_while<O, F>(
        &mut self,
        engine: &mut RuntimeEngine<O>,
        mut keep_running: F,
    ) -> Result<(), RuntimeLoopError<O::Error, S::Error>>
    where
        O: DmxOutput,
        F: FnMut() -> bool,
    {
        while keep_running() {
            self.run_next_frame(engine)?;
        }
        self.stop(engine)
    }

    pub fn stop<O: DmxOutput>(
        &mut self,
        engine: &mut RuntimeEngine<O>,
    ) -> Result<(), RuntimeLoopError<O::Error, S::Error>> {
        engine.stop().map_err(RuntimeLoopError::Engine)
    }

    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.scheduler.as_ref().map(FrameScheduler::next_deadline)
    }

    pub fn timing_stats(&self) -> Option<RuntimeTimingStats> {
        self.scheduler.as_ref().map(FrameScheduler::stats)
    }

    pub fn clock(&self) -> &C {
        &self.clock
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use inception_core::{Duration, VirtualClock};

    use super::*;

    #[derive(Debug, Default)]
    struct VirtualSleeper;

    impl Sleeper<VirtualClock> for VirtualSleeper {
        type Error = Infallible;

        fn sleep_until(
            &mut self,
            clock: &VirtualClock,
            deadline: Timestamp,
        ) -> Result<(), Self::Error> {
            let now = clock.now();
            if deadline > now {
                clock.advance(Duration::from_nanos(deadline.0 - now.0));
            }
            Ok(())
        }
    }

    #[test]
    fn virtual_sleeper_advances_to_an_absolute_deadline() {
        let clock = VirtualClock::new();
        let mut sleeper = VirtualSleeper;
        sleeper
            .sleep_until(&clock, Timestamp::from_millis(25))
            .unwrap();
        assert_eq!(clock.now(), Timestamp::from_millis(25));
    }
}
