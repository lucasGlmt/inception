//! Hand-written recursive-descent parser with precedence climbing for
//! binary expressions.
//!
//! ## Error recovery
//!
//! The parser never panics and never gets stuck. Every `parse_*` method
//! that builds a required node (a statement, an expression, an
//! identifier) always returns *something*: on malformed input it records
//! a [`SyntaxError`] and substitutes a placeholder node (e.g. an empty
//! identifier, an `Int(0)` literal) so the caller can keep building the
//! tree. `lux-syntax::parse` still reports `Err` whenever any error was
//! recorded, regardless of whether a full tree was produced — the
//! placeholder tree exists only so a single malformed construct doesn't
//! prevent later, unrelated errors in the same file from being reported.
//!
//! Statement and item loops additionally guard against zero-progress
//! steps (a `parse_*` call that fails without consuming a token) by
//! force-advancing one token, which guarantees termination on any input.

use crate::ast::*;
use crate::error::SyntaxError;
use crate::lexer::tokenize;
use crate::span::Span;
use crate::token::{Token, TokenKind, Unit};

/// Parses a full Lux source file. On success returns the AST; on failure
/// returns every diagnostic collected while lexing and parsing (there may
/// be more than one).
pub fn parse(source: &str) -> Result<SourceFile, Vec<SyntaxError>> {
    let (file, errors) = parse_recovering(source);

    if errors.is_empty() {
        Ok(file)
    } else {
        Err(errors)
    }
}

