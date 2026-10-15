//! Borrowed selected-read consumer contract. Local lookup only: no transport,
//! discovery reconstruction, cache mutation, or laboratory submission methods.
use super::PresentationEpoch;
use crate::{
    presentation::RuntimeRef,
    readonly_values::{Freshness, LivePoint, ReadAbsence, ReadBinding, ReadQuality, ScalarKind},
};
use std::collections::{VecDeque, vec_deque};

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
    points: TracePoints<'a>,
    pub(crate) source_floor: Option<f64>,
}
#[derive(Clone, Copy, Debug)]
enum TracePoints<'a> {
    Encoded(&'a [u8]),
    Cache(&'a VecDeque<LivePoint>, u64),
}
impl Trace<'_> {
    #[cfg(test)]
    pub(crate) fn encoded_bytes(&self) -> Option<&[u8]> {
        match self.points {
            TracePoints::Encoded(bytes) => Some(bytes),
            TracePoints::Cache(_, _) => None,
        }
    }
    pub(crate) fn encoded(bytes: &[u8], source_floor: Option<f64>) -> Trace<'_> {
        Trace {
            points: TracePoints::Encoded(bytes),
            source_floor,
        }
    }
    pub(crate) fn cache(points: &VecDeque<LivePoint>, dropped: u64) -> Trace<'_> {
        Trace {
            points: TracePoints::Cache(points, dropped),
            source_floor: None,
        }
    }
    pub(crate) fn len(&self) -> usize {
        match self.points {
            TracePoints::Encoded(bytes) => bytes.len() / 16,
            TracePoints::Cache(points, _) => points.len(),
        }
    }
    pub(crate) fn dropped(&self) -> Option<u64> {
        match self.points {
            TracePoints::Encoded(_) => None,
            TracePoints::Cache(_, dropped) => Some(dropped),
        }
    }
    pub(crate) fn last(&self) -> Option<LivePoint> {
        match self.points {
            TracePoints::Encoded(bytes) => {
                bytes.chunks_exact(16).next_back().map(|pair| LivePoint {
                    time_seconds: f64::from_le_bytes(pair[..8].try_into().unwrap()),
                    value: f64::from_le_bytes(pair[8..].try_into().unwrap()),
                })
            }
            TracePoints::Cache(points, _) => points.back().copied(),
        }
    }
    pub(crate) fn points(&self) -> Points<'_> {
        match self.points {
            TracePoints::Encoded(bytes) => Points::Encoded(bytes.chunks_exact(16)),
            TracePoints::Cache(points, _) => Points::Cache(points.iter()),
        }
    }
}
pub(crate) enum Points<'a> {
    Encoded(std::slice::ChunksExact<'a, u8>),
    Cache(vec_deque::Iter<'a, LivePoint>),
}
impl Iterator for Points<'_> {
    type Item = LivePoint;
    fn next(&mut self) -> Option<LivePoint> {
        match self {
            Self::Encoded(pairs) => pairs.next().map(|pair| LivePoint {
                time_seconds: f64::from_le_bytes(pair[..8].try_into().unwrap()),
                value: f64::from_le_bytes(pair[8..].try_into().unwrap()),
            }),
            Self::Cache(points) => points.next().copied(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = match self {
            Self::Encoded(pairs) => pairs.len(),
            Self::Cache(points) => points.len(),
        };
        (len, Some(len))
    }
}
impl ExactSizeIterator for Points<'_> {}
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
/// Optional parent-cache entity view; the value and its own freshness travel
/// together. A global Fresh cache cannot promote an individually Stale entity.
pub(crate) struct RawEntity<'a> {
    pub(crate) value: &'a serde_json::Value,
    pub(crate) freshness: Freshness,
}
pub(crate) trait ReadOnlyProvider {
    /// Parent-cache adapters have no child epoch/incarnation. Absence is explicit.
    fn epoch(&self) -> Option<PresentationEpoch>;
    fn incarnation(&self) -> Option<u64>;
    fn original_freshness(&self) -> Freshness;
    fn current(&self) -> bool;
    /// The visitor cannot retain a reference to a frame-local decoded scalar.
    /// Neither implementation builds a second model or point buffer.
    fn with_selected(
        &self,
        target: &RuntimeRef,
        binding: ReadBinding,
        consume: &mut dyn FnMut(Option<SelectedRead<'_>>),
    );
    fn trace(&self, target: &RuntimeRef) -> Option<Trace<'_>>;
    /// Optional bounded-display source; LRD1 deliberately does not contain it.
    fn supports_raw_entities(&self) -> bool {
        false
    }
    fn raw_entity(&self, _target: &RuntimeRef) -> Option<RawEntity<'_>> {
        None
    }
    fn display_freshness(&self) -> Freshness {
        if !self.current() && self.original_freshness() == Freshness::Fresh {
            Freshness::Stale
        } else {
            self.original_freshness()
        }
    }
}
