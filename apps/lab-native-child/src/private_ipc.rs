//! Child half of the original private IPC contract; no bridge or publication.
#[path = "../../lab-workbench/src/private_ipc/document_owner.rs"]
mod document_owner;
#[path = "../../lab-workbench/src/private_ipc/protocol.rs"]
pub(crate) mod protocol;
#[path = "../../lab-workbench/src/private_ipc/read_decode.rs"]
pub(crate) mod reads;
#[path = "../../lab-workbench/src/private_ipc/renderer_progress.rs"]
pub(crate) mod renderer_progress;
#[path = "../../lab-workbench/src/private_ipc/replica.rs"]
mod replica;
#[path = "../../lab-workbench/src/private_ipc/selected_provider.rs"]
mod selected_provider;
#[path = "../../lab-workbench/src/private_ipc/transport.rs"]
pub(crate) mod transport;
pub(crate) use document_owner::DocumentOwner;
pub(crate) use replica::Replica;
