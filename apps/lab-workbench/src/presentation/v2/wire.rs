//! Offline capacity probes for the proposed shapes. No endpoint or call dispatcher.
//! The network body is compact JSON: canonical pretty-document LF bytes must never
//! be inserted raw into a line-framed message. Both use the same normalized DTO.

use super::{
    bounded::{self, JsonUsage, U64Decimal},
    canonical::CappedBuffer,
    dto::DocumentDto,
    limits,
};
use serde::Serialize;

/// Full-body lexical and physical-frame usage, under worst-case envelope fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireUsage {
    pub(crate) json: JsonUsage,
    pub(crate) lf_frame_bytes: usize,
    pub(crate) crlf_frame_bytes: usize,
}

#[derive(Clone, Copy)]
pub(super) enum Exchange {
    ReplaceRequest,
    PresentationResult,
}

// These private serialization-only shapes cannot receive or dispatch a call.
#[derive(Serialize)]
struct Expected<'a> {
    workbench_id: &'a str,
    revision: U64Decimal,
}
#[derive(Serialize)]
struct ReplaceArgs<'a, T> {
    expected: Expected<'a>,
    document: &'a T,
}
#[derive(Serialize)]
struct ReplaceRequest<'a, T> {
    v: u8,
    r#type: &'static str,
    call_id: &'a str,
    op: &'static str,
    args: ReplaceArgs<'a, T>,
}
#[derive(Serialize)]
struct Presentation<'a, T> {
    workbench_id: &'a str,
    presentation_revision: U64Decimal,
    document: &'a T,
}
#[derive(Serialize)]
struct PresentationResult<'a, T> {
    v: u8,
    r#type: &'static str,
    call_id: &'a str,
    result: Presentation<'a, T>,
}

pub(super) fn check(doc: &DocumentDto) -> Result<(WireUsage, WireUsage), String> {
    let mut buffer = CappedBuffer::new(limits::JSON_BODY_BYTES)?;
    let request = probe(doc, Exchange::ReplaceRequest, &mut buffer)
        .map_err(|error| format!("request wire headroom: {error}"))?;
    let response = probe(doc, Exchange::PresentationResult, &mut buffer)
        .map_err(|error| format!("response wire headroom: {error}"))?;
    Ok((request, response)) // The single temporary body buffer is dropped here.
}

// Generic only for deterministic scanner fixtures in tests. The proof entry above
// accepts only the DTO owned by the preceding checked stage.
pub(super) fn probe<T: Serialize>(
    doc: &T,
    exchange: Exchange,
    buffer: &mut CappedBuffer,
) -> Result<WireUsage, String> {
    buffer.bytes.clear();
    // Any decoded 64-byte call ID is legal today. NUL needs six encoded bytes per
    // decoded byte, the maximum escaping expansion; IDs are not restricted to ASCII
    // alphanumerics by the current call-ID contract.
    let call_id = "\0".repeat(limits::NAME_BYTES);
    let workbench_id = "ffffffffffffffffffffffffffffffff";
    let revision = U64Decimal(u64::MAX);
    let serialized = match exchange {
        Exchange::ReplaceRequest => serde_json::to_writer(
            &mut *buffer,
            &ReplaceRequest {
                v: 2,
                r#type: "request",
                call_id: &call_id,
                op: "ui_replace_document",
                args: ReplaceArgs {
                    expected: Expected {
                        workbench_id,
                        revision,
                    },
                    document: doc,
                },
            },
        ),
        Exchange::PresentationResult => serde_json::to_writer(
            &mut *buffer,
            &PresentationResult {
                v: 2,
                r#type: "result",
                call_id: &call_id,
                result: Presentation {
                    workbench_id,
                    presentation_revision: revision,
                    document: doc,
                },
            },
        ),
    };
    serialized.map_err(|error| error.to_string())?;
    inspect_frame(&buffer.bytes)
}

pub(super) fn inspect_frame(body: &[u8]) -> Result<WireUsage, String> {
    let lf_frame_bytes = body.len().checked_add(1).ok_or("frame size overflow")?;
    let crlf_frame_bytes = body.len().checked_add(2).ok_or("frame size overflow")?;
    if lf_frame_bytes > limits::FRAME_BYTES || crlf_frame_bytes > limits::FRAME_BYTES {
        return Err("LF/CRLF physical frame limit".into());
    }
    // Compact encoding must not split a frame; escaped control bytes remain legal.
    if body.contains(&b'\n') || body.contains(&b'\r') {
        return Err("unescaped frame delimiter".into());
    }
    let json = bounded::inspect(body)?;
    Ok(WireUsage {
        json,
        lf_frame_bytes,
        crlf_frame_bytes,
    })
}
