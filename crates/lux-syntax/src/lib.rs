//! Lexer, parser and AST for the Lux lighting language.
//!
//! This crate is purely syntactic: it turns source text into an AST and
//! reports lexical/syntactic diagnostics. It has no notion of types,
//! scopes or symbol resolution — that's `lux-hir` and `lux-typeck`'s job
//! — and no runtime dependencies.

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod token;

pub use ast::SourceFile;
pub use error::SyntaxError;
pub use parser::{parse, parse_recovering};
pub use span::{Span, Spanned};
