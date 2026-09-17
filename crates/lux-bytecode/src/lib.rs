//! Lux bytecode: format, verifier, and debug disassembly.
//!
//! Independent of the Lux compiler frontend (`lux-syntax`, `lux-hir`,
//! `lux-typeck`, `lux-compiler`) by design — see `AGENTS.md`. This is the
//! crate `inception-vm` will eventually execute directly, and it must be
//! usable on its own, e.g. to load and verify a module without pulling in
//! an entire compiler.

pub mod attribute;
pub mod disasm;
pub mod event;
pub mod ids;
pub mod instruction;
pub mod intrinsic;
pub mod module;
pub mod role;
pub mod value;
pub mod verify;

#[cfg(test)]
mod tests;

pub use attribute::Attribute;
pub use disasm::disassemble;
pub use event::{EventAction, EventBinding, EventPattern};
pub use ids::{ConstantId, FunctionId, LocalId, TargetId};
pub use instruction::Instruction;
pub use intrinsic::IntrinsicId;
pub use module::{BytecodeModule, BytecodeVersion, Function};
pub use role::{
    Capability, CapabilitySet, PortableRigContract, PortableRole, RoleCardinality, RoleId,
};
pub use value::{ColorValue, Constant, ScalarValueType, ValueType};
pub use verify::{VerificationError, VerificationErrorKind, verify};
