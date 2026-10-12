//! Isolated v2 syntax, local and structural checks. No publication permission.
//!
//! Canonical serialization, envelope headroom and measured allocation checks are
//! still pending. No conversion to the active v1 document exists.

mod bounded;
mod dto;
mod graph;
mod structure;
mod validation;

pub(crate) use bounded::{I64Decimal, U64Decimal};
pub(crate) use dto::*;
pub(crate) use graph::{ViewOccurrence, ViewOccurrenceId};
pub(crate) use structure::{StaticAvailability, StaticBlocker, StructuralSummary};

/// A closed-shape document, still requiring local checks and full admission.
#[derive(Debug)]
pub(crate) struct ParsedDocument(DocumentDto);

/// Local checks succeeded; references and full admission have NOT been checked.
/// The private payload cannot be modified through this wrapper.
#[derive(Debug)]
pub(crate) struct LocallyCheckedDocument(DocumentDto);

/// References, graph, expanded counts and static semantics passed. NOT admitted.
/// Only immutable access is exposed; no public constructor or deserializer exists.
#[derive(Debug)]
pub(crate) struct StructurallyCheckedDocument {
    dto: DocumentDto,
    summary: StructuralSummary,
}

impl ParsedDocument {
    /// Checks local constraints only. This is never a publication permit.
    pub(crate) fn check_local(self) -> Result<LocallyCheckedDocument, String> {
        validation::check(&self.0)?;
        Ok(LocallyCheckedDocument(self.0))
    }
}

impl LocallyCheckedDocument {
    /// Consumes the local stage; success still grants no Runtime or publication authority.
    pub(crate) fn check_structure(self) -> Result<StructurallyCheckedDocument, String> {
        let summary = structure::check(&self.0)?;
        Ok(StructurallyCheckedDocument {
            dto: self.0,
            summary,
        })
    }
    /// Read-only syntax for future admission passes, not an admitted document.
    pub(crate) fn dto(&self) -> &DocumentDto {
        &self.0
    }
}

impl StructurallyCheckedDocument {
    /// Immutable syntax retained for the future final admission stage.
    pub(crate) fn dto(&self) -> &DocumentDto {
        &self.dto
    }

    /// Static blockers and exact expanded counts; absence of a blocker is not eligibility.
    pub(crate) fn summary(&self) -> &StructuralSummary {
        &self.summary
    }

    /// Resolves one bounded occurrence address without expanding or allocating views.
    /// Positions are document-local; a future renderer must also fence its owner epoch.
    pub(crate) fn occurrence(
        &self,
        window: usize,
        tab: usize,
        panel: usize,
        root: usize,
        children: &[usize],
    ) -> Option<ViewOccurrence<'_>> {
        graph::occurrence(&self.dto, &self.summary, window, tab, panel, root, children)
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
mod structure_tests;
#[cfg(test)]
mod tests;
