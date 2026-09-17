//! Abstract syntax tree.
//!
//! Every node that matters for diagnostics or lowering carries a [`Span`].
//! Unit-suffixed literals are normalized to a single canonical internal
//! unit at parse time (milliseconds for durations, degrees for angles,
//! hertz for frequency, BPM for tempo, whole percent for intensity) so
//! later stages never need to re-derive them from raw source text. Range
//! checks (e.g. intensity `0..=100`) are deliberately *not* enforced here;
//! that's `lux-typeck`'s job, per the workspace's layering rules.
//!
//! This AST intentionally only contains what the current language surface
//! needs. Future constructs (`Target.intensity = 50%;`, transitions,
//! `run scene;`) are not pre-declared here: `Statement` and `Expression`
//! are plain enums, so adding a new variant later is a local, additive
//! change rather than something that needs to be designed in now.

use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Scene(SceneDecl),
    RigContract(RigContractDecl),
    Import(ImportDecl),
    EventHandler(EventHandlerDecl),
}

/// `import std.Math;` — a module import. `path` holds every dotted
/// segment (`[std, Math]`); no wildcards (`std.Math.*`) and no aliasing
/// exist in this milestone, so a bare dotted path is the entire surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub path: Vec<Identifier>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigContractDecl {
    pub name: Identifier,
    pub roles: Vec<RoleDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoleDecl {
    pub name: Identifier,
    pub capabilities: Vec<Identifier>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneDecl {
    pub name: Identifier,
    pub body: Block,
    pub span: Span,
}

/// `on <device>.<control>(<x>, <y>).<action> { ... }` — V1's only event
/// pattern shape (see `AGENTS.md`/RFC 0007): `on launchpad.pad(1, 1).press`.
/// Deliberately not a generic event-expression grammar — `device`,
/// `control` and `action` are kept as raw identifiers, like
/// `AssignStatement::attribute`; validating they're literally
/// `launchpad`/`pad`/`press`|`release` is `lux-hir`'s job, not the
/// parser's.
#[derive(Debug, Clone, PartialEq)]
pub struct EventHandlerDecl {
    pub device: Identifier,
    pub control: Identifier,
    pub x: EventCoordinate,
    pub y: EventCoordinate,
    pub action: Identifier,
    pub body: Block,
    pub span: Span,
}

/// A pad coordinate as written. `value` may be out of `1..=8` — checked by
/// `lux-hir`, the same "range checks aren't enforced in the AST" split
/// this module's own docs describe for e.g. `Literal::Intensity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventCoordinate {
    pub value: i64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let(LetStatement),
    Wait(WaitStatement),
    Expression(ExpressionStatement),
    Assign(AssignStatement),
    Transition(TransitionStatement),
    BindSignal(BindSignalStatement),
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let(s) => s.span,
            Statement::Wait(s) => s.span,
            Statement::Expression(s) => s.span,
            Statement::Assign(s) => s.span,
            Statement::Transition(s) => s.span,
            Statement::BindSignal(s) => s.span,
        }
    }
}

/// `<target>.<attribute> = <value>;` — a lighting attribute assignment,
/// e.g. `Washes.intensity = 50%;`.
///
/// Both `target` and `attribute` are kept as raw names, not resolved
/// here: role names are resolved from the rig contract during HIR lowering,
/// while attribute validity is `lux-typeck`'s responsibility, exactly like
/// `LetStatement::type_annotation`.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignStatement {
    pub target: Identifier,
    pub attribute: Identifier,
    pub value: Expression,
    pub span: Span,
}

/// `<target>.<attribute> -> <value> over <duration>;`.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionStatement {
    pub target: Identifier,
    pub attribute: Identifier,
    pub value: Expression,
    pub duration: Expression,
    pub span: Span,
}

/// `<target>.<attribute> <- <signal>;` — a continuous signal binding, e.g.
/// `Front.intensity <- level;`.
///
/// Distinct from [`AssignStatement`] (immediate value) and
/// [`TransitionStatement`] (finite interpolation): this statement makes the
/// attribute continuously derive its value from a `Signal<T>` until the
/// binding is replaced or detached by a later `=`, `->` or `<-` on the same
/// `(target, attribute)`.
#[derive(Debug, Clone, PartialEq)]
pub struct BindSignalStatement {
    pub target: Identifier,
    pub attribute: Identifier,
    pub signal: Expression,
    pub span: Span,
}

