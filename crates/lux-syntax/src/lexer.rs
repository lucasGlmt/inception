//! Hand-written lexer.
//!
//! The lexer never panics on invalid input: unrecognized characters or
//! malformed literals are reported as [`SyntaxError`]s and lexing
//! continues, so a single bad character doesn't hide the rest of the
//! file's diagnostics.

use crate::error::SyntaxError;
use crate::span::Span;
use crate::token::{Token, TokenKind, Unit, keyword};

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    errors: Vec<SyntaxError>,
}

/// Lexes `source` into a token stream (always terminated by
/// [`TokenKind::Eof`]) plus any lexical errors encountered. Lexing never
/// aborts early: it always produces a full token stream so the parser can
/// still attempt recovery.
pub fn tokenize(source: &str) -> (Vec<Token>, Vec<SyntaxError>) {
    let mut lexer = Lexer {
        source,
        bytes: source.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
        errors: Vec::new(),
    };
    lexer.run();
    (lexer.tokens, lexer.errors)
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens
            .push(Token::new(kind, Span::new(start, self.pos)));
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.errors.push(SyntaxError::new(message, span));
    }

    fn run(&mut self) {
        loop {
            self.skip_trivia();
            let Some(b) = self.peek() else { break };
            let start = self.pos;

            match b {
                b'{' => {
                    self.advance();
                    self.push(TokenKind::LBrace, start);
                }
                b'}' => {
                    self.advance();
                    self.push(TokenKind::RBrace, start);
                }
                b'(' => {
                    self.advance();
                    self.push(TokenKind::LParen, start);
                }
                b')' => {
                    self.advance();
                    self.push(TokenKind::RParen, start);
                }
                b',' => {
                    self.advance();
                    self.push(TokenKind::Comma, start);
                }
                b';' => {
                    self.advance();
                    self.push(TokenKind::Semicolon, start);
                }
                b':' => {
                    self.advance();
                    self.push(TokenKind::Colon, start);
                }
                b'.' => {
                    self.advance();
                    self.push(TokenKind::Dot, start);
                }
                b'+' => {
                    self.advance();
                    self.push(TokenKind::Plus, start);
                }
                b'-' => {
                    self.advance();
                    if self.peek() == Some(b'>') {
                        self.advance();
                        self.push(TokenKind::Arrow, start);
                    } else {
                        self.push(TokenKind::Minus, start);
                    }
                }
                b'*' => {
                    self.advance();
                    self.push(TokenKind::Star, start);
                }
                b'/' => {
                    self.advance();
                    self.push(TokenKind::Slash, start);
                }
                b'=' => {
                    self.advance();
                    self.push(TokenKind::Eq, start);
                }
                b'<' => {
                    self.advance();
                    self.push(TokenKind::Less, start);
                }
                b'>' => {
                    self.advance();
                    self.push(TokenKind::Greater, start);
                }
                b'#' => self.lex_hex_color(start),
                b'0'..=b'9' => self.lex_number(start),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident(start),
                _ => {
                    // Advance by one UTF-8 scalar so we don't get stuck on
                    // multi-byte characters, but report the byte position.
                    let ch_len = self.source[self.pos..]
                        .chars()
                        .next()
                        .map(char::len_utf8)
                        .unwrap_or(1);
                    self.pos += ch_len;
                    self.error(
                        format!("unexpected character `{}`", &self.source[start..self.pos]),
                        Span::new(start, self.pos),
                    );
                }
            }
        }

        self.push(TokenKind::Eof, self.pos);
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.advance();
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(b) = self.peek() {
                        if b == b'\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_ident(&mut self, start: usize) {
        while matches!(
            self.peek(),
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')
        ) {
            self.advance();
        }
        let text = &self.source[start..self.pos];
        let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        self.push(kind, start);
    }

    fn lex_number(&mut self, start: usize) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.advance();
        }

        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(b'0'..=b'9')) {
            is_float = true;
            self.advance(); // '.'
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }

        let digits_end = self.pos;
        let digits_text = &self.source[start..digits_end];

        let suffix_start = self.pos;
        let suffix = self.consume_unit_suffix();

        match suffix {
            Some(unit) if !is_float => {
                let value: i64 = digits_text.parse().unwrap_or(0);
                self.push(TokenKind::UnitValue(value, unit), start);
            }
            Some(unit) => {
                self.error(
                    format!(
                        "unit suffix `{}` cannot be applied to a decimal literal; only whole numbers support units",
                        unit.suffix()
                    ),
                    Span::new(suffix_start, self.pos),
                );
                let value: f64 = digits_text.parse().unwrap_or(0.0);
                self.push(TokenKind::Float(value), start);
            }
            None => {
                if is_float {
                    let value: f64 = digits_text.parse().unwrap_or(0.0);
                    self.push(TokenKind::Float(value), start);
                } else {
                    match digits_text.parse::<i64>() {
                        Ok(value) => self.push(TokenKind::Int(value), start),
                        Err(_) => {
                            self.error(
                                format!("integer literal `{digits_text}` is out of range"),
                                Span::new(start, digits_end),
                            );
                            self.push(TokenKind::Int(0), start);
                        }
                    }
                }

                // A number followed directly by other letters that isn't a
                // known unit (e.g. `42xyz`) is a lexical error, but we
                // still consume the trailing identifier-like text so it
                // isn't re-lexed as a separate token.
                if matches!(self.peek(), Some(b'a'..=b'z') | Some(b'A'..=b'Z')) {
                    let bad_start = self.pos;
                    while matches!(
                        self.peek(),
                        Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')
                    ) {
                        self.advance();
                    }
                    self.error(
                        format!(
                            "unknown unit suffix `{}`",
                            &self.source[bad_start..self.pos]
                        ),
                        Span::new(bad_start, self.pos),
                    );
                }
            }
        }
    }

    /// Tries to consume a known unit suffix (`ms`, `s`, `%`, `deg`, `hz`,
    /// `bpm`) starting at the current position. Restores the position and
    /// returns `None` if what follows isn't one of these.
    fn consume_unit_suffix(&mut self) -> Option<Unit> {
        const SUFFIXES: &[(&str, Unit)] = &[
            ("ms", Unit::Milliseconds),
            ("s", Unit::Seconds),
            ("%", Unit::Percent),
            ("deg", Unit::Degrees),
            ("hz", Unit::Hertz),
            ("bpm", Unit::Bpm),
        ];

        let rest = &self.source[self.pos..];
        // Longest match first so `ms` wins over a hypothetical `m`.
        let mut best: Option<(&str, Unit)> = None;
        for (suffix, unit) in SUFFIXES {
            if let Some(after) = rest.strip_prefix(suffix) {
                let next_is_ident_continue = after
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_alphanumeric() || c == '_')
                    .unwrap_or(false);
                // Don't match `s` as a prefix of `secondsish` etc.
                if *suffix != "%" && next_is_ident_continue {
                    continue;
                }
                let is_better = match best {
                    None => true,
                    Some((current, _)) => suffix.len() > current.len(),
                };
                if is_better {
                    best = Some((suffix, *unit));
                }
            }
        }

        let (suffix, unit) = best?;
        self.pos += suffix.len();
        Some(unit)
    }

    fn lex_hex_color(&mut self, start: usize) {
        self.advance(); // '#'
        let digits_start = self.pos;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9') | Some(b'a'..=b'f') | Some(b'A'..=b'F')
        ) {
            self.advance();
        }
        let digits = &self.source[digits_start..self.pos];
        if digits.len() != 6 {
            self.error(
                format!(
                    "invalid hex color literal `#{digits}`; expected exactly 6 hex digits (e.g. `#ff0000`)"
                ),
                Span::new(start, self.pos),
            );
            return;
        }
        let r = u8::from_str_radix(&digits[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&digits[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&digits[4..6], 16).unwrap_or(0);
        self.push(TokenKind::HexColor(r, g, b), start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty(), "unexpected lex errors: {errors:?}");
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn keywords() {
        assert_eq!(
            kinds("scene let mut wait true false"),
            vec![
                TokenKind::Scene,
                TokenKind::Let,
                TokenKind::Mut,
                TokenKind::Wait,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn color_keywords() {
        assert_eq!(
            kinds("red blue green white black"),
            vec![
                TokenKind::Red,
                TokenKind::Blue,
                TokenKind::Green,
                TokenKind::White,
                TokenKind::Black,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn identifiers_include_builtin_type_names() {
        // Builtin type names are not reserved keywords.
        assert_eq!(
            kinds("Duration Intensity foo_bar"),
            vec![
                TokenKind::Ident("Duration".to_string()),
                TokenKind::Ident("Intensity".to_string()),
                TokenKind::Ident("foo_bar".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn numbers_and_units() {
        assert_eq!(
            kinds("0 42 1.5 500ms 2s 0% 50% 100% 45deg 10hz 128bpm"),
            vec![
                TokenKind::Int(0),
                TokenKind::Int(42),
                TokenKind::Float(1.5),
                TokenKind::UnitValue(500, Unit::Milliseconds),
                TokenKind::UnitValue(2, Unit::Seconds),
                TokenKind::UnitValue(0, Unit::Percent),
                TokenKind::UnitValue(50, Unit::Percent),
                TokenKind::UnitValue(100, Unit::Percent),
                TokenKind::UnitValue(45, Unit::Degrees),
                TokenKind::UnitValue(10, Unit::Hertz),
                TokenKind::UnitValue(128, Unit::Bpm),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn hex_colors() {
        assert_eq!(
            kinds("#ff0000 #00ff88"),
            vec![
                TokenKind::HexColor(0xff, 0x00, 0x00),
                TokenKind::HexColor(0x00, 0xff, 0x88),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn dot_for_attribute_assignment() {
        assert_eq!(
            kinds("Washes.intensity"),
            vec![
                TokenKind::Ident("Washes".to_string()),
                TokenKind::Dot,
                TokenKind::Ident("intensity".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn dot_does_not_interfere_with_float_literals() {
        assert_eq!(kinds("1.5"), vec![TokenKind::Float(1.5), TokenKind::Eof]);
    }

    #[test]
    fn invalid_hex_color_reports_error_without_panicking() {
        let (tokens, errors) = tokenize("#zz");
        assert!(!errors.is_empty());
        assert!(matches!(tokens.last().unwrap().kind, TokenKind::Eof));
    }

    #[test]
    fn comments_are_ignored() {
        assert_eq!(
            kinds("// a comment\nlet"),
            vec![TokenKind::Let, TokenKind::Eof]
        );
    }

    #[test]
    fn spans_are_correct() {
        let (tokens, errors) = tokenize("let x");
        assert!(errors.is_empty());
        assert_eq!(tokens[0].span, Span::new(0, 3)); // "let"
        assert_eq!(tokens[1].span, Span::new(4, 5)); // "x"
    }

    #[test]
    fn unexpected_character_does_not_panic() {
        let (tokens, errors) = tokenize("let x = @;");
        assert!(!errors.is_empty());
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Semicolon));
    }

    #[test]
    fn decimal_with_unit_suffix_is_reported() {
        let (tokens, errors) = tokenize("1.5s");
        assert!(!errors.is_empty());
        assert_eq!(tokens[0].kind, TokenKind::Float(1.5));
    }

    #[test]
    fn unknown_suffix_is_reported() {
        let (_tokens, errors) = tokenize("42xyz");
        assert!(!errors.is_empty());
    }

    #[test]
    fn negative_number_is_minus_then_int() {
        assert_eq!(
            kinds("-12"),
            vec![TokenKind::Minus, TokenKind::Int(12), TokenKind::Eof]
        );
    }
}
