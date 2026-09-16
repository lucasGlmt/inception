//! A single diagnostic type unifying the per-stage error types from
//! `lux-syntax`, `lux-hir` and `lux-typeck`, so callers of
//! [`crate::check`] get one `Vec<Diagnostic>` regardless of which stage
//! failed.

use lux_syntax::{Span, SyntaxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Syntax,
    Resolve,
    Type,
    /// The compiler itself produced bytecode that failed its own
    /// verifier — a compiler bug, not something the user's source can be
    /// blamed for. See `lux_compiler::compile`.
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: Stage,
    pub message: String,
    /// Not meaningful for `Stage::Internal`: there is no user source
    /// location to blame for a compiler bug, so this is a zero-width span
    /// at the start of the file.
    pub span: Span,
    pub help: Option<String>,
    pub secondary_span: Option<Span>,
}

impl Diagnostic {
    pub(crate) fn internal(message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            stage: Stage::Internal,
            message: message.into(),
            span: Span::at(0),
            help: None,
            secondary_span: None,
        }
    }
}

impl From<SyntaxError> for Diagnostic {
    fn from(err: SyntaxError) -> Self {
        Diagnostic {
            stage: Stage::Syntax,
            message: err.message,
            span: err.span,
            help: err.help,
            secondary_span: None,
        }
    }
}

impl From<lux_hir::HirError> for Diagnostic {
    fn from(err: lux_hir::HirError) -> Self {
        Diagnostic {
            stage: Stage::Resolve,
            message: err.message,
            span: err.span,
            help: err.help,
            secondary_span: err.secondary_span,
        }
    }
}

impl From<lux_typeck::TypeError> for Diagnostic {
    fn from(err: lux_typeck::TypeError) -> Self {
        Diagnostic {
            stage: Stage::Type,
            message: err.message,
            span: err.span,
            help: err.help,
            secondary_span: err.secondary_span,
        }
    }
}

fn into_diagnostics<E: Into<Diagnostic>>(errors: Vec<E>) -> Vec<Diagnostic> {
    errors.into_iter().map(Into::into).collect()
}

pub(crate) fn from_syntax_errors(errors: Vec<SyntaxError>) -> Vec<Diagnostic> {
    into_diagnostics(errors)
}

pub(crate) fn from_hir_errors(errors: Vec<lux_hir::HirError>) -> Vec<Diagnostic> {
    into_diagnostics(errors)
}

pub(crate) fn from_type_errors(errors: Vec<lux_typeck::TypeError>) -> Vec<Diagnostic> {
    into_diagnostics(errors)
}
