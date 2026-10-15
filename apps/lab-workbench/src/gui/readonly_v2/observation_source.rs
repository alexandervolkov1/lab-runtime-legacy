//! Borrowed selected-read consumer contract. Local lookup only: no transport,
//! discovery reconstruction, cache mutation, or laboratory submission methods.
use super::PresentationEpoch;
use crate::{
    model::{Freshness, LivePoint, ReadAbsence, ReadQuality, ScalarKind},
    presentation::RuntimeRef,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Scalar<'a> {
    Absent(ReadAbsence),
    Boolean(bool),
    Integer(i64),
    Number(f64),
    Text(&'a str),
    Enum(&'a str),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unit<'a> {
    pub(crate) id: &'a str,
    pub(crate) symbol: &'a str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Origin<'a> {
    pub(crate) boot: Option<&'a str>,
    pub(crate) projection: Option<&'a str>,
    pub(crate) revision: Option<u64>,
    pub(crate) entity_revision: Option<u64>,
    pub(crate) generation: Option<u64>,
    pub(crate) observed_at: Option<u64>,
    pub(crate) source_at: Option<u64>,
    pub(crate) cursor: Option<u64>,
}
/// Little-endian finite pairs, borrowed from a fully validated LRD1 slot. No
/// aligned casts, decoded history, or extra point buffer. Construction stays at
/// the trusted codec boundary, which proves count/finite/source-floor checks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trace<'a> {
    pub(crate) encoded: &'a [u8],
    pub(crate) source_floor: Option<f64>,
}
impl Trace<'_> {
    pub(crate) fn len(&self) -> usize {
        self.encoded.len() / 16
    }
    pub(crate) fn points(&self) -> impl ExactSizeIterator<Item = LivePoint> + '_ {
        self.encoded.chunks_exact(16).map(|pair| LivePoint {
            time_seconds: f64::from_le_bytes(pair[..8].try_into().unwrap()),
            value: f64::from_le_bytes(pair[8..].try_into().unwrap()),
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fields<'a> {
    pub(crate) scalar: Scalar<'a>,
    pub(crate) kind: Option<ScalarKind>,
    pub(crate) unit: Option<Unit<'a>>,
    pub(crate) quality: Option<ReadQuality>,
    pub(crate) freshness: Freshness,
    pub(crate) origin: Origin<'a>,
    pub(crate) trace: Trace<'a>,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct SelectedRead<'a> {
    pub(crate) target: &'a RuntimeRef,
    pub(crate) fields: Fields<'a>,
    /// Independent receiver latch; never replaces original quality/freshness.
    pub(crate) current: bool,
}
impl SelectedRead<'_> {
    pub(crate) fn display_freshness(&self) -> Freshness {
        if !self.current && self.fields.freshness == Freshness::Fresh {
            Freshness::Stale
        } else {
            self.fields.freshness
        }
    }
}
pub(crate) trait ReadOnlyProvider {
    fn epoch(&self) -> PresentationEpoch;
    fn incarnation(&self) -> u64;
    fn original_freshness(&self) -> Freshness;
    fn current(&self) -> bool;
    fn selected(&self, target: &RuntimeRef) -> Option<SelectedRead<'_>>;
}
