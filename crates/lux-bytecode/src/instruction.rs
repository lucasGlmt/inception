//! The V1 instruction set: a small, stack-based opcode list.
//!
//! Kept intentionally minimal (see `AGENTS.md`'s "hors scope" list) —
//! nothing DMX-, fixture- or scheduling-related exists yet. Represented
//! as a plain Rust enum rather than a packed binary encoding: the
//! priority for this milestone is correctness and verifiability, not a
//! final on-disk format (see item 15 of the task brief).

use crate::attribute::Attribute;
use crate::ids::{ConstantId, FunctionId, LocalId, TargetId};
use crate::intrinsic::IntrinsicId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    /// Pushes a constant pool entry onto the stack.
    Const(ConstantId),

    /// Pushes a local's current value onto the stack.
    LoadLocal(LocalId),
    /// Pops the top of the stack into a local.
    StoreLocal(LocalId),

    Add,
    Sub,
    Mul,
    Div,

    /// Pops a `Duration` and suspends execution for that long.
    Wait,

    /// Calls another function. V1 defines no parameters or return
    /// values: this has no stack effect yet, and nothing currently
    /// generates it (see `lux-mir`) — it exists so the opcode doesn't
    /// need to be redesigned once user-defined functions exist.
    Call(FunctionId),

    /// Calls one builtin stdlib intrinsic (`std.Math`, `std.Color`, ...),
    /// dispatched purely by `intrinsic` — never by name/string. One
    /// opcode for every intrinsic rather than one per function: adding a
    /// new stdlib function later never needs a new opcode, only a new
    /// `IntrinsicId` variant. `arg_count` is redundant with
    /// `intrinsic.param_types().len()` (the verifier checks the two
    /// agree, see `verify.rs`) but is carried explicitly so the VM can
    /// pop its operands generically, without its own per-intrinsic arity
    /// table. Net stack effect is `1 - arg_count` (every intrinsic
    /// returns exactly one value).
    CallIntrinsic {
        intrinsic: IntrinsicId,
        arg_count: u8,
    },

    /// Ends the current function. The stack must be empty at this point
    /// (Lux scenes don't return a value in this milestone).
    Return,

    /// Discards the top of the stack, e.g. for a bare expression
    /// statement whose value is unused.
    Pop,

    /// Pops a value and applies it to `attribute` on every fixture
    /// `target` resolves to, in the semantic `LightingState` (never DMX —
    /// see `AGENTS.md`'s lighting-state/renderer separation). The popped
    /// value's type must match `attribute.value_type()`; net stack
    /// effect is `-1`, the same as `StoreLocal`/`Wait`/`Pop`.
    SetAttribute {
        target: TargetId,
        attribute: Attribute,
    },

    /// Pops `Duration` first, then an attribute-compatible target value,
    /// and starts a non-blocking transition at the VM clock's current
    /// timestamp. V1 supports `Intensity` only.
    TransitionAttribute {
        target: TargetId,
        attribute: Attribute,
    },

    /// Pops a `Signal<T>` value (`T` must match `attribute.value_type()`)
    /// and installs it as the continuously-sampled controller for
    /// `attribute` on every fixture `target` resolves to, replacing any
    /// transition or signal binding previously active on that
    /// `(fixture, attribute)` pair. Net stack effect is `-1`, same as
    /// `SetAttribute`. Unlike `SetAttribute`, this does not itself write a
    /// value into `LightingState` — the binding is sampled continuously by
    /// the runtime, not just once here (see `AGENTS.md`'s deterministic
    /// signal-sampling rule).
    BindSignal {
        target: TargetId,
        attribute: Attribute,
    },

    /// Pops an `Int` index, then a `Sequence<T>` receiver, and pushes the
    /// element at that index — `<receiver>[<index>]`. Net stack effect is
    /// `-1`. Not a `CallIntrinsic`: indexing is language syntax, not a
    /// named stdlib function, matching `Add`/`Sub`/`Wait`'s style of a
    /// dedicated opcode with dynamically-typed verification (see
    /// `verify.rs`) rather than a fixed `param_types()` table — the
    /// element type isn't fixed per instruction, it's whatever
    /// `Sequence<T>` the receiver on the stack actually is.
    Index,
}