/// A type name written in source (e.g. `Duration` in `let x: Duration = ...`,
/// or `Signal` with `type_args: [Intensity]` for `Signal<Intensity>`).
///
/// Kept as raw text + span rather than a resolved type: `lux-syntax` has no
/// notion of which type names are valid or which ones accept type
/// arguments, that's decided during type checking so the set of builtin
/// types (and which are generic) lives in exactly one place. `type_args` is
/// empty for every non-generic type; the parser accepts `Name<Arg>` for any
/// `Name`, and even allows `Arg` to itself carry type arguments — nothing
/// here enforces that only `Signal` is generic or that nesting is
/// disallowed, that's `lux-typeck`'s call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName {
    pub name: String,
    pub span: Span,
    pub type_args: Vec<TypeName>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStatement {
    pub is_mut: bool,
    pub name: Identifier,
    pub type_annotation: Option<TypeName>,
    pub value: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaitStatement {
    pub value: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionStatement {
    pub expr: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(Literal, Span),
    Identifier(Identifier),
    Unary(UnaryExpr),
    Binary(BinaryExpr),
    Call(CallExpr),
    MethodCall(MethodCallExpr),
    Index(IndexExpr),
    Grouped(Box<Expression>, Span),
}

impl Expression {
    pub fn span(&self) -> Span {
        match self {
            Expression::Literal(_, span) => *span,
            Expression::Identifier(id) => id.span,
            Expression::Unary(e) => e.span,
            Expression::Binary(e) => e.span,
            Expression::Call(e) => e.span,
            Expression::MethodCall(e) => e.span,
            Expression::Index(e) => e.span,
            Expression::Grouped(_, span) => *span,
        }
    }
}

/// `<receiver> [ <index> ]` — e.g. `palette[0]`. V1 only supports indexing
/// a `Sequence<T>` with an `Int`, checked by `lux-typeck`; this node only
/// records the syntax, same as [`CallExpr`]/[`MethodCallExpr`].
#[derive(Debug, Clone, PartialEq)]
pub struct IndexExpr {
    pub receiver: Box<Expression>,
    pub index: Box<Expression>,
    pub span: Span,
}

/// `<receiver> . <method> ( <args> )` — e.g. `wave.range(0%, 100%)`, or
/// `Effects.sine(2s).phase(90deg)` where `receiver` is itself a `Call`.
///
/// Distinct from [`CallExpr`] (a qualified stdlib call, `Module.function(...)`):
/// the parser can't tell `wave.range(...)` (`wave` a local) apart from
/// `Effects.sine(...)` (`Effects` a module) when the receiver is a bare
/// identifier — both still parse as [`CallExpr`] via [`CallPath::qualifier`]
/// (unchanged) — so this node only ever appears for a receiver that is
/// *not* a bare identifier already fully consumed that way, i.e. for
/// chained/nested method calls. `lux-hir` is what actually distinguishes
/// "qualifier is an imported module" from "qualifier is a local variable"
/// (the latter also becoming a method call at that point) — see
/// `resolve.rs`'s docs.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodCallExpr {
    pub receiver: Box<Expression>,
    pub method: Identifier,
    pub args: Vec<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorName {
    Red,
    Blue,
    Green,
    White,
    Black,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorLiteral {
    Named(ColorName),
    Hex(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Canonicalized to whole milliseconds.
    Duration(i64),
    /// Whole percent; may be out of `0..=100`, checked by `lux-typeck`.
    Intensity(i64),
    /// Canonicalized to whole degrees.
    Angle(i64),
    /// Canonicalized to whole hertz.
    Frequency(i64),
    /// Canonicalized to whole beats per minute.
    Tempo(i64),
    Color(ColorLiteral),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub operand: Box<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub lhs: Box<Expression>,
    pub rhs: Box<Expression>,
    pub span: Span,
}

/// The callee of a call expression: either a bare name (`foo(...)`, always
/// unresolved in this milestone — Lux has no user-defined functions yet) or
/// a module-qualified name (`Math.sin(...)`), resolved against the calling
/// file's imports.
#[derive(Debug, Clone, PartialEq)]
pub struct CallPath {
    pub qualifier: Option<Identifier>,
    pub name: Identifier,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallExpr {
    pub callee: CallPath,
    pub args: Vec<Expression>,
    pub span: Span,
}
