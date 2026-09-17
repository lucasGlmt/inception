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
        target_count: module.target_count,
        rig_contract: module.rig_contract.as_ref().map(|contract| {
            lux_bytecode::PortableRigContract {
                name: contract.name.clone(),
                roles: contract
                    .roles
                    .iter()
                    .map(|role| lux_bytecode::PortableRole {
                        id: lux_bytecode::RoleId(role.id.0),
                        target: to_bytecode_target_id(role.target),
                        name: role.name.clone(),
                        required_capabilities: lux_bytecode::CapabilitySet::from_bits(
                            role.capabilities.bits(),
                        ),
                        cardinality: match role.cardinality {
                            lux_hir::RoleCardinality::GroupNonEmpty => {
                                lux_bytecode::RoleCardinality::GroupNonEmpty
                            }
                        },
                    })
                    .collect(),
            }
        }),
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
        let stack_delta: i8 = match instruction {
            MirInstruction::Const(constant) => {
                let id = self.intern(to_bytecode_constant(*constant));
                code.push(Instruction::Const(id));
                1
            }
            MirInstruction::LoadLocal(id) => {
                code.push(Instruction::LoadLocal(to_bytecode_local_id(*id)));
                1
            }
            MirInstruction::StoreLocal(id) => {
                code.push(Instruction::StoreLocal(to_bytecode_local_id(*id)));
                -1
            }
            MirInstruction::Add => {
                code.push(Instruction::Add);
                -1
            }
            MirInstruction::Sub => {
                code.push(Instruction::Sub);
                -1
            }
            MirInstruction::Mul => {
                code.push(Instruction::Mul);
                -1
            }
            MirInstruction::Div => {
                code.push(Instruction::Div);
                -1
            }
            MirInstruction::Wait => {
                code.push(Instruction::Wait);
                -1
            }
            MirInstruction::Pop => {
                code.push(Instruction::Pop);
                -1
            }
            MirInstruction::SetAttribute { target, attribute } => {
                code.push(Instruction::SetAttribute {
                    target: to_bytecode_target_id(*target),
                    attribute: to_bytecode_attribute(*attribute),
                });
                -1
            }
            MirInstruction::TransitionAttribute { target, attribute } => {
                code.push(Instruction::TransitionAttribute {
                    target: to_bytecode_target_id(*target),
                    attribute: to_bytecode_attribute(*attribute),
                });
                -2
            }
            MirInstruction::BindSignal { target, attribute } => {
                code.push(Instruction::BindSignal {
                    target: to_bytecode_target_id(*target),
                    attribute: to_bytecode_attribute(*attribute),
                });
                -1
            }
            MirInstruction::CallIntrinsic {
                intrinsic,
                arg_count,
            } => {
                code.push(Instruction::CallIntrinsic {
                    intrinsic: to_bytecode_intrinsic(*intrinsic),
                    arg_count: *arg_count,
                });
                1 - *arg_count as i8
            }
            MirInstruction::Index => {
                code.push(Instruction::Index);
                -1
            }
        };

        if stack_delta > 0 {
            *depth += stack_delta as usize;
        } else {
            *depth = depth.saturating_sub((-stack_delta) as usize);
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

fn to_bytecode_target_id(id: lux_hir::TargetId) -> lux_bytecode::TargetId {
    lux_bytecode::TargetId(id.0)
}

fn to_bytecode_attribute(attribute: lux_typeck::Attribute) -> lux_bytecode::Attribute {
    match attribute {
        lux_typeck::Attribute::Intensity => lux_bytecode::Attribute::Intensity,
        lux_typeck::Attribute::Color => lux_bytecode::Attribute::Color,
        lux_typeck::Attribute::Strobe => lux_bytecode::Attribute::Strobe,
    }
}

/// The one boundary conversion between `lux-stdlib`'s compiler-side
/// `IntrinsicId` and `lux-bytecode`'s independent, runtime-facing copy
/// (see that type's module doc for why the duplication exists) — same
/// idiom as `to_bytecode_attribute`/`to_value_type` just above.
fn to_bytecode_intrinsic(id: lux_stdlib::IntrinsicId) -> lux_bytecode::IntrinsicId {
    use lux_bytecode::IntrinsicId as B;
    use lux_stdlib::IntrinsicId as S;
    match id {
        S::MathSin => B::MathSin,
        S::MathCos => B::MathCos,
        S::MathAbsInt => B::MathAbsInt,
        S::MathAbsFloat => B::MathAbsFloat,
        S::MathMinInt => B::MathMinInt,
        S::MathMinFloat => B::MathMinFloat,
        S::MathMaxInt => B::MathMaxInt,
        S::MathMaxFloat => B::MathMaxFloat,
        S::MathClampInt => B::MathClampInt,
        S::MathClampFloat => B::MathClampFloat,
        S::MathLerp => B::MathLerp,
        S::ColorRgb => B::ColorRgb,
        S::ColorMix => B::ColorMix,
        S::ColorHsv => B::ColorHsv,
        S::SignalConstantInt => B::SignalConstantInt,
        S::SignalConstantFloat => B::SignalConstantFloat,
        S::SignalConstantAngle => B::SignalConstantAngle,
        S::SignalConstantIntensity => B::SignalConstantIntensity,
        S::SignalConstantColor => B::SignalConstantColor,
        S::EffectsSine => B::EffectsSine,
        S::EffectsTriangle => B::EffectsTriangle,
        S::EffectsSaw => B::EffectsSaw,
        S::EffectsSquare => B::EffectsSquare,
        S::SignalRangeFloat => B::SignalRangeFloat,
        S::SignalRangeIntensity => B::SignalRangeIntensity,
        S::SignalRangeAngle => B::SignalRangeAngle,
        S::SignalPhase => B::SignalPhase,
        S::SignalSpreadFloat => B::SignalSpreadFloat,
        S::SignalSpreadInt => B::SignalSpreadInt,
        S::SignalSpreadAngle => B::SignalSpreadAngle,
        S::SignalSpreadIntensity => B::SignalSpreadIntensity,
        S::SignalSpreadColor => B::SignalSpreadColor,
        S::SignalInvert => B::SignalInvert,
        S::SequenceOfInt => B::SequenceOfInt,
        S::SequenceOfFloat => B::SequenceOfFloat,
        S::SequenceOfAngle => B::SequenceOfAngle,
        S::SequenceOfIntensity => B::SequenceOfIntensity,
        S::SequenceOfColor => B::SequenceOfColor,
        S::SequenceLengthInt => B::SequenceLengthInt,
        S::SequenceLengthFloat => B::SequenceLengthFloat,
        S::SequenceLengthAngle => B::SequenceLengthAngle,
        S::SequenceLengthIntensity => B::SequenceLengthIntensity,
        S::SequenceLengthColor => B::SequenceLengthColor,
        S::EffectsStepInt => B::EffectsStepInt,
        S::EffectsStepFloat => B::EffectsStepFloat,
        S::EffectsStepAngle => B::EffectsStepAngle,
        S::EffectsStepIntensity => B::EffectsStepIntensity,
        S::EffectsStepColor => B::EffectsStepColor,
    }
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
        Type::Signal(elem) => lux_bytecode::ValueType::Signal(to_scalar_value_type(elem)),
        Type::Sequence(elem) => {
            lux_bytecode::ValueType::Sequence(to_sequence_scalar_value_type(elem))
        }
    }
}

fn to_scalar_value_type(elem: lux_typeck::SignalElement) -> lux_bytecode::ScalarValueType {
    match elem {
        lux_typeck::SignalElement::Int => lux_bytecode::ScalarValueType::Int,
        lux_typeck::SignalElement::Float => lux_bytecode::ScalarValueType::Float,
        lux_typeck::SignalElement::Angle => lux_bytecode::ScalarValueType::Angle,
        lux_typeck::SignalElement::Intensity => lux_bytecode::ScalarValueType::Intensity,
        lux_typeck::SignalElement::Color => lux_bytecode::ScalarValueType::Color,
    }
}

fn to_sequence_scalar_value_type(
    elem: lux_typeck::SequenceElement,
) -> lux_bytecode::ScalarValueType {
    match elem {
        lux_typeck::SequenceElement::Int => lux_bytecode::ScalarValueType::Int,
        lux_typeck::SequenceElement::Float => lux_bytecode::ScalarValueType::Float,
        lux_typeck::SequenceElement::Angle => lux_bytecode::ScalarValueType::Angle,
        lux_typeck::SequenceElement::Intensity => lux_bytecode::ScalarValueType::Intensity,
        lux_typeck::SequenceElement::Color => lux_bytecode::ScalarValueType::Color,
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