/// Parses a Lux file and always returns the recovered syntax tree together
/// with its diagnostics. This is the parser entry point used by IDE tooling:
/// compiler callers should continue to use [`parse`], which rejects a tree
/// containing placeholders.
pub fn parse_recovering(source: &str) -> (SourceFile, Vec<SyntaxError>) {
    let (tokens, lex_errors) = tokenize(source);
    let mut parser = Parser::new(tokens);
    let file = parser.parse_source_file();

    let mut errors = lex_errors;
    errors.extend(parser.errors);

    (file, errors)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<SyntaxError>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    fn peek(&self) -> &Token {
        // `tokenize` always terminates the stream with `Eof`, and `advance`
        // never steps past it, so this index is always in bounds.
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    /// Looks `n` tokens ahead without consuming anything. Falls back to
    /// the trailing `Eof` if `n` runs past the end of the stream.
    fn peek_nth_kind(&self, n: usize) -> &TokenKind {
        &self
            .tokens
            .get(self.pos + n)
            .unwrap_or_else(|| self.tokens.last().expect("token stream always has an Eof"))
            .kind
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek_kind() == &kind
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.errors.push(SyntaxError::new(message, span));
    }

    fn expect(&mut self, kind: TokenKind, context: &str) -> Option<Token> {
        if self.check(kind) {
            Some(self.advance())
        } else {
            let tok = self.peek().clone();
            self.error(
                format!("{context}, found {}", tok.kind.describe()),
                tok.span,
            );
            None
        }
    }

    fn expect_identifier(&mut self, context: &str) -> Identifier {
        if let TokenKind::Ident(_) = self.peek_kind() {
            let tok = self.advance();
            let TokenKind::Ident(name) = tok.kind else {
                unreachable!()
            };
            Identifier {
                name,
                span: tok.span,
            }
        } else {
            let tok = self.peek().clone();
            self.error(
                format!("expected {context}, found {}", tok.kind.describe()),
                tok.span,
            );
            Identifier {
                name: String::new(),
                span: Span::at(tok.span.start),
            }
        }
    }

    /// Parses a type name, optionally followed by one bracketed type
    /// argument (`Signal<Intensity>`). `<`/`>` are unambiguous here: Lux
    /// defines no comparison operators, so a standalone `Less`/`Greater`
    /// token in type position can only ever start/end a type argument —
    /// same reasoning already relied on for `Group<Color + Intensity>` in
    /// `parse_role_decl`. Whether a given name is actually allowed to carry
    /// a type argument (today, only `Signal`) is decided later, in
    /// `lux-typeck` — this parses the syntax generally.
    fn expect_type_name(&mut self) -> TypeName {
        let mut result = if let TokenKind::Ident(_) = self.peek_kind() {
            let tok = self.advance();
            let TokenKind::Ident(name) = tok.kind else {
                unreachable!()
            };
            TypeName {
                name,
                span: tok.span,
                type_args: Vec::new(),
            }
        } else {
            let tok = self.peek().clone();
            self.error(
                format!("expected type name, found {}", tok.kind.describe()),
                tok.span,
            );
            TypeName {
                name: String::new(),
                span: Span::at(tok.span.start),
                type_args: Vec::new(),
            }
        };

        if self.check(TokenKind::Less) {
            self.advance();
            let arg = self.expect_type_name();
            let close = self.expect(TokenKind::Greater, "expected `>` after type argument");
            let end = close.map(|t| t.span.end).unwrap_or(arg.span.end);
            result.span = Span::new(result.span.start, end);
            result.type_args.push(arg);
        }

        result
    }

    fn parse_source_file(&mut self) -> SourceFile {
        let mut items = Vec::new();
        while !self.at_eof() {
            let start_pos = self.pos;
            if let Some(item) = self.parse_item() {
                items.push(item);
            }
            if self.pos == start_pos {
                self.advance();
            }
        }
        SourceFile { items }
    }

    fn parse_item(&mut self) -> Option<Item> {
        if self.check(TokenKind::Scene) {
            Some(Item::Scene(self.parse_scene_decl()))
        } else if self.check(TokenKind::Rig) {
            Some(Item::RigContract(self.parse_rig_contract_decl()))
        } else if self.check(TokenKind::Import) {
            Some(Item::Import(self.parse_import_decl()))
        } else {
            let tok = self.peek().clone();
            self.error(
                format!(
                    "expected `scene`, `rig` or `import`, found {}",
                    tok.kind.describe()
                ),
                tok.span,
            );
            None
        }
    }

    /// `import` IDENT (`.` IDENT)* `;` — no wildcards (`std.Math.*`), no
    /// aliasing.
    fn parse_import_decl(&mut self) -> ImportDecl {
        let import_tok = self.advance(); // `import`
        let mut path = vec![self.expect_identifier("expected module name after `import`")];
        while self.check(TokenKind::Dot) {
            self.advance();
            if self.check(TokenKind::Star) {
                let star = self.advance();
                self.error(
                    "wildcard imports (`import a.b.*;`) are not supported".to_string(),
                    star.span,
                );
                break;
            }
            path.push(self.expect_identifier("expected module path segment after `.`"));
        }
        let semi = self.expect(TokenKind::Semicolon, "expected `;` after import");
        let end = semi
            .map(|t| t.span.end)
            .unwrap_or_else(|| path.last().expect("path always has >=1 segment").span.end);
        ImportDecl {
            path,
            span: Span::new(import_tok.span.start, end),
        }
    }

    fn parse_rig_contract_decl(&mut self) -> RigContractDecl {
        let rig = self.advance();
        self.expect(TokenKind::Contract, "expected `contract` after `rig`");
        let name = self.expect_identifier("expected rig contract name");
        self.expect(TokenKind::LBrace, "expected `{` to start rig contract");
        let mut roles = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_eof() {
            let start_pos = self.pos;
            roles.push(self.parse_role_decl());
            if self.pos == start_pos {
                self.advance();
            }
        }
        let close = self.expect(TokenKind::RBrace, "expected `}` to close rig contract");
        let end = close
            .map(|token| token.span.end)
            .unwrap_or(self.peek().span.end);
        RigContractDecl {
            name,
            roles,
            span: Span::new(rig.span.start, end),
        }
    }

    fn parse_role_decl(&mut self) -> RoleDecl {
        let role = self.expect(TokenKind::Role, "expected `role` in rig contract");
        let start = role
            .as_ref()
            .map(|token| token.span.start)
            .unwrap_or(self.peek().span.start);
        let name = self.expect_identifier("expected role name");
        self.expect(TokenKind::Colon, "expected `:` after role name");
        self.expect(TokenKind::Group, "expected `Group` role type");
        self.expect(TokenKind::Less, "expected `<` after `Group`");
        let mut capabilities = Vec::new();
        loop {
            capabilities.push(self.expect_identifier("expected capability name"));
            if self.check(TokenKind::Plus) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(TokenKind::Greater, "expected `>` after role capabilities");
        let semi = self.expect(TokenKind::Semicolon, "expected `;` after role declaration");
        let end = semi
            .map(|token| token.span.end)
            .unwrap_or(self.peek().span.start);
        RoleDecl {
            name,
            capabilities,
            span: Span::new(start, end),
        }
    }

    fn parse_scene_decl(&mut self) -> SceneDecl {
        let scene_tok = self.advance(); // `scene`
        let name = self.expect_identifier("expected scene name");
        let body = self.parse_block();
        let span = Span::new(scene_tok.span.start, body.span.end);
        SceneDecl { name, body, span }
    }

    fn parse_block(&mut self) -> Block {
        let open = self.expect(TokenKind::LBrace, "expected `{` to start block");
        let start = open
            .as_ref()
            .map(|t| t.span.start)
            .unwrap_or(self.peek().span.start);

        let mut statements = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_eof() {
            let start_pos = self.pos;
            statements.push(self.parse_statement());
            if self.pos == start_pos {
                self.advance();
            }
        }

        let close = self.expect(TokenKind::RBrace, "expected `}` to close block");
        let end = close.map(|t| t.span.end).unwrap_or(self.peek().span.end);
        Block {
            statements,
            span: Span::new(start, end),
        }
    }

    fn parse_statement(&mut self) -> Statement {
        match self.peek_kind() {
            TokenKind::Let => Statement::Let(self.parse_let_statement()),
            TokenKind::Wait => Statement::Wait(self.parse_wait_statement()),
            // `Ident . Ident (` is a qualified call (`Math.sin(...)`) used
            // as a bare expression statement, not an attribute assignment
            // or transition — those never have `(` right after the
            // attribute name (they're followed by `=` or `->`).
            TokenKind::Ident(_)
                if *self.peek_nth_kind(1) == TokenKind::Dot
                    && *self.peek_nth_kind(3) != TokenKind::LParen =>
            {
                self.parse_attribute_statement()
            }
            _ => Statement::Expression(self.parse_expression_statement()),
        }
    }

    fn parse_attribute_statement(&mut self) -> Statement {
        let target = self.expect_identifier("expected target name");
        let start = target.span.start;
        self.expect(TokenKind::Dot, "expected `.` after target name");
        let attribute = self.expect_identifier("expected attribute name");

        if self.check(TokenKind::Arrow) {
            self.advance();
            let value = self.parse_expression();
            self.expect(TokenKind::Over, "expected `over` after transition value");
            let duration = self.parse_expression();
            let semi = self.expect(TokenKind::Semicolon, "expected `;` after transition");
            let end = semi.map(|t| t.span.end).unwrap_or(duration.span().end);
            Statement::Transition(TransitionStatement {
                target,
                attribute,
                value,
                duration,
                span: Span::new(start, end),
            })
        } else if self.check(TokenKind::LeftArrow) {
            self.advance();
            let signal = self.parse_expression();
            let semi = self.expect(TokenKind::Semicolon, "expected `;` after signal binding");
            let end = semi.map(|t| t.span.end).unwrap_or(signal.span().end);
            Statement::BindSignal(BindSignalStatement {
                target,
                attribute,
                signal,
                span: Span::new(start, end),
            })
        } else {
            self.expect(
                TokenKind::Eq,
                "expected `=`, `->` or `<-` after attribute name",
            );
            let value = self.parse_expression();
            let semi = self.expect(
                TokenKind::Semicolon,
                "expected `;` after attribute assignment",
            );
            let end = semi.map(|t| t.span.end).unwrap_or(value.span().end);
            Statement::Assign(AssignStatement {
                target,
                attribute,
                value,
                span: Span::new(start, end),
            })
        }
    }

    fn parse_let_statement(&mut self) -> LetStatement {
        let let_tok = self.advance(); // `let`
        let is_mut = if self.check(TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let name = self.expect_identifier("expected variable name");
        let type_annotation = if self.check(TokenKind::Colon) {
            self.advance();
            Some(self.expect_type_name())
        } else {
            None
        };
        self.expect(TokenKind::Eq, "expected `=` in `let` statement");
        let value = self.parse_expression();
        let semi = self.expect(TokenKind::Semicolon, "expected `;` after `let` statement");
        let end = semi.map(|t| t.span.end).unwrap_or(value.span().end);
        LetStatement {
            is_mut,
            name,
            type_annotation,
            value,
            span: Span::new(let_tok.span.start, end),
        }
    }

    fn parse_wait_statement(&mut self) -> WaitStatement {
        let wait_tok = self.advance(); // `wait`
        let value = self.parse_expression();
        let semi = self.expect(TokenKind::Semicolon, "expected `;` after `wait` statement");
        let end = semi.map(|t| t.span.end).unwrap_or(value.span().end);
        WaitStatement {
            value,
            span: Span::new(wait_tok.span.start, end),
        }
    }

    fn parse_expression_statement(&mut self) -> ExpressionStatement {
        let expr = self.parse_expression();
        let semi = self.expect(TokenKind::Semicolon, "expected `;` after expression");
        let end = semi.map(|t| t.span.end).unwrap_or(expr.span().end);
        let span = Span::new(expr.span().start, end);
        ExpressionStatement { expr, span }
    }

    fn parse_expression(&mut self) -> Expression {
        self.parse_binary_expression(0)
    }

    fn binding_power(op: BinaryOp) -> u8 {
        match op {
            BinaryOp::Add | BinaryOp::Sub => 1,
            BinaryOp::Mul | BinaryOp::Div => 2,
        }
    }

    fn parse_binary_expression(&mut self, min_bp: u8) -> Expression {
        let mut lhs = self.parse_unary_expression();

        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => break,
            };
            let bp = Self::binding_power(op);
            if bp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.parse_binary_expression(bp + 1);
            let span = lhs.span().to(rhs.span());
            lhs = Expression::Binary(BinaryExpr {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            });
        }

        lhs
    }

    fn parse_unary_expression(&mut self) -> Expression {
        if self.check(TokenKind::Minus) {
            let minus = self.advance();
            let operand = self.parse_unary_expression();
            let span = minus.span.to(operand.span());
            Expression::Unary(UnaryExpr {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
                span,
            })
        } else {
            self.parse_postfix_expression()
        }
    }

    /// Parses a primary expression, then any number of chained
    /// `.method(args)` and `[index]` suffixes — e.g.
    /// `Effects.sine(2s).phase(90deg).range(5%, 100%)` or `palette[0]`.
    /// The single-level `Ident . Ident (` case (`Effects.sine(...)`,
    /// `wave.range(...)`) is already fully consumed by
    /// `parse_primary_expression`'s own qualified-call handling — this
    /// loop only ever fires for a *further* `.method(...)`/`[...]` chained
    /// after whatever that returned, which can't be a bare identifier
    /// anymore (it's already a `Call`/`MethodCall`/`Index`/... at that
    /// point), so there is no ambiguity between the two.
    fn parse_postfix_expression(&mut self) -> Expression {
        let mut expr = self.parse_primary_expression();
        loop {
            if self.check(TokenKind::Dot)
                && matches!(self.peek_nth_kind(1), TokenKind::Ident(_))
                && *self.peek_nth_kind(2) == TokenKind::LParen
            {
                self.advance(); // `.`
                let method = self.expect_identifier("expected method name after `.`");
                expr = self.parse_method_call(expr, method);
            } else if self.check(TokenKind::LBracket) {
                expr = self.parse_index(expr);
            } else {
                break;
            }
        }
        expr
    }

    /// `<receiver> [ <index> ]`, e.g. `palette[0]`.
    fn parse_index(&mut self, receiver: Expression) -> Expression {
        self.advance(); // `[`
        let index = self.parse_expression();
        let close = self.expect(TokenKind::RBracket, "expected `]` after index expression");
        let end = close.map(|t| t.span.end).unwrap_or(index.span().end);
        let span = Span::new(receiver.span().start, end);
        Expression::Index(IndexExpr {
            receiver: Box::new(receiver),
            index: Box::new(index),
            span,
        })
    }

    fn parse_method_call(&mut self, receiver: Expression, method: Identifier) -> Expression {
        let lparen = self.advance(); // `(`
        let mut args = Vec::new();
        if !self.check(TokenKind::RParen) {
            loop {
                args.push(self.parse_expression());
                if self.check(TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let close = self.expect(TokenKind::RParen, "expected `)` after method arguments");
        let end = close
            .map(|t| t.span.end)
            .unwrap_or_else(|| args.last().map(|a| a.span().end).unwrap_or(lparen.span.end));
        let span = Span::new(receiver.span().start, end);
        Expression::MethodCall(MethodCallExpr {
            receiver: Box::new(receiver),
            method,
            args,
            span,
        })
    }

    fn parse_primary_expression(&mut self) -> Expression {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Expression::Literal(Literal::Int(v), tok.span)
            }
            TokenKind::Float(v) => {
                self.advance();
                Expression::Literal(Literal::Float(v), tok.span)
            }
            TokenKind::UnitValue(v, unit) => {
                self.advance();
                let lit = match unit {
                    Unit::Milliseconds => Literal::Duration(v),
                    Unit::Seconds => Literal::Duration(v * 1000),
                    Unit::Percent => Literal::Intensity(v),
                    Unit::Degrees => Literal::Angle(v),
                    Unit::Hertz => Literal::Frequency(v),
                    Unit::Bpm => Literal::Tempo(v),
                };
                Expression::Literal(lit, tok.span)
            }
            TokenKind::HexColor(r, g, b) => {
                self.advance();
                Expression::Literal(Literal::Color(ColorLiteral::Hex(r, g, b)), tok.span)
            }
            TokenKind::True => {
                self.advance();
                Expression::Literal(Literal::Bool(true), tok.span)
            }
            TokenKind::False => {
                self.advance();
                Expression::Literal(Literal::Bool(false), tok.span)
            }
            TokenKind::Red => {
                self.advance();
                Expression::Literal(
                    Literal::Color(ColorLiteral::Named(ColorName::Red)),
                    tok.span,
                )
            }
            TokenKind::Blue => {
                self.advance();
                Expression::Literal(
                    Literal::Color(ColorLiteral::Named(ColorName::Blue)),
                    tok.span,
                )
            }
            TokenKind::Green => {
                self.advance();
                Expression::Literal(
                    Literal::Color(ColorLiteral::Named(ColorName::Green)),
                    tok.span,
                )
            }
            TokenKind::White => {
                self.advance();
                Expression::Literal(
                    Literal::Color(ColorLiteral::Named(ColorName::White)),
                    tok.span,
                )
            }
            TokenKind::Black => {
                self.advance();
                Expression::Literal(
                    Literal::Color(ColorLiteral::Named(ColorName::Black)),
                    tok.span,
                )
            }
            TokenKind::Ident(name) => {
                self.advance();
                let id = Identifier {
                    name,
                    span: tok.span,
                };
                if self.check(TokenKind::LParen) {
                    let callee = CallPath {
                        qualifier: None,
                        name: id,
                        span: tok.span,
                    };
                    self.parse_call(callee)
                } else if self.check(TokenKind::Dot) && *self.peek_nth_kind(2) == TokenKind::LParen
                {
                    // Not reachable today: `Ident . Ident (` never lands
                    // here from `parse_statement` (routed to the
                    // attribute-statement guard above), but a qualified
                    // call can still appear nested inside another
                    // expression, e.g. `1 + Math.sin(90deg)`.
                    self.advance(); // `.`
                    let name = self.expect_identifier("expected function name after `.`");
                    let span = Span::new(id.span.start, name.span.end);
                    let callee = CallPath {
                        qualifier: Some(id),
                        name,
                        span,
                    };
                    self.parse_call(callee)
                } else {
                    Expression::Identifier(id)
                }
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expression();
                let close = self.expect(
                    TokenKind::RParen,
                    "expected `)` to close grouped expression",
                );
                let end = close.map(|t| t.span.end).unwrap_or(inner.span().end);
                Expression::Grouped(Box::new(inner), Span::new(tok.span.start, end))
            }
            _ => {
                self.error(
                    format!("expected expression, found {}", tok.kind.describe()),
                    tok.span,
                );
                Expression::Literal(Literal::Int(0), Span::at(tok.span.start))
            }
        }
    }

    fn parse_call(&mut self, callee: CallPath) -> Expression {
        let lparen = self.advance(); // `(`
        let mut args = Vec::new();
        if !self.check(TokenKind::RParen) {
            loop {
                args.push(self.parse_expression());
                if self.check(TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let close = self.expect(TokenKind::RParen, "expected `)` after call arguments");
        let end = close
            .map(|t| t.span.end)
            .unwrap_or_else(|| args.last().map(|a| a.span().end).unwrap_or(lparen.span.end));
        let span = Span::new(callee.span.start, end);
        Expression::Call(CallExpr { callee, args, span })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_scene() {
        let src = "scene main {\n    wait 1s;\n}\n";
        let file = parse(src).expect("should parse");
        assert_eq!(file.items.len(), 1);
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        assert_eq!(scene.name.name, "main");
        assert_eq!(scene.body.statements.len(), 1);
        assert!(matches!(scene.body.statements[0], Statement::Wait(_)));
    }

    #[test]
    fn parses_typed_let_and_wait_reference() {
        let src = r#"
            scene main {
                let duration: Duration = 500ms;
                wait duration;
            }
        "#;
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        assert_eq!(scene.body.statements.len(), 2);
        match &scene.body.statements[0] {
            Statement::Let(let_stmt) => {
                assert!(!let_stmt.is_mut);
                assert_eq!(let_stmt.name.name, "duration");
                assert_eq!(let_stmt.type_annotation.as_ref().unwrap().name, "Duration");
                assert_eq!(
                    let_stmt.value,
                    Expression::Literal(Literal::Duration(500), let_stmt.value.span())
                );
            }
            other => panic!("expected let statement, got {other:?}"),
        }
        match &scene.body.statements[1] {
            Statement::Wait(wait_stmt) => {
                assert!(
                    matches!(&wait_stmt.value, Expression::Identifier(id) if id.name == "duration")
                );
            }
            other => panic!("expected wait statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_generic_type_annotation() {
        let src = r#"
            scene main {
                let s: Signal<Intensity> = x;
            }
        "#;
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        match &scene.body.statements[0] {
            Statement::Let(let_stmt) => {
                let annotation = let_stmt.type_annotation.as_ref().unwrap();
                assert_eq!(annotation.name, "Signal");
                assert_eq!(annotation.type_args.len(), 1);
                assert_eq!(annotation.type_args[0].name, "Intensity");
                assert!(annotation.type_args[0].type_args.is_empty());
            }
            other => panic!("expected let statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_generic_type_annotation() {
        // The parser accepts arbitrary nesting syntactically; rejecting
        // `Signal<Signal<...>>` semantically is `lux-typeck`'s job.
        let src = r#"
            scene main {
                let s: Signal<Signal<Intensity>> = x;
            }
        "#;
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        match &scene.body.statements[0] {
            Statement::Let(let_stmt) => {
                let annotation = let_stmt.type_annotation.as_ref().unwrap();
                assert_eq!(annotation.name, "Signal");
                assert_eq!(annotation.type_args[0].name, "Signal");
                assert_eq!(annotation.type_args[0].type_args[0].name, "Intensity");
            }
            other => panic!("expected let statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_mut_and_literals() {
        let src = r#"
            scene main {
                let mut intensity: Intensity = 50%;
                let color = #ff0088;
            }
        "#;
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        match &scene.body.statements[0] {
            Statement::Let(let_stmt) => assert!(let_stmt.is_mut),
            other => panic!("expected let statement, got {other:?}"),
        }
        match &scene.body.statements[1] {
            Statement::Let(let_stmt) => assert_eq!(
                let_stmt.value,
                Expression::Literal(
                    Literal::Color(ColorLiteral::Hex(0xff, 0x00, 0x88)),
                    let_stmt.value.span()
                )
            ),
            other => panic!("expected let statement, got {other:?}"),
        }
    }

    #[test]
    fn respects_arithmetic_precedence() {
        let src = "scene main { let x = 1 + 2 * 3; }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        match &let_stmt.value {
            Expression::Binary(bin) => {
                assert_eq!(bin.op, BinaryOp::Add);
                assert!(matches!(*bin.lhs, Expression::Literal(Literal::Int(1), _)));
                match &*bin.rhs {
                    Expression::Binary(inner) => assert_eq!(inner.op, BinaryOp::Mul),
                    other => panic!("expected nested multiplication, got {other:?}"),
                }
            }
            other => panic!("expected binary expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_parenthesized_expression() {
        let src = "scene main { let x = (1 + 2) * 3; }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::Binary(bin) = &let_stmt.value else {
            panic!("expected binary expression");
        };
        assert_eq!(bin.op, BinaryOp::Mul);
        assert!(matches!(*bin.lhs, Expression::Grouped(_, _)));
    }

    #[test]
    fn parses_function_calls() {
        let src = "scene main { blackout(); foo(1, 2); }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        assert_eq!(scene.body.statements.len(), 2);
        match &scene.body.statements[1] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expr {
                Expression::Call(call) => {
                    assert!(call.callee.qualifier.is_none());
                    assert_eq!(call.callee.name.name, "foo");
                    assert_eq!(call.args.len(), 2);
                }
                other => panic!("expected call, got {other:?}"),
            },
            other => panic!("expected expression statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_import_decl() {
        let file = parse("import std.Math; scene main {}").expect("should parse");
        let Item::Import(import) = &file.items[0] else {
            panic!("expected import")
        };
        assert_eq!(
            import
                .path
                .iter()
                .map(|id| id.name.as_str())
                .collect::<Vec<_>>(),
            vec!["std", "Math"]
        );
    }

    #[test]
    fn parses_qualified_call() {
        let src = "import std.Math; scene main { let x = Math.sin(90deg); }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[1] else {
            panic!("expected scene")
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::Call(call) = &let_stmt.value else {
            panic!("expected call expression, got {:?}", let_stmt.value);
        };
        assert_eq!(call.callee.qualifier.as_ref().unwrap().name, "Math");
        assert_eq!(call.callee.name.name, "sin");
        assert_eq!(call.args.len(), 1);
    }

    /// `wave.range(...)` — a bare identifier receiver still parses as a
    /// `CallExpr` (identical shape to `Math.sin(...)`), since the parser
    /// can't tell "a local variable" from "a module qualifier" apart —
    /// that distinction is `lux-hir`'s job.
    #[test]
    fn single_level_method_call_on_identifier_parses_as_call_expr() {
        let src = "scene main { let x = wave.range(0%, 100%); }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::Call(call) = &let_stmt.value else {
            panic!("expected call expression, got {:?}", let_stmt.value);
        };
        assert_eq!(call.callee.qualifier.as_ref().unwrap().name, "wave");
        assert_eq!(call.callee.name.name, "range");
        assert_eq!(call.args.len(), 2);
    }

    #[test]
    fn chained_method_calls_produce_nested_method_call_expressions() {
        let src = "scene main { let x = Effects.sine(2s).phase(90deg).range(5%, 100%); }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::MethodCall(range_call) = &let_stmt.value else {
            panic!("expected outer method call, got {:?}", let_stmt.value);
        };
        assert_eq!(range_call.method.name, "range");
        assert_eq!(range_call.args.len(), 2);

        let Expression::MethodCall(phase_call) = range_call.receiver.as_ref() else {
            panic!("expected nested method call, got {:?}", range_call.receiver);
        };
        assert_eq!(phase_call.method.name, "phase");
        assert_eq!(phase_call.args.len(), 1);

        let Expression::Call(sine_call) = phase_call.receiver.as_ref() else {
            panic!(
                "expected innermost qualified call, got {:?}",
                phase_call.receiver
            );
        };
        assert_eq!(sine_call.callee.qualifier.as_ref().unwrap().name, "Effects");
        assert_eq!(sine_call.callee.name.name, "sine");
    }

    #[test]
    fn method_call_with_no_arguments_parses() {
        let src = "scene main { let x = wave.invert(); }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::Call(call) = &let_stmt.value else {
            panic!("expected call expression, got {:?}", let_stmt.value);
        };
        assert_eq!(call.callee.name.name, "invert");
        assert!(call.args.is_empty());
    }

    #[test]
    fn malformed_method_calls_report_syntax_errors_without_panicking() {
        for source in [
            "scene main { let x = wave.phase(90deg).range(5%; }",
            "scene main { let x = wave.phase(90deg).range(; }",
            "scene main { let x = wave.phase(90deg).; }",
        ] {
            assert!(
                parse(source).is_err(),
                "source unexpectedly parsed: {source}"
            );
        }
    }

    #[test]
    fn wildcard_import_is_a_syntax_error() {
        assert!(parse("import std.Math.*;").is_err());
    }

    #[test]
    fn parses_attribute_assignment() {
        let src = "scene main { Washes.intensity = 50%; }";
        let file = parse(src).expect("should parse");
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        assert_eq!(scene.body.statements.len(), 1);
        match &scene.body.statements[0] {
            Statement::Assign(assign) => {
                assert_eq!(assign.target.name, "Washes");
                assert_eq!(assign.attribute.name, "intensity");
                assert_eq!(
                    assign.value,
                    Expression::Literal(Literal::Intensity(50), assign.value.span())
                );
            }
            other => panic!("expected assign statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_attribute_transition_as_a_dedicated_statement() {
        let file = parse("scene main { Washes.intensity -> 100% over 2s; }").unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::Transition(transition) = &scene.body.statements[0] else {
            panic!("expected transition statement");
        };
        assert_eq!(transition.target.name, "Washes");
        assert_eq!(transition.attribute.name, "intensity");
        assert!(matches!(
            transition.value,
            Expression::Literal(Literal::Intensity(100), _)
        ));
        assert!(matches!(
            transition.duration,
            Expression::Literal(Literal::Duration(2000), _)
        ));
    }

    #[test]
    fn parses_attribute_signal_binding_as_a_dedicated_statement() {
        let file = parse("scene main { Washes.intensity <- level; }").unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::BindSignal(bind) = &scene.body.statements[0] else {
            panic!(
                "expected bind-signal statement, got {:?}",
                scene.body.statements[0]
            );
        };
        assert_eq!(bind.target.name, "Washes");
        assert_eq!(bind.attribute.name, "intensity");
        assert!(matches!(&bind.signal, Expression::Identifier(id) if id.name == "level"));
    }

    #[test]
    fn parses_attribute_signal_binding_with_inline_call() {
        let file = parse("scene main { Washes.intensity <- Signal.constant(50%); }").unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene")
        };
        let Statement::BindSignal(bind) = &scene.body.statements[0] else {
            panic!("expected bind-signal statement");
        };
        assert!(matches!(&bind.signal, Expression::Call(_)));
    }

    /// `=`, `->` and `<-` must never be confused by the lexer: in
    /// particular `<-` (lexed as one token) must not be mistaken for
    /// `<` followed by unary `-`, and `->`/`<-` must produce distinct
    /// statement kinds even though they share the `-` byte.
    #[test]
    fn assign_transition_and_bind_signal_are_lexically_distinct() {
        let assign = parse("scene main { Washes.intensity = 50%; }").unwrap();
        let transition = parse("scene main { Washes.intensity -> 50% over 1s; }").unwrap();
        let bind = parse("scene main { Washes.intensity <- level; }").unwrap();

        let Item::Scene(assign_scene) = &assign.items[0] else {
            unreachable!()
        };
        let Item::Scene(transition_scene) = &transition.items[0] else {
            unreachable!()
        };
        let Item::Scene(bind_scene) = &bind.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            assign_scene.body.statements[0],
            Statement::Assign(_)
        ));
        assert!(matches!(
            transition_scene.body.statements[0],
            Statement::Transition(_)
        ));
        assert!(matches!(
            bind_scene.body.statements[0],
            Statement::BindSignal(_)
        ));
    }

    #[test]
    fn malformed_signal_bindings_report_syntax_errors_without_panicking() {
        for source in [
            "scene main { Washes.intensity <- ; }",
            "scene main { Washes.intensity <- level }",
        ] {
            assert!(
                parse(source).is_err(),
                "source unexpectedly parsed: {source}"
            );
        }
    }

    /// LSP recovery: an incomplete `<-` statement, as it exists mid-typing,
    /// must still produce a `BindSignal` node (not fall back to `Assign`
    /// or drop the statement entirely) so the LSP can still offer
    /// `ExpectedType`/completions for it — mirrors the existing transition
    /// recovery behavior.
    #[test]
    fn incomplete_signal_binding_still_recovers_to_a_bind_signal_node() {
        for source in [
            "scene main { Washes.intensity <- ",
            "scene main { Washes.intensity <- sig",
        ] {
            let (file, errors) = parse_recovering(source);
            assert!(!errors.is_empty(), "expected recovery errors for {source}");
            let Item::Scene(scene) = &file.items[0] else {
                panic!("expected scene")
            };
            assert!(
                matches!(scene.body.statements.last(), Some(Statement::BindSignal(_))),
                "expected a BindSignal statement to be recovered for {source}, got {:?}",
                scene.body.statements
            );
        }
    }

    #[test]
    fn parses_rig_contract_with_typed_group_role() {
        let file =
            parse("rig contract DemoRig { role Washes: Group<Color + Intensity>; } scene main {}")
                .unwrap();
        let Item::RigContract(contract) = &file.items[0] else {
            panic!("expected rig contract");
        };
        assert_eq!(contract.name.name, "DemoRig");
        assert_eq!(contract.roles.len(), 1);
        assert_eq!(contract.roles[0].name.name, "Washes");
        assert_eq!(
            contract.roles[0]
                .capabilities
                .iter()
                .map(|capability| capability.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Color", "Intensity"]
        );
    }

    #[test]
    fn malformed_transitions_report_syntax_errors_without_panicking() {
        for source in [
            "scene main { Washes.intensity -> 100%; }",
            "scene main { Washes.intensity -> over 2s; }",
            "scene main { Washes.intensity -> 100% 2s; }",
            "scene main { Washes.intensity -> 100% over; }",
        ] {
            assert!(
                parse(source).is_err(),
                "source unexpectedly parsed: {source}"
            );
        }
    }

    #[test]
    fn missing_scene_name_is_an_error() {
        let result = parse("scene { }");
        assert!(result.is_err());
    }

    #[test]
    fn wait_without_expression_is_an_error() {
        let result = parse("scene main { wait; }");
        assert!(result.is_err());
    }

    #[test]
    fn let_without_value_is_an_error() {
        let result = parse("scene main { let x = ; }");
        assert!(result.is_err());
    }

    #[test]
    fn missing_semicolon_is_an_error() {
        let result = parse("scene main { wait 1s }");
        assert!(result.is_err());
    }

    #[test]
    fn parser_never_panics_on_garbage() {
        let inputs = [
            "",
            "scene",
            "scene main",
            "scene main {",
            "}",
            "let let let",
            "scene main { let = = = ; }",
            "scene main { wait 1 + ; }",
            "@#$%",
            "scene main { foo(1, ; }",
            "scene main { Washes. = 1; }",
            "scene main { Washes.intensity = ; }",
            "scene main { . = 1; }",
            "scene main { Washes.intensity <- ",
            "scene main { Washes.intensity <- sig",
            "scene main { Washes.intensity < -sig; }",
            "scene main { let x = wave.; }",
            "scene main { let x = wave.range(; }",
            "scene main { let x = wave.range(0%,; }",
        ];
        for input in inputs {
            let _ = parse(input); // must not panic
        }
    }

    #[test]
    fn reports_multiple_errors_when_possible() {
        let src = "scene main { wait; let x = ; }";
        let errors = parse(src).expect_err("should have errors");
        assert!(
            errors.len() >= 2,
            "expected at least 2 errors, got {errors:?}"
        );
    }

    #[test]
    fn parses_index_expression() {
        let file = parse("scene main { let x = palette[0]; }").unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene");
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::Index(index) = &let_stmt.value else {
            panic!("expected index expression, got {:?}", let_stmt.value);
        };
        assert!(matches!(*index.receiver, Expression::Identifier(_)));
        assert!(matches!(
            *index.index,
            Expression::Literal(Literal::Int(0), _)
        ));
    }

    #[test]
    fn parses_chained_index_after_method_call() {
        let file = parse("scene main { let x = Sequence.of(1, 2).length(); }").unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene");
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        assert!(matches!(let_stmt.value, Expression::MethodCall(_)));
    }

    #[test]
    fn parses_index_on_call_result() {
        let file = parse("scene main { let x = Sequence.of(1, 2)[0]; }").unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene");
        };
        let Statement::Let(let_stmt) = &scene.body.statements[0] else {
            panic!("expected let statement");
        };
        let Expression::Index(index) = &let_stmt.value else {
            panic!("expected index expression, got {:?}", let_stmt.value);
        };
        assert!(matches!(*index.receiver, Expression::Call(_)));
    }

    #[test]
    fn parses_sequence_generic_type_annotation() {
        let file = parse(
            r#"
                scene main {
                    let s: Sequence<Color> = x;
                }
                "#,
        )
        .unwrap();
        let Item::Scene(scene) = &file.items[0] else {
            panic!("expected scene");
        };
        match &scene.body.statements[0] {
            Statement::Let(let_stmt) => {
                let annotation = let_stmt.type_annotation.as_ref().unwrap();
                assert_eq!(annotation.name, "Sequence");
                assert_eq!(annotation.type_args.len(), 1);
                assert_eq!(annotation.type_args[0].name, "Color");
            }
            other => panic!("expected let statement, got {other:?}"),
        }
    }

    #[test]
    fn missing_closing_bracket_is_an_error() {
        let result = parse("scene main { let x = palette[0; }");
        assert!(result.is_err());
    }
}
