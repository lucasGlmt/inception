use crate::ids::{ConstantId, FunctionId, LocalId};
use crate::instruction::Instruction;
use crate::module::{BytecodeModule, BytecodeVersion, Function};
use crate::value::{Constant, ValueType};
use crate::verify::{VerificationErrorKind, verify};

fn empty_module() -> BytecodeModule {
    BytecodeModule {
        version: BytecodeVersion::CURRENT,
        constants: Vec::new(),
        functions: Vec::new(),
        entry: None,
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
