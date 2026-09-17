use crate::attribute::Attribute;
use crate::ids::{ConstantId, FunctionId, LocalId, TargetId};
use crate::instruction::Instruction;
use crate::intrinsic::IntrinsicId;
use crate::module::{BytecodeModule, BytecodeVersion, Function};
use crate::value::{Constant, ValueType};
use crate::verify::{VerificationErrorKind, verify};

fn empty_module() -> BytecodeModule {
    BytecodeModule {
        version: BytecodeVersion::CURRENT,
        constants: Vec::new(),
        functions: Vec::new(),
        entry: None,
        target_count: 0,
        rig_contract: None,
    }
}

fn function_with(
    constants: Vec<Constant>,
    locals: Vec<ValueType>,
    code: Vec<Instruction>,
    max_stack: u16,
) -> BytecodeModule {
    let mut module = empty_module();
    module.constants = constants;
    module.functions.push(Function {
        id: FunctionId(0),
        code,
        locals,
        max_stack,
        debug_name: Some("main".to_string()),
    });
    module.entry = Some(FunctionId(0));
    module
}

#[test]
fn valid_wait_duration_passes() {
    let module = function_with(
        vec![Constant::Duration(1_000_000_000)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Wait,
            Instruction::Return,
        ],
        1,
    );
    verify(&module).expect("valid module should verify");
}

#[test]
fn stack_underflow_on_add_with_empty_stack_is_rejected() {
    let module = function_with(
        vec![],
        vec![],
        vec![Instruction::Add, Instruction::Return],
        2,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::StackUnderflow)
    );
}

#[test]
fn waiting_on_a_color_is_rejected() {
    let module = function_with(
        vec![Constant::Color(crate::value::ColorValue {
            r: 255,
            g: 0,
            b: 0,
        })],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Wait,
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidWaitOperand(ValueType::Color))
    );
}

#[test]
fn out_of_range_local_is_rejected() {
    let module = function_with(
        vec![],
        vec![ValueType::Duration],
        vec![Instruction::LoadLocal(LocalId(42)), Instruction::Return],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidLocalId(LocalId(42)))
    );
}

#[test]
fn out_of_range_constant_is_rejected() {
    let module = function_with(
        vec![Constant::Int(1), Constant::Int(2)],
        vec![],
        vec![Instruction::Const(ConstantId(400)), Instruction::Return],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidConstantId(ConstantId(400)))
    );
}

#[test]
fn call_to_missing_function_is_rejected() {
    let module = function_with(
        vec![],
        vec![],
        vec![Instruction::Call(FunctionId(10)), Instruction::Return],
        0,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidFunctionId(FunctionId(10)))
    );
}

#[test]
fn store_local_type_mismatch_is_rejected() {
    let module = function_with(
        vec![Constant::Color(crate::value::ColorValue {
            r: 0,
            g: 0,
            b: 0,
        })],
        vec![ValueType::Duration],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::StoreLocal(LocalId(0)),
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| e.kind
        == VerificationErrorKind::TypeMismatch {
            expected: ValueType::Duration,
            found: ValueType::Color,
        }));
}

#[test]
fn arithmetic_on_incompatible_types_is_rejected() {
    let module = function_with(
        vec![Constant::Duration(1), Constant::Intensity(1)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::Add,
            Instruction::Return,
        ],
        2,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| e.kind
        == VerificationErrorKind::InvalidArithmeticOperands {
            lhs: ValueType::Duration,
            rhs: ValueType::Intensity,
        }));
}

#[test]
fn returning_with_a_nonempty_stack_is_rejected() {
    let module = function_with(
        vec![Constant::Int(1)],
        vec![],
        vec![Instruction::Const(ConstantId(0)), Instruction::Return],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidReturn)
    );
}

#[test]
fn missing_final_return_is_rejected() {
    let module = function_with(vec![], vec![], vec![Instruction::Pop], 0);
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidReturn)
    );
}

