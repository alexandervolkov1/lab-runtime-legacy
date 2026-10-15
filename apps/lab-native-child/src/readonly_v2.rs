//! Private aliases to the exact Workbench ownership source; no renderer fork.
#[path = "../../lab-workbench/src/gui/readonly_v2/deletion.rs"]
pub(crate) mod deletion;
#[path = "../../lab-workbench/src/gui/readonly_v2/native_types.rs"]
mod native_types;
pub(crate) use native_types::MAX_SHAPES;

pub(crate) mod frame {
    pub(crate) use super::native_types::ContextGeneration;
}
pub(crate) mod texture_owner {
    pub(crate) use super::native_types::TextureKey;
}
pub(crate) mod completion {
    pub(crate) use super::native_types::{Completion, WaitFailure, WaitRequest};
}
