//! Isolated v2 syntax and local checks. Nothing here admits or publishes a document.
//!
//! Cross-registry, graph, expansion, canonical serialization and measured allocation
//! checks are deliberately pending. No conversion to the active v1 document exists.

mod bounded;
mod dto;
mod validation;

pub(crate) use bounded::{I64Decimal, U64Decimal};
pub(crate) use dto::*;

/// A closed-shape document, still requiring local checks and full admission.
#[derive(Debug)]
pub(crate) struct ParsedDocument(DocumentDto);

/// Local checks succeeded; references and full admission have NOT been checked.
/// The private payload cannot be modified through this wrapper.
#[derive(Debug)]
pub(crate) struct LocallyCheckedDocument(DocumentDto);

impl ParsedDocument {
    /// Checks local constraints only. This is never a publication permit.
    pub(crate) fn check_local(self) -> Result<LocallyCheckedDocument, String> {
        validation::check(&self.0)?;
        Ok(LocallyCheckedDocument(self.0))
    }
}

impl LocallyCheckedDocument {
    /// Read-only syntax for future admission passes, not an admitted document.
    pub(crate) fn dto(&self) -> &DocumentDto {
        &self.0
    }
}

/// Parses an already bounded byte slice without acquiring observations or authority.
/// Callers reading a stream must cap it before buffering; this API performs no I/O.
pub(crate) fn parse(bytes: &[u8]) -> Result<ParsedDocument, String> {
    bounded::preflight(bytes)?;
    serde_json::from_slice(bytes)
        .map(ParsedDocument)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
