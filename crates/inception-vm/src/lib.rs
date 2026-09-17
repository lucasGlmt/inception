//! A minimal, deterministic, single-fiber Lux bytecode interpreter.
//!
//! Consumes exactly one thing: `lux_bytecode::BytecodeModule`. It has no
//! notion of Lux source, the AST, HIR or MIR, and no notion of lighting
//! state, fixtures, DMX or a rig — see `AGENTS.md` and this task's scope.
//!
//! Time is never read from the OS here: see `inception_core::Clock` and
//! [`vm`]'s module docs on `WAIT` semantics for why, and
//! `inception_core::VirtualClock` for how tests drive execution without
//! ever really waiting.

pub mod binding;
pub mod error;
pub mod frame;
pub mod intrinsic;
pub mod signal;
pub mod state;
pub mod value;
pub mod vm;

#[cfg(test)]
mod tests;

pub use binding::{ActiveSignalBinding, SignalBindingStore};
pub use error::{VmError, VmErrorKind, VmInitError};
pub use signal::{SignalError, SignalId, SignalKind, SignalSampleContext, SignalStore};
pub use state::VmState;
pub use value::Value;
pub use vm::Vm;