#[test]
fn declared_max_stack_too_small_is_rejected() {
    let module = function_with(
        vec![Constant::Int(1), Constant::Int(2)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::Add,
            Instruction::Return,
        ],
        // Declares room for only 1 value, but 2 constants get pushed
        // before ADD consumes them.
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::StackOverflow)
    );
}

#[test]
fn unsupported_version_is_rejected() {
    let mut module = empty_module();
    module.version = BytecodeVersion {
        major: 99,
        minor: 0,
    };
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, VerificationErrorKind::UnsupportedVersion(_)))
    );
}

#[test]
fn invalid_entry_is_rejected() {
    let mut module = empty_module();
    module.entry = Some(FunctionId(0));
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidEntry(FunctionId(0)))
    );
}

#[test]
fn function_id_not_matching_its_position_is_rejected() {
    let mut module = empty_module();
    module.functions.push(Function {
        id: FunctionId(99),
        code: vec![Instruction::Return],
        locals: vec![],
        max_stack: 0,
        debug_name: None,
    });
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidFunctionId(FunctionId(99)))
    );
}

#[test]
fn empty_module_verifies() {
    verify(&empty_module()).expect("an empty module has nothing to reject");
}

#[test]
fn reports_multiple_errors_in_one_pass() {
    let module = function_with(
        vec![],
        vec![],
        vec![Instruction::LoadLocal(LocalId(0)), Instruction::Add],
        0,
    );
    let errors = verify(&module).expect_err("should be rejected");
    // Missing RETURN, LoadLocal out of range, and Add underflow (only lhs
    // pushed, then errored, so rhs pop also underflows) should all surface.
    assert!(
        errors.len() >= 2,
        "expected multiple diagnostics, got {errors:?}"
    );
}

#[test]
fn disassemble_does_not_panic_on_invalid_module() {
    let module = function_with(vec![], vec![], vec![Instruction::Const(ConstantId(5))], 0);
    let _ = crate::disasm::disassemble(&module);
}

#[test]
fn valid_set_attribute_passes() {
    let mut module = function_with(
        vec![Constant::Intensity(32768)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::SetAttribute {
                target: TargetId(0),
                attribute: Attribute::Intensity,
            },
            Instruction::Return,
        ],
        1,
    );
    module.target_count = 1;
    verify(&module).expect("valid module should verify");
}

#[test]
fn set_attribute_with_wrong_value_type_is_rejected() {
    // The exact scenario from item 36 of the task brief: a Duration
    // pushed for an Intensity attribute must be rejected before
    // execution.
    let mut module = function_with(
        vec![Constant::Duration(1_000_000_000)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::SetAttribute {
                target: TargetId(0),
                attribute: Attribute::Intensity,
            },
            Instruction::Return,
        ],
        1,
    );
    module.target_count = 1;
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| e.kind
        == VerificationErrorKind::TypeMismatch {
            expected: ValueType::Intensity,
            found: ValueType::Duration,
        }));
}

#[test]
fn set_attribute_with_out_of_range_target_is_rejected() {
    let module = function_with(
        vec![Constant::Intensity(0)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::SetAttribute {
                target: TargetId(0),
                attribute: Attribute::Intensity,
            },
            Instruction::Return,
        ],
        1,
    );
    // target_count left at 0: TargetId(0) is out of range.
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidTargetId(TargetId(0)))
    );
}

#[test]
fn set_attribute_on_empty_stack_is_a_stack_underflow() {
    let mut module = function_with(
        vec![],
        vec![],
        vec![
            Instruction::SetAttribute {
                target: TargetId(0),
                attribute: Attribute::Intensity,
            },
            Instruction::Return,
        ],
        0,
    );
    module.target_count = 1;
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::StackUnderflow)
    );
}

#[test]
fn valid_intensity_transition_passes() {
    let mut module = function_with(
        vec![
            Constant::Intensity(u16::MAX),
            Constant::Duration(2_000_000_000),
        ],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::TransitionAttribute {
                target: TargetId(0),
                attribute: Attribute::Intensity,
            },
            Instruction::Return,
        ],
        2,
    );
    module.target_count = 1;
    verify(&module).unwrap();
}

