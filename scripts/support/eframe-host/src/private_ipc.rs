//! Original bounded private protocol and admission, without Custom Glow pairing.
#![allow(dead_code, reason = "shared bounded protocol surface")]
#[path = "../../../../apps/lab-workbench/src/private_ipc/protocol.rs"]
pub(crate) mod protocol;
#[path = "../../../../apps/lab-workbench/src/private_ipc/read_decode.rs"]
pub(crate) mod reads;
#[path = "../../../../apps/lab-workbench/src/private_ipc/replica.rs"]
mod replica;
#[path = "../../../../apps/lab-workbench/src/private_ipc/selected_provider.rs"]
pub(crate) mod selected_provider;
#[path = "../../../../apps/lab-workbench/src/private_ipc/transport.rs"]
pub(crate) mod transport;
#[path = "../../../../apps/lab-workbench/src/private_ipc/ui_document.rs"]
mod ui_document;
pub(crate) use replica::Replica;
pub(crate) use selected_provider::SelectedSnapshot;
pub(crate) use ui_document::UiDocument;
