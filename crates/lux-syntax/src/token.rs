//! Token kinds produced by the lexer.
//!
//! ## Design decision: keywords vs. builtin type names
//!
//! - Structural keywords (`scene`, `let`, `mut`, `wait`) and literal
//!   keywords (`true`, `false`, and the named colors `red`, `blue`,
//!   `green`, `white`, `black`) are lexed as **dedicated token kinds**.
//!   They can never be used as identifiers.
//! - Builtin scalar type names (`Bool`, `Int`, `Float`, `Duration`,
//!   `Intensity`, `Color`, `Angle`, `Frequency`, `Tempo`) are **not**
//!   reserved. They are lexed as plain [`TokenKind::Ident`] and are only
//!   given meaning by the parser when they appear in type-annotation
//!   position (after `:`). This keeps the lexer's keyword table small and
//!   lets later compiler stages (not the lexer) own the set of known
//!   types.
//!
//! ## Design decision: unit-suffixed numeric literals
//!
//! A number immediately followed (no whitespace) by a known unit suffix
//! (`ms`, `s`, `%`, `deg`, `hz`, `bpm`) is lexed as a single literal token
//! carrying both the numeric value and the unit, rather than as a number
//! token followed by an identifier token. This means unit-suffixed
//! literals are always integers in this MVP (no `1.5s`); a bare decimal
//! literal such as `1.5` has no unit and lexes as [`TokenKind::Float`].

use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Milliseconds,
    Seconds,
    Percent,
    Degrees,
    Hertz,
    Bpm,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Milliseconds => "ms",
            Unit::Seconds => "s",
            Unit::Percent => "%",
            Unit::Degrees => "deg",
            Unit::Hertz => "hz",
            Unit::Bpm => "bpm",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    Float(f64),
    /// An integer literal immediately followed by a unit suffix, e.g.
    /// `500ms`, `50%`, `45deg`, `10hz`, `128bpm`.
    UnitValue(i64, Unit),
    HexColor(u8, u8, u8),

    // Identifiers
    Ident(String),

    // Literal keywords
    True,
    False,
    Red,
    Blue,
    Green,
    White,
    Black,

    // Structural keywords
    Scene,
    Let,
    Mut,
    Wait,
    Over,
    Rig,
    Contract,
    Role,
    Group,
    Import,

    // Punctuation
    LBrace,
    RBrace,
    LParen,
    RParen,
    Comma,
    Semicolon,
    Colon,
    Dot,
    Plus,
    Minus,
    Arrow,
    LeftArrow,
    Star,
    Slash,
    Eq,
    Less,
    Greater,

    Eof,
}

impl TokenKind {
    /// Human-readable description used in diagnostics, e.g.
    /// "expected `;`, found `wait`".
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Int(v) => format!("integer `{v}`"),
            TokenKind::Float(v) => format!("float `{v}`"),
            TokenKind::UnitValue(v, unit) => format!("`{v}{}`", unit.suffix()),
            TokenKind::HexColor(r, g, b) => format!("`#{r:02x}{g:02x}{b:02x}`"),
            TokenKind::Ident(name) => format!("identifier `{name}`"),
            TokenKind::True => "`true`".to_string(),
            TokenKind::False => "`false`".to_string(),
            TokenKind::Red => "`red`".to_string(),
            TokenKind::Blue => "`blue`".to_string(),
            TokenKind::Green => "`green`".to_string(),
            TokenKind::White => "`white`".to_string(),
            TokenKind::Black => "`black`".to_string(),
            TokenKind::Scene => "`scene`".to_string(),
            TokenKind::Let => "`let`".to_string(),
            TokenKind::Mut => "`mut`".to_string(),
            TokenKind::Wait => "`wait`".to_string(),
            TokenKind::Over => "`over`".to_string(),
            TokenKind::Rig => "`rig`".to_string(),
            TokenKind::Contract => "`contract`".to_string(),
            TokenKind::Role => "`role`".to_string(),
            TokenKind::Group => "`Group`".to_string(),
            TokenKind::Import => "`import`".to_string(),
            TokenKind::LBrace => "`{`".to_string(),
            TokenKind::RBrace => "`}`".to_string(),
            TokenKind::LParen => "`(`".to_string(),
            TokenKind::RParen => "`)`".to_string(),
            TokenKind::Comma => "`,`".to_string(),
            TokenKind::Semicolon => "`;`".to_string(),
            TokenKind::Colon => "`:`".to_string(),
            TokenKind::Dot => "`.`".to_string(),
            TokenKind::Plus => "`+`".to_string(),
            TokenKind::Minus => "`-`".to_string(),
            TokenKind::Arrow => "`->`".to_string(),
            TokenKind::LeftArrow => "`<-`".to_string(),
            TokenKind::Star => "`*`".to_string(),
            TokenKind::Slash => "`/`".to_string(),
            TokenKind::Eq => "`=`".to_string(),
            TokenKind::Less => "`<`".to_string(),
            TokenKind::Greater => "`>`".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Maps identifier text to a reserved keyword token, if any.
pub fn keyword(ident: &str) -> Option<TokenKind> {
    Some(match ident {
        "scene" => TokenKind::Scene,
        "let" => TokenKind::Let,
        "mut" => TokenKind::Mut,
        "wait" => TokenKind::Wait,
        "over" => TokenKind::Over,
        "rig" => TokenKind::Rig,
        "contract" => TokenKind::Contract,
        "role" => TokenKind::Role,
        "Group" => TokenKind::Group,
        "import" => TokenKind::Import,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "red" => TokenKind::Red,
        "blue" => TokenKind::Blue,
        "green" => TokenKind::Green,
        "white" => TokenKind::White,
        "black" => TokenKind::Black,
        _ => return None,
    })
}
