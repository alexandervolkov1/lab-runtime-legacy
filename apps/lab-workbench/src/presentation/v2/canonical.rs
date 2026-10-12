//! Bounded derived canonical bytes and a borrowed proof, never an admitted owner.

use super::{
    StructurallyCheckedDocument, limits,
    wire::{self, WireUsage},
};
use std::io::{self, Write};

/// Derivative bytes tied to the immutable checked source. NOT a publication permit.
#[derive(Debug)]
pub(crate) struct CanonicalWireCandidate<'a> {
    source: &'a StructurallyCheckedDocument,
    bytes: Vec<u8>,
    request: WireUsage,
    response: WireUsage,
}
impl CanonicalWireCandidate<'_> {
    pub(crate) fn source(&self) -> &StructurallyCheckedDocument {
        self.source
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn request_usage(&self) -> WireUsage {
        self.request
    }
    pub(crate) fn response_usage(&self) -> WireUsage {
        self.response
    }
    pub(super) fn into_parts(self) -> (Vec<u8>, WireUsage, WireUsage) {
        (self.bytes, self.request, self.response)
    }
}

// Reserve one known maximum, never grow it and reject before extending past it.
// Final admission charges capacity and conservative allocator slack separately.
// An allocation failure here returns an error, not a candidate.
pub(super) struct CappedBuffer {
    pub(super) bytes: Vec<u8>,
    limit: usize,
}
impl CappedBuffer {
    pub(super) fn new(limit: usize) -> Result<Self, String> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(limit)
            .map_err(|_| "bounded serialization allocation failed")?;
        Ok(Self { bytes, limit })
    }
}
impl Write for CappedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|len| len > self.limit)
        {
            return Err(io::Error::other("serialization byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn check(
    source: &StructurallyCheckedDocument,
) -> Result<CanonicalWireCandidate<'_>, String> {
    let mut canonical = CappedBuffer::new(limits::PRESENTATION_FILE_BYTES)?;
    serde_json::to_writer_pretty(&mut canonical, source.dto())
        .map_err(|error| format!("canonical document: {error}"))?;
    let (request, response) = wire::check(source.dto())?;
    Ok(CanonicalWireCandidate {
        source,
        bytes: canonical.bytes,
        request,
        response,
    })
}
