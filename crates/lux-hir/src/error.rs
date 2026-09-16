//! Structured name-resolution diagnostics.

use lux_syntax::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirError {
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
    /// An earlier, related span (e.g. the first definition when reporting
    /// a duplicate).
    pub secondary_span: Option<Span>,
}

impl HirError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            help: None,
            secondary_span: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_secondary_span(mut self, span: Span) -> Self {
        self.secondary_span = Some(span);
        self
    }
}
