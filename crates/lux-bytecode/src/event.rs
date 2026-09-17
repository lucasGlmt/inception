//! Compiled event patterns and bindings — the portable representation an
//! `on <pattern> { ... }` block compiles to. A pattern is matched against
//! an `inception_core::InputEvent` by `inception-runtime`'s `EventRouter`
//! at runtime; nothing here is re-derived from HIR/AST at that point (see
//! `AGENTS.md`'s "single source of truth" and this crate's own "must not
//! depend on the compiler frontend" rule — `EventPattern` is an
//! intentional, boundary-preserving duplicate of `lux_hir::HirEventPattern`,
//! the same relationship `TargetId`/`Attribute` already have with their
//! `lux-hir` counterparts).

use crate::ids::FunctionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAction {
    Press,
    Release,
}

/// V1 supports exactly one device/control shape: a Launchpad X pad,
/// addressed by its `1..=8` grid coordinates. Closed by design — not a
/// general event-expression tree (see `AGENTS.md`'s "small coherent
/// language" principle and RFC 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPattern {
    LaunchpadPad { x: u8, y: u8, action: EventAction },
}

/// A compiled `on { ... }` block: the pattern it fires on, plus the
/// `FunctionId` of its (never directly called, never `entry`) handler
/// function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventBinding {
    pub pattern: EventPattern,
    pub handler: FunctionId,
}
