//! Fixed deletion diagnostics. Completion and object release are separate facts.
use super::{frame::ContextGeneration, texture_owner::TextureKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObjectKind {
    Texture,
    Buffer,
    Shader,
    Program,
    VertexArray,
    Fence,
    Generation,
}

/// Exact backend identity; texture identities also retain their original key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ObjectIdentity {
    pub(crate) generation: ContextGeneration,
    pub(crate) kind: ObjectKind,
    pub(crate) name: u64,
    pub(crate) texture: Option<TextureKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeleteDisposition {
    /// The adapter proves that the deletion call was not made.
    NotStarted,
    /// Deletion may have started; neither a receipt nor another delete is valid.
    Ambiguous,
}

/// A failure latches admission and deletion. Polling completion never retries it.
/// A future resolution protocol must establish the exact object's state before
/// permitting another operation; this stage intentionally provides no retry API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeletionFailure {
    pub(crate) object: ObjectIdentity,
    pub(crate) disposition: DeleteDisposition,
}

/// Ok explicitly confirms release of the requested obligation. Err retains it.
pub(crate) type DeletionResult = Result<(), DeletionFailure>;
