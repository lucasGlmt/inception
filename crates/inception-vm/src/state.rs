//! The VM's explicit execution state.

use inception_core::Timestamp;

use crate::error::VmError;

/// Where a [`crate::Vm`] currently stands. Kept as one enum rather than
/// several booleans (`is_running`/`is_waiting`/`is_finished`, ...)
/// specifically so states that shouldn't coexist — e.g. "waiting" and
/// "finished" — can't be represented at all, per `AGENTS.md`'s "invalid
/// states should be difficult or impossible to represent" principle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VmState {
    /// Constructed, not yet started (`Vm::start` not called).
    Ready,
    /// Actively executing instructions. Since this VM runs synchronously
    /// within a single `run_until_blocked` call (no threads, no fibers),
    /// this state is only ever observed *during* that call, never by a
    /// caller inspecting `Vm::state()` in between calls.
    Running,
    /// Blocked on a `WAIT`, to be resumed once `Clock::now() >= T`. See
    /// the module-level docs on `WAIT` semantics.
    WaitingUntil(Timestamp),
    /// The entry function returned. Terminal: nothing left to execute.
    Finished,
    /// Execution hit an unrecoverable error. Terminal: the VM does not
    /// attempt to resume past a fault.
    Faulted(VmError),
}

impl VmState {
    pub fn is_waiting(&self) -> bool {
        matches!(self, VmState::WaitingUntil(_))
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, VmState::Finished)
    }

    pub fn is_faulted(&self) -> bool {
        matches!(self, VmState::Faulted(_))
    }
}
