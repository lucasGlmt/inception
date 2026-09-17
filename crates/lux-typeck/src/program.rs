//! The output of a successful [`crate::checker::check`]: every local's
//! resolved type, ready for `lux-mir` to consume without re-running
//! inference.

use lux_hir::{HandlerId, SceneId};

use crate::types::Type;

#[derive(Debug, Clone, PartialEq)]
pub struct TypedProgram {
    /// Indexed by `SceneId`, in the same order as the `HirFile` this was
    /// produced from.
    pub scenes: Vec<TypedScene>,
    /// Indexed by `HandlerId`, in the same order as `HirFile::handlers`.
    /// Reuses `TypedScene` rather than a distinct type: an event handler's
    /// body is structurally identical to a scene's (locals + statements),
    /// so a second type would be pure duplication.
    pub handlers: Vec<TypedScene>,
}

impl TypedProgram {
    pub fn scene(&self, id: SceneId) -> &TypedScene {
        &self.scenes[id.0 as usize]
    }

    pub fn handler(&self, id: HandlerId) -> &TypedScene {
        &self.handlers[id.0 as usize]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedScene {
    /// Indexed by `LocalId`, in the same order as the source
    /// `HirScene::locals`.
    pub local_types: Vec<Type>,
}
