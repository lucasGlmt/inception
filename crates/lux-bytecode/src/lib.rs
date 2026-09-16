//! Lux bytecode: format, verifier, and debug disassembly.
//!
//! Independent of the Lux compiler frontend (`lux-syntax`, `lux-hir`,
//! `lux-typeck`, `lux-compiler`) by design — see `AGENTS.md`. This is the
//! crate `inception-vm` will eventually execute directly, and it must be
//! usable on its own, e.g. to load and verify a module without pulling in
//! an entire compiler.

pub mod disasm;
pub mod ids;
pub mod instruction;
pub mod module;
pub mod value;
pub mod verify;

#[cfg(test)]
mod tests;

pub use disasm::disassemble;
pub use ids::{ConstantId, FunctionId, LocalId};
pub use instruction::Instruction;
pub use module::{BytecodeModule, BytecodeVersion, Function};
pub use value::{ColorValue, Constant, ValueType};
pub use verify::{VerificationError, VerificationErrorKind, verify};
