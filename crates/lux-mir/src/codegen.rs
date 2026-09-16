//! Lowers MIR into a `lux_bytecode::BytecodeModule`.
//!
//! Two things happen here that MIR itself doesn't do:
//!
//! - **Constant pooling**: every [`crate::mir::MirInstruction::Const`]
//!   gets its own fresh entry in the module's constant pool, in
//!   encounter order. Deduplication is explicitly optional per the task
//!   brief (item 22) — skipping it keeps this pass a straightforward,
//!   trivially deterministic `Vec` append rather than needing an interning
//!   scheme, and correctness matters far more than pool size at this
//!   stage.
//! - **Stack depth accounting**: `max_stack` is computed here from the
//!   actual sequence of pushes/pops (one flat counter, since V1 has no
//!   branches to reconcile). `lux_bytecode::verify` does not trust this
//!   value — it's recomputed independently there — so a bug in this
//!   accounting would be caught as an internal-compiler-error by
//!   `lux-compiler::compile`, not silently accepted.
//!
//! This pass never fails: it's total over any [`crate::mir::MirModule`]
//! that `lower` could have produced. If it ever produces bytecode that
//! `lux_bytecode::verify` rejects, that's a compiler bug, not a user
//! error — see `lux-compiler`'s `compile`.

use lux_bytecode::{BytecodeModule, BytecodeVersion, Constant, Function, Instruction};
use lux_typeck::Type;

use crate::ids::FunctionId;
use crate::mir::{BasicBlock, MirConstant, MirFunction, MirInstruction, MirModule, Terminator};

pub fn lower_to_bytecode(module: &MirModule) -> BytecodeModule {
    let mut builder = Builder {
        constants: Vec::new(),
    };

    let functions = module
        .functions
        .iter()
        .map(|f| builder.lower_function(f))
        .collect();

    BytecodeModule {
        version: BytecodeVersion::CURRENT,
        constants: builder.constants,
        functions,
        entry: module.entry.map(to_bytecode_function_id),
    }
}

struct Builder {
    constants: Vec<Constant>,
}

impl Builder {
    fn intern(&mut self, constant: Constant) -> lux_bytecode::ConstantId {
        let id = lux_bytecode::ConstantId(self.constants.len() as u32);
        self.constants.push(constant);
        id
    }

    fn lower_function(&mut self, function: &MirFunction) -> Function {
        let mut code = Vec::new();
        let mut depth: usize = 0;
        let mut max_stack: usize = 0;

        for block in &function.blocks {
            self.lower_block(block, &mut code, &mut depth, &mut max_stack);
        }

        let locals = function
            .locals
            .iter()
            .map(|local| to_value_type(local.ty))
            .collect();

        Function {
            id: to_bytecode_function_id(function.id),
            code,
            locals,
            max_stack: max_stack as u16,
            debug_name: Some(function.name.clone()),
        }
    }

    fn lower_block(
        &mut self,
        block: &BasicBlock,
        code: &mut Vec<Instruction>,
        depth: &mut usize,
        max_stack: &mut usize,
    ) {
        for instruction in &block.instructions {
            self.lower_instruction(instruction, code, depth, max_stack);
        }
        match block.terminator {
            Terminator::Return => code.push(Instruction::Return),
        }
    }

    fn lower_instruction(
        &mut self,
        instruction: &MirInstruction,
        code: &mut Vec<Instruction>,
        depth: &mut usize,
        max_stack: &mut usize,
    ) {
        // Every arithmetic/store/wait/pop instruction pops exactly one
        // more value than it pushes (they all take one operand and push
        // at most nothing extra); only `Const`/`LoadLocal` are net pushes.
        let net_pushes = match instruction {
            MirInstruction::Const(constant) => {
                let id = self.intern(to_bytecode_constant(*constant));
                code.push(Instruction::Const(id));
                true
            }
            MirInstruction::LoadLocal(id) => {
                code.push(Instruction::LoadLocal(to_bytecode_local_id(*id)));
                true
            }
            MirInstruction::StoreLocal(id) => {
                code.push(Instruction::StoreLocal(to_bytecode_local_id(*id)));
                false
            }
            MirInstruction::Add => {
                code.push(Instruction::Add);
                false
            }
            MirInstruction::Sub => {
                code.push(Instruction::Sub);
                false
            }
            MirInstruction::Mul => {
                code.push(Instruction::Mul);
                false
            }
            MirInstruction::Div => {
                code.push(Instruction::Div);
                false
            }
            MirInstruction::Wait => {
                code.push(Instruction::Wait);
                false
            }
            MirInstruction::Pop => {
                code.push(Instruction::Pop);
                false
            }
        };

        if net_pushes {
            *depth += 1;
        } else {
            *depth = depth.saturating_sub(1);
        }
        *max_stack = (*max_stack).max(*depth);
    }
}

fn to_bytecode_function_id(id: FunctionId) -> lux_bytecode::FunctionId {
    lux_bytecode::FunctionId(id.0)
}

/// V1's `LocalId` is `u32`-backed in MIR (matching `lux-hir`) but
/// `u16`-backed in bytecode. A function with more than 65535 locals — far
/// beyond anything a real Lux scene produces — would silently truncate
/// here; this is a known, documented V1 limitation rather than a
/// defensive check, since a truncated ID would still be caught as an
/// `InvalidLocalId` (or a bogus-but-bounds-checked one) by
/// `lux_bytecode::verify` rather than cause unsafety.
fn to_bytecode_local_id(id: lux_hir::LocalId) -> lux_bytecode::LocalId {
    lux_bytecode::LocalId(id.0 as u16)
}

fn to_value_type(ty: Type) -> lux_bytecode::ValueType {
    match ty {
        Type::Bool => lux_bytecode::ValueType::Bool,
        Type::Int => lux_bytecode::ValueType::Int,
        Type::Float => lux_bytecode::ValueType::Float,
        Type::Duration => lux_bytecode::ValueType::Duration,
        Type::Intensity => lux_bytecode::ValueType::Intensity,
        Type::Color => lux_bytecode::ValueType::Color,
        Type::Angle => lux_bytecode::ValueType::Angle,
        Type::Frequency => lux_bytecode::ValueType::Frequency,
        Type::Tempo => lux_bytecode::ValueType::Tempo,
    }
}

fn to_bytecode_constant(constant: MirConstant) -> Constant {
    match constant {
        MirConstant::Bool(b) => Constant::Bool(b),
        MirConstant::Int(i) => Constant::Int(i),
        MirConstant::Float(f) => Constant::Float(f),
        MirConstant::Duration(ns) => Constant::Duration(ns),
        MirConstant::Intensity(v) => Constant::Intensity(v),
        MirConstant::Color(c) => Constant::Color(lux_bytecode::ColorValue {
            r: c.r,
            g: c.g,
            b: c.b,
        }),
        MirConstant::Angle(a) => Constant::Angle(a),
        MirConstant::Frequency(f) => Constant::Frequency(f),
        MirConstant::Tempo(t) => Constant::Tempo(t),
    }
}
