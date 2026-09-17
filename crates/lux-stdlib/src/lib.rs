//! The Lux standard library's signature registry: `std.Math`, `std.Color`
//! and (in the future) more modules, represented as real, typed data —
//! never as string comparisons scattered through the compiler or VM.
//!
//! This crate has **zero dependencies**, including no dependency on
//! `lux-syntax`. It only describes *what* stdlib functions exist and
//! their signatures; it has no opinion on source syntax, HIR, bytecode or
//! runtime representations. `lux-hir`/`lux-typeck`/`lux-mir` depend on it
//! directly for compile-time resolution; `lux-bytecode` and
//! `inception-vm` deliberately do **not** (see `intrinsic` module doc).

mod intrinsic;
mod methods;
mod registry;
mod signature;
mod types;

pub use intrinsic::IntrinsicId;
pub use methods::{
    SIGNAL_FLOAT_METHODS, resolve_signal_float_method, signal_float_method_candidates,
};
pub use registry::{
    OverloadError, candidates, find_module, find_module_by_short_name, resolve_overload,
};
pub use signature::{Param, Signature, StdModule};
pub use types::ParamType;

/// Every standard-library module, e.g. for `import std.$0` completion.
pub use registry::STD_MODULES;