#[test]
fn transition_rejects_wrong_value_and_duration_types() {
    let mut module = function_with(
        vec![
            Constant::Color(crate::ColorValue { r: 255, g: 0, b: 0 }),
            Constant::Intensity(32768),
        ],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::TransitionAttribute {
                target: TargetId(0),
                attribute: Attribute::Intensity,
            },
            Instruction::Return,
        ],
        2,
    );
    module.target_count = 1;
    let errors = verify(&module).unwrap_err();
    assert!(errors.iter().any(|error| error.kind
        == VerificationErrorKind::TypeMismatch {
            expected: ValueType::Duration,
            found: ValueType::Intensity,
        }));
    assert!(errors.iter().any(|error| error.kind
        == VerificationErrorKind::TypeMismatch {
            expected: ValueType::Intensity,
            found: ValueType::Color,
        }));
}

#[test]
fn valid_call_intrinsic_passes() {
    let module = function_with(
        vec![Constant::Angle(90_000)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::MathSin,
                arg_count: 1,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        1,
    );
    verify(&module).expect("valid module should verify");
}

#[test]
fn call_intrinsic_arity_mismatch_is_rejected() {
    let module = function_with(
        vec![Constant::Angle(90_000)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::MathSin,
                arg_count: 2,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| e.kind
        == VerificationErrorKind::IntrinsicArityMismatch {
            intrinsic: IntrinsicId::MathSin,
            declared: 2,
            expected: 1,
        }));
}

#[test]
fn call_intrinsic_wrong_operand_type_is_rejected() {
    let module = function_with(
        vec![Constant::Color(crate::value::ColorValue {
            r: 1,
            g: 2,
            b: 3,
        })],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::MathSin,
                arg_count: 1,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| e.kind
        == VerificationErrorKind::InvalidIntrinsicOperand {
            intrinsic: IntrinsicId::MathSin,
            index: 0,
            expected: ValueType::Angle,
            found: ValueType::Color,
        }));
}

#[test]
fn call_intrinsic_on_empty_stack_is_a_stack_underflow() {
    let module = function_with(
        vec![],
        vec![],
        vec![
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::MathSin,
                arg_count: 1,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::StackUnderflow)
    );
}

#[test]
fn call_intrinsic_pushes_its_return_type() {
    let module = function_with(
        vec![Constant::Int(255), Constant::Int(120), Constant::Int(20)],
        vec![ValueType::Color],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::Const(ConstantId(2)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::ColorRgb,
                arg_count: 3,
            },
            Instruction::StoreLocal(LocalId(0)),
            Instruction::Return,
        ],
        3,
    );
    verify(&module).expect("valid module should verify");
}

#[test]
fn signal_constant_pushes_the_matching_signal_type() {
    use crate::value::ScalarValueType;

    let module = function_with(
        vec![Constant::Intensity(32767)],
        vec![ValueType::Signal(ScalarValueType::Intensity)],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SignalConstantIntensity,
                arg_count: 1,
            },
            Instruction::StoreLocal(LocalId(0)),
            Instruction::Return,
        ],
        1,
    );
    verify(&module).expect("valid module should verify");
}

#[test]
fn signal_of_one_element_type_is_not_a_signal_of_another() {
    use crate::value::ScalarValueType;

    // A local declared `Signal<Color>` but stored a `Signal<Intensity>`
    // (mismatched element type) must be rejected, just like any other
    // `StoreLocal` type mismatch.
    let module = function_with(
        vec![Constant::Intensity(32767)],
        vec![ValueType::Signal(ScalarValueType::Color)],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SignalConstantIntensity,
                arg_count: 1,
            },
            Instruction::StoreLocal(LocalId(0)),
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| matches!(
        e.kind,
        VerificationErrorKind::TypeMismatch {
            expected: ValueType::Signal(ScalarValueType::Color),
            found: ValueType::Signal(ScalarValueType::Intensity),
        }
    )));
}

