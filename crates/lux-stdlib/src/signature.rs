//! The stdlib function signature model. A `Signature` is everything the
//! compiler, verifier and LSP need to know about one callable stdlib
//! function — never just an opaque runtime callback.

use crate::intrinsic::IntrinsicId;
use crate::types::ParamType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    pub name: &'static str,
    pub ty: ParamType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    /// e.g. `&["std", "Math"]`.
    pub module_path: &'static [&'static str],
    /// e.g. `"sin"` — what call sites write after the module qualifier.
    pub name: &'static str,
    pub params: &'static [Param],
    pub return_ty: ParamType,
    pub intrinsic: IntrinsicId,
    /// Always `true` in V1: no `std.Math`/`std.Color` function may read the
    /// clock, randomness, IO, DMX state or any mutable global — same input
    /// always produces the same output. Kept explicit (rather than assumed)
    /// so a future, deliberately-impure module doesn't have to retrofit
    /// this field.
    pub pure: bool,
    /// Short human-readable documentation, surfaced verbatim by the LSP's
    /// hover/signature-help and by `docs/stdlib/*.md` generation.
    pub doc: &'static str,
}

impl Signature {
    pub fn arity(&self) -> usize {
        self.params.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdModule {
    /// e.g. `&["std", "Math"]`.
    pub path: &'static [&'static str],
    /// What call sites use after `import`, e.g. `"Math"`.
    pub short_name: &'static str,
    pub functions: &'static [Signature],
    pub doc: &'static str,
}
