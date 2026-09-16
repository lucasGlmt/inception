//! Human-readable disassembly, for debugging/tests only — the future VM
//! must not depend on this.

use std::fmt::Write as _;

use crate::instruction::Instruction;
use crate::module::BytecodeModule;

/// Renders `module` as readable assembly-like text, e.g.:
///
/// ```text
/// fn #0 main
///   locals: 1
///   stack: 1
///
/// 0000 CONST       #0 Duration(1000000000)
/// 0001 STORE_LOCAL 0
/// 0002 LOAD_LOCAL  0
/// 0003 WAIT
/// 0004 RETURN
/// ```
pub fn disassemble(module: &BytecodeModule) -> String {
    let mut out = String::new();

    for (index, function) in module.functions.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }

        let name = function.debug_name.as_deref().unwrap_or("<anonymous>");
        let _ = writeln!(out, "fn #{} {name}", function.id.0);
        let _ = writeln!(out, "  locals: {}", function.locals.len());
        let _ = writeln!(out, "  stack: {}", function.max_stack);
        out.push('\n');

        for (pc, instruction) in function.code.iter().enumerate() {
            let _ = write!(out, "{pc:04} ");
            write_instruction(&mut out, module, *instruction);
            out.push('\n');
        }
    }

    out
}

fn write_instruction(out: &mut String, module: &BytecodeModule, instruction: Instruction) {
    match instruction {
        Instruction::Const(id) => {
            let value = module
                .constants
                .get(id.0 as usize)
                .map(|c| format!("{c:?}"))
                .unwrap_or_else(|| "<invalid>".to_string());
            let _ = write!(out, "{:<11} #{} {value}", "CONST", id.0);
        }
        Instruction::LoadLocal(id) => {
            let _ = write!(out, "{:<11} {}", "LOAD_LOCAL", id.0);
        }
        Instruction::StoreLocal(id) => {
            let _ = write!(out, "{:<11} {}", "STORE_LOCAL", id.0);
        }
        Instruction::Add => {
            let _ = write!(out, "ADD");
        }
        Instruction::Sub => {
            let _ = write!(out, "SUB");
        }
        Instruction::Mul => {
            let _ = write!(out, "MUL");
        }
        Instruction::Div => {
            let _ = write!(out, "DIV");
        }
        Instruction::Wait => {
            let _ = write!(out, "WAIT");
        }
        Instruction::Call(id) => {
            let _ = write!(out, "{:<11} #{}", "CALL", id.0);
        }
        Instruction::CallIntrinsic {
            intrinsic,
            arg_count,
        } => {
            let _ = write!(
                out,
                "{:<11} {intrinsic:?} argc={arg_count}",
                "CALL_INTRINSIC"
            );
        }
        Instruction::Return => {
            let _ = write!(out, "RETURN");
        }
        Instruction::Pop => {
            let _ = write!(out, "POP");
        }
        Instruction::SetAttribute { target, attribute } => {
            let _ = write!(out, "{:<11} #{} {attribute:?}", "SET_ATTRIBUTE", target.0);
        }
        Instruction::TransitionAttribute { target, attribute } => {
            let _ = write!(
                out,
                "{:<20} #{} {attribute:?}",
                "TRANSITION_ATTRIBUTE", target.0
            );
        }
    }
}

impl std::fmt::Display for BytecodeModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&disassemble(self))
    }
}
