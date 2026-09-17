//! Runtime orchestration over an already linked [`RuntimeImage`].
//!
//! [`RuntimeEngine`] performs one logical, timestamped lighting cycle. The
//! separate [`RuntimeLoop`] owns real-time scheduling. This keeps virtual-time
//! tests free of sleeps and keeps OS timing out of the semantic hot path.

mod config;
mod engine;
mod error;
mod event_router;
mod runtime_loop;
mod scheduler;

pub use config::{RuntimeConfig, RuntimeConfigError};
pub use engine::{EventDispatchReport, LoadedProgram, ReloadReport, RuntimeEngine, RuntimeHost};
pub use error::{OutputOperation, RuntimeError, RuntimeLoopError};
pub use event_router::EventRouter;
pub use runtime_loop::{RuntimeLoop, Sleeper, StdSleeper};
pub use scheduler::{FrameDeadline, FrameScheduler, RuntimeTimingStats};
