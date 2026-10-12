//! Complete offline document admission; not publication or renderer qualification.

use super::{
    DocumentDto, StructuralSummary, StructurallyCheckedDocument, WireUsage,
    allocation::{AllocationUsage, Heap},
    visual_budget::VisualReservation,
};

/// Immutable, single-owner offline result. No deserializer, unchecked constructor,
/// mutable DTO accessor, or conversion to the running presentation owner exists.
/// Visual reservation is conditional on a future capacity-enforcing consumer;
/// this type does not qualify a renderer or make any action eligible.
#[derive(Debug)]
pub(crate) struct AdmittedV2Document {
    source: StructurallyCheckedDocument,
    canonical: Vec<u8>,
    request: WireUsage,
    response: WireUsage,
    allocation: AllocationUsage,
    visual: VisualReservation,
}

impl AdmittedV2Document {
    pub(crate) fn dto(&self) -> &DocumentDto {
        self.source.dto()
    }
    pub(crate) fn summary(&self) -> &StructuralSummary {
        self.source.summary()
    }
    pub(crate) fn source(&self) -> &StructurallyCheckedDocument {
        &self.source
    }
    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }
    pub(crate) fn allocation(&self) -> AllocationUsage {
        self.allocation
    }
    pub(crate) fn visual_reservation(&self) -> VisualReservation {
        self.visual
    }
    pub(crate) fn request_usage(&self) -> WireUsage {
        self.request
    }
    pub(crate) fn response_usage(&self) -> WireUsage {
        self.response
    }
}

pub(super) fn finish(source: StructurallyCheckedDocument) -> Result<AdmittedV2Document, String> {
    let summary = source.summary();
    let drafts = super::allocation::add(source.dto().editor_bindings.0.len(), 1)?;
    let visual =
        VisualReservation::new(summary.expanded_components, summary.expanded_plots, drafts)?;
    // Reject an oversized source before allocating serialization scratch.
    let mut allocation = AllocationUsage::default();
    allocation.root::<AdmittedV2Document>()?;
    source.dto().heap(&mut allocation)?;
    summary.heap(&mut allocation)?;
    allocation.check()?;
    let mut reserved = allocation;
    reserved.block(super::limits::PRESENTATION_FILE_BYTES)?;
    reserved.check()?;
    let proof = source.check_canonical_wire()?;
    let (canonical, request, response) = proof.into_parts();
    // Charge actual retained capacity, not serialized length or requested reserve.
    allocation.block(canonical.capacity())?;
    allocation.check()?;
    Ok(AdmittedV2Document {
        source,
        canonical,
        request,
        response,
        allocation,
        visual,
    })
}

/// Runs every stage. Failure yields no partially admitted result.
pub(crate) fn admit(bytes: &[u8]) -> Result<AdmittedV2Document, String> {
    super::parse(bytes)?
        .check_local()?
        .check_structure()?
        .check_admission()
}
