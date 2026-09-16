//! Deterministic time primitives.
//!
//! Everything here is nanosecond-based (matching `lux-bytecode`'s
//! `Constant::Duration(u64)` representation) and free of any dependency
//! on wall-clock or OS time. Engine code should never call
//! `std::time::Instant::now()`, `SystemTime::now()`, `thread::sleep()` or
//! an async runtime's sleep directly — time is always injected through
//! [`Clock`], so it can be replaced with [`VirtualClock`] in tests and
//! advanced instantly, with no real waiting and no non-determinism.

use std::cell::Cell;

/// A point in time, in nanoseconds since some [`Clock`]'s epoch. Two
/// timestamps are only comparable if they came from the same clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub const ZERO: Timestamp = Timestamp(0);

    pub const fn from_nanos(nanos: u64) -> Self {
        Timestamp(nanos)
    }

    pub fn from_millis(millis: u64) -> Self {
        Timestamp(millis.saturating_mul(1_000_000))
    }

    pub fn from_secs(secs: u64) -> Self {
        Timestamp(secs.saturating_mul(1_000_000_000))
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }
}

/// A span of time, in nanoseconds. Always non-negative — Lux has no
/// negative duration values (see `lux-typeck`'s typing rules), so this
/// type doesn't need to represent one either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Duration(pub u64);

impl Duration {
    pub const ZERO: Duration = Duration(0);

    pub const fn from_nanos(nanos: u64) -> Self {
        Duration(nanos)
    }

    pub fn from_millis(millis: u64) -> Self {
        Duration(millis.saturating_mul(1_000_000))
    }

    pub fn from_secs(secs: u64) -> Self {
        Duration(secs.saturating_mul(1_000_000_000))
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// `None` on overflow, rather than wrapping or panicking — a caller
    /// executing Lux-level arithmetic (see `inception-vm`) is expected to
    /// turn that into a structured runtime error instead of silently
    /// producing a wrong value.
    pub fn checked_add(self, other: Duration) -> Option<Duration> {
        self.0.checked_add(other.0).map(Duration)
    }

    /// `None` on underflow (i.e. `other > self`) — Lux durations can't be
    /// negative, so "1s - 500ms" is fine but "500ms - 1s" has no valid
    /// result rather than wrapping to a huge value.
    pub fn checked_sub(self, other: Duration) -> Option<Duration> {
        self.0.checked_sub(other.0).map(Duration)
    }
}

/// `Timestamp + Duration` saturates instead of overflowing: this is used
/// to compute a `WAIT`'s absolute wake-up time, an engine scheduling
/// concern rather than a Lux-level value the program can observe or
/// compute with. Saturating just means "wake up effectively never
/// sooner than representable", which is harmless — nanosecond `u64`
/// already covers roughly 584 years from zero.
impl std::ops::Add<Duration> for Timestamp {
    type Output = Timestamp;

    fn add(self, rhs: Duration) -> Timestamp {
        Timestamp(self.0.saturating_add(rhs.0))
    }
}

/// Supplies the current time. Implementations must be monotonic:
/// `now()` should never go backwards.
///
/// Engine and VM code must only ever learn the time through this trait —
/// never by reading the OS clock directly — so that swapping in a
/// [`VirtualClock`] makes execution fully deterministic and instant, with
/// no real sleeping.
pub trait Clock {
    fn now(&self) -> Timestamp;
}

/// A clock entirely under the test's control. Time only ever moves when
/// [`VirtualClock::advance`] is called — never on its own, and never by
/// blocking.
///
/// Uses a `Cell` (not a `Mutex`) for interior mutability: `Clock::now`
/// takes `&self`, so a single `VirtualClock` value can be held by both a
/// test (to advance it) and a `Vm` (to read it) via ordinary shared
/// references, with no locking needed for a single-threaded VM.
#[derive(Debug, Default)]
pub struct VirtualClock {
    now: Cell<u64>,
}

impl VirtualClock {
    pub fn new() -> Self {
        Self { now: Cell::new(0) }
    }

    /// Moves time forward by `duration`. Instant — never blocks, never
    /// sleeps.
    pub fn advance(&self, duration: Duration) {
        self.now.set(self.now.get().saturating_add(duration.0));
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.now.get())
    }
}

/// A minimal real-time clock for actually running Inception outside of
/// tests. Based on `Instant` (monotonic), never on civil/wall-clock time.
/// Not this milestone's focus — see `AGENTS.md`'s "Deterministic
/// runtime" principle and item 5 of the VM task brief — but simple
/// enough that leaving it out would just move the same handful of lines
/// into whichever crate first needs real time.
#[derive(Debug, Clone)]
pub struct MonotonicClock {
    start: std::time::Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.start.elapsed().as_nanos() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero() {
        let clock = VirtualClock::new();
        assert_eq!(clock.now(), Timestamp::ZERO);
    }

    #[test]
    fn advance_moves_time_forward() {
        let clock = VirtualClock::new();
        clock.advance(Duration::from_secs(1));
        assert_eq!(clock.now(), Timestamp::from_secs(1));
    }

    #[test]
    fn multiple_advances_accumulate() {
        let clock = VirtualClock::new();
        clock.advance(Duration::from_millis(500));
        clock.advance(Duration::from_millis(500));
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.now(), Timestamp::from_millis(1250));
    }

    #[test]
    fn advance_never_blocks_and_needs_no_mut_binding() {
        // The point of `Cell`-based interior mutability: a shared `&self`
        // is enough to advance, so a VM holding `&clock` and a test
        // holding `clock` can coexist without `Rc<RefCell<_>>`.
        let clock = VirtualClock::new();
        let shared: &VirtualClock = &clock;
        clock.advance(Duration::from_secs(1));
        assert_eq!(shared.now(), Timestamp::from_secs(1));
    }

    #[test]
    fn timestamp_plus_duration_saturates_instead_of_overflowing() {
        let t = Timestamp(u64::MAX - 1);
        assert_eq!(t + Duration::from_nanos(100), Timestamp(u64::MAX));
    }

    #[test]
    fn duration_checked_add_reports_overflow() {
        assert_eq!(Duration(u64::MAX).checked_add(Duration(1)), None);
        assert_eq!(Duration(1).checked_add(Duration(1)), Some(Duration(2)));
    }

    #[test]
    fn duration_checked_sub_reports_underflow() {
        assert_eq!(
            Duration::from_millis(500).checked_sub(Duration::from_secs(1)),
            None
        );
        assert_eq!(
            Duration::from_secs(1).checked_sub(Duration::from_millis(500)),
            Some(Duration::from_millis(500))
        );
    }

    #[test]
    fn monotonic_clock_does_not_go_backwards() {
        let clock = MonotonicClock::new();
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }
}
