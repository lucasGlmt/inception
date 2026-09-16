//! The output of a successful [`crate::checker::check`]: every local's
//! resolved type, ready for `lux-mir` to consume without re-running
//! inference.

use lux_hir::SceneId;

use crate::types::Type;

#[derive(Debug, Clone, PartialEq)]
pub struct TypedProgram {
    /// Indexed by `SceneId`, in the same order as the `HirFile` this was
    /// produced from.
    pub scenes: Vec<TypedScene>,
}

impl TypedProgram {
    pub fn scene(&self, id: SceneId) -> &TypedScene {
        &self.scenes[id.0 as usize]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedScene {
    /// Indexed by `LocalId`, in the same order as the source
    /// `HirScene::locals`.
    pub local_types: Vec<Type>,
}