#[test]
fn signal_is_not_its_element_type() {
    use crate::value::ScalarValueType;

    // A local declared plain `Intensity` but stored a `Signal<Intensity>`
    // must be rejected — no implicit unwrap at the bytecode level either.
    let module = function_with(
        vec![Constant::Intensity(32767)],
        vec![ValueType::Intensity],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SignalConstantIntensity,
                arg_count: 1,
            },
            Instruction::StoreLocal(LocalId(0)),
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| matches!(
        e.kind,
        VerificationErrorKind::TypeMismatch {
            expected: ValueType::Intensity,
            found: ValueType::Signal(ScalarValueType::Intensity),
        }
    )));
}

#[test]
fn valid_sequence_of_construction_passes() {
    let module = function_with(
        vec![Constant::Int(1), Constant::Int(2), Constant::Int(3)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::Const(ConstantId(2)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceOfInt,
                arg_count: 3,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        3,
    );
    verify(&module).expect("valid module should verify");
}

#[test]
fn sequence_of_with_zero_arg_count_is_rejected() {
    let module = function_with(
        vec![],
        vec![],
        vec![
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceOfInt,
                arg_count: 0,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        1,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(
        |e| e.kind == VerificationErrorKind::EmptyVariadicIntrinsic(IntrinsicId::SequenceOfInt)
    ));
}

#[test]
fn sequence_of_with_mismatched_element_type_is_rejected() {
    let module = function_with(
        vec![
            Constant::Int(1),
            Constant::Color(crate::value::ColorValue { r: 255, g: 0, b: 0 }),
        ],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceOfInt,
                arg_count: 2,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        2,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(errors.iter().any(|e| matches!(
        e.kind,
        VerificationErrorKind::InvalidVariadicIntrinsicOperand {
            intrinsic: IntrinsicId::SequenceOfInt,
            expected: ValueType::Int,
            found: ValueType::Color,
            ..
        }
    )));
}

#[test]
fn valid_sequence_index_passes() {
    let module = function_with(
        vec![Constant::Int(1), Constant::Int(2), Constant::Int(0)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceOfInt,
                arg_count: 2,
            },
            Instruction::Const(ConstantId(2)),
            Instruction::Index,
            Instruction::Pop,
            Instruction::Return,
        ],
        3,
    );
    verify(&module).expect("valid module should verify");
}

#[test]
fn indexing_a_non_sequence_is_rejected() {
    let module = function_with(
        vec![Constant::Int(1), Constant::Int(0)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::Const(ConstantId(1)),
            Instruction::Index,
            Instruction::Pop,
            Instruction::Return,
        ],
        2,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::IndexOnNonSequence(ValueType::Int))
    );
}

#[test]
fn indexing_with_a_non_int_index_is_rejected() {
    let module = function_with(
        vec![
            Constant::Int(1),
            Constant::Color(crate::value::ColorValue { r: 0, g: 0, b: 0 }),
        ],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceOfInt,
                arg_count: 1,
            },
            Instruction::Const(ConstantId(1)),
            Instruction::Index,
            Instruction::Pop,
            Instruction::Return,
        ],
        2,
    );
    let errors = verify(&module).expect_err("should be rejected");
    assert!(
        errors
            .iter()
            .any(|e| e.kind == VerificationErrorKind::InvalidIndexOperand(ValueType::Color))
    );
}

#[test]
fn valid_sequence_length_passes() {
    let module = function_with(
        vec![Constant::Int(1)],
        vec![],
        vec![
            Instruction::Const(ConstantId(0)),
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceOfInt,
                arg_count: 1,
            },
            Instruction::CallIntrinsic {
                intrinsic: IntrinsicId::SequenceLengthInt,
                arg_count: 1,
            },
            Instruction::Pop,
            Instruction::Return,
        ],
        2,
    );
    verify(&module).expect("valid module should verify");
}
