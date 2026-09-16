//! Structured syntax diagnostics.
//!
//! Both the lexer and the parser report errors as [`SyntaxError`] rather
//! than bare strings, so downstream tooling (CLI, LSP, tests) can render
//! or filter them without re-parsing error text.

use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub span: Span,
    /// Optional short suggestion for fixing the error, shown as a `help:`
    /// line by renderers that want one.
    pub help: Option<String>,
}

impl SyntaxError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}
