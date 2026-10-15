//! Complete offline document admission; not publication or renderer qualification.

use super::{
    DocumentDto, StructuralSummary, StructurallyCheckedDocument, WireUsage,
    allocation::{AllocationUsage, Heap},
    visual_budget::VisualReservation,
};

// Test-only instrumentation of actual admitted values, not consumer wrappers or
// estimated credits. No production fields, allocation, or quota are added.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DocumentLifetimes {
    pub(crate) live: usize,
    pub(crate) peak: usize,
    pub(crate) admission_calls: usize,
    pub(crate) constructed: usize,
    pub(crate) dropped: usize,
}
#[cfg(test)]
thread_local! {
    static LIFETIMES: std::cell::Cell<DocumentLifetimes> = const { std::cell::Cell::new(DocumentLifetimes {
        live: 0, peak: 0, admission_calls: 0, constructed: 0, dropped: 0,
    }) };
}
#[cfg(test)]
pub(crate) fn document_lifetimes(reset: bool) -> DocumentLifetimes {
    LIFETIMES.with(|counts| {
        let current = counts.get();
        if reset {
            assert_eq!(current.live, 0, "previous parsed document remains alive");
            counts.set(DocumentLifetimes::default());
        }
        current
    })
}
#[cfg(test)]
impl Drop for AdmittedV2Document {
    fn drop(&mut self) {
        LIFETIMES.with(|counts| {
            let mut current = counts.get();
            current.live -= 1;
            current.dropped += 1;
            counts.set(current);
        });
    }
}

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
    #[cfg(test)]
    LIFETIMES.with(|counts| {
        let mut current = counts.get();
        current.live += 1;
        current.peak = current.peak.max(current.live);
        current.constructed += 1;
        counts.set(current);
    });
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
    #[cfg(test)]
    LIFETIMES.with(|counts| {
        let mut current = counts.get();
        current.admission_calls += 1;
        counts.set(current);
    });
    super::parse(bytes)?
        .check_local()?
        .check_structure()?
        .check_admission()
}
