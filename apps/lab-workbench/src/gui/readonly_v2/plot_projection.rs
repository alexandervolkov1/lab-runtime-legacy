//! One borrowed Plot occurrence, one frame-local point arena, no GUI or cache.

use super::observation_source::{ReadOnlyProvider, Scalar};
use super::{Location, OccurrenceIdentity, PresentationEpoch, read};
use crate::{
    presentation::{RuntimeRef, v2},
    readonly_values::{
        Freshness, LIVE_TRACE_POINTS, LivePoint, ReadAbsence, ReadBinding, ReadQuality, ScalarKind,
    },
};
use egui_plot::PlotPoint;
use std::{
    fmt::{self, Write},
    ops::Range,
    time::Duration,
};

pub(crate) const MAX_POINTS: usize = 16_384;
pub(crate) const MAX_TRACES: usize = 32;
/// Part of (never in addition to) the eventual 8 MiB visual reservation.
pub(crate) const PROJECTION_RESERVATION: usize = 512 * 1024;
/// Two passes over at most 32 existing 4096-point buffers, including repeats.
pub(crate) const MAX_POINT_VISITS: usize = 2 * MAX_TRACES * LIVE_TRACE_POINTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectionError {
    InvalidOccurrence,
    Budget,
    Allocation,
}

/// Fixed UTF-8 output; overflow is an error, never a silently truncated label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
}
impl<const N: usize> Text<N> {
    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("only UTF-8 writes")
    }
}
impl<const N: usize> Write for Text<N> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self
            .len
            .checked_add(value.len())
            .filter(|&n| n <= N)
            .ok_or(fmt::Error)?;
        self.bytes[self.len..end].copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TraceIdentity<'a> {
    pub(crate) occurrence: OccurrenceIdentity<'a>,
    pub(crate) plot_id: &'a v2::Id,
    pub(crate) trace_id: &'a v2::Id,
    pub(crate) index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CurveState {
    Ready,
    Disabled(v2::StaticBlocker),
    Absent(ReadAbsence),
    ExpectKindMismatch,
    UnsupportedKind,
    OversizedValue,
    Unconfirmed,
    MissingBuffer,
    EmptyWindow,
}

#[derive(Debug)]
pub(crate) struct TraceView<'a> {
    pub(crate) identity: TraceIdentity<'a>,
    /// Original definitions preserve the full target, label, visibility and style.
    pub(crate) definition: &'a v2::Trace,
    pub(crate) binding: &'a v2::DataBinding,
    pub(crate) availability: v2::StaticAvailability,
    pub(crate) state: CurveState,
    pub(crate) kind: Option<ScalarKind>,
    pub(crate) freshness: Freshness,
    pub(crate) quality: Option<ReadQuality>,
    pub(crate) runtime_unit_id: Text<32>,
    pub(crate) runtime_unit_symbol: Text<16>,
    /// Ordinal prefix is injective even for adversarial duplicate labels.
    pub(crate) legend: Text<520>,
    pub(crate) window: Option<(f64, f64)>,
    pub(crate) source_points: usize,
    pub(crate) window_points: usize,
    pub(crate) buffer_dropped: Option<u64>,
    pub(crate) decimated_points: usize,
    points: Range<usize>,
}

#[derive(Debug)]
pub(crate) struct PlotView<'a> {
    pub(crate) identity: OccurrenceIdentity<'a>,
    /// Includes the exact title, axes and time window; no schema copy.
    pub(crate) definition: &'a v2::Plot,
    pub(crate) traces: Vec<TraceView<'a>>,
    points: Vec<PlotPoint>,
    pub(crate) point_visits: usize,
}
impl PlotView<'_> {
    pub(crate) fn points(&self, index: usize) -> &[PlotPoint] {
        self.traces
            .get(index)
            .map_or(&[], |trace| &self.points[trace.points.clone()])
    }
    pub(crate) fn prepared_points(&self) -> usize {
        self.points.len()
    }
}

fn reserve<T>(count: usize) -> Result<Vec<T>, ProjectionError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| ProjectionError::Allocation)?;
    Ok(result)
}
fn signal_target(source: &v2::RuntimeRef) -> Result<Option<RuntimeRef>, ProjectionError> {
    let v2::RuntimeRef::Signal {
        instrument,
        parameter,
    } = source
    else {
        return Ok(None);
    };
    let copy = |value: &str| {
        let mut text = String::new();
        text.try_reserve_exact(value.len())
            .map_err(|_| ProjectionError::Allocation)?;
        text.push_str(value);
        Ok(text)
    };
    Ok(Some(RuntimeRef::Signal {
        instrument: copy(instrument.as_str())?,
        parameter: copy(parameter.as_str())?,
    }))
}

/// Select one cell of a validated navigation page. No expansion, Context, or
/// persistent state. The caller must release this view before projecting again.
pub(crate) fn project<'a>(
    document: &'a v2::AdmittedV2Document,
    observations: &dyn ReadOnlyProvider,
    epoch: PresentationEpoch,
    location: Location,
    index: usize,
    allowance: usize,
) -> Result<PlotView<'a>, ProjectionError> {
    // Guard BEFORE any allocation. Checked arithmetic also fences future growth
    // of TraceView; 64 KiB covers inline scratch, typed reads and allocation slack.
    let bytes = MAX_POINTS
        .checked_mul(std::mem::size_of::<PlotPoint>())
        .and_then(|n| n.checked_add(MAX_TRACES.checked_mul(std::mem::size_of::<TraceView<'_>>())?))
        .and_then(|n| n.checked_add(64 * 1024))
        .ok_or(ProjectionError::Budget)?;
    if allowance < PROJECTION_RESERVATION || bytes > PROJECTION_RESERVATION {
        return Err(ProjectionError::Budget);
    }
    if !location.valid(document) || index / super::PAGE_SIZE != location.page {
        return Err(ProjectionError::InvalidOccurrence);
    }
    let (root, path) = location.occurrence(index);
    let occurrence = document
        .source()
        .occurrence(
            location.window,
            location.tab,
            location.panel,
            root,
            &path[..location.depth],
        )
        .ok_or(ProjectionError::InvalidOccurrence)?;
    let v2::Component::Plot { plot_id, .. } = occurrence.component else {
        return Err(ProjectionError::InvalidOccurrence);
    };
    let plot = document
        .dto()
        .plots
        .0
        .iter()
        .find(|plot| &plot.id == plot_id)
        .ok_or(ProjectionError::InvalidOccurrence)?;
    let identity = OccurrenceIdentity {
        epoch,
        occurrence: occurrence.identity,
    };
    let mut result = PlotView {
        identity,
        definition: plot,
        traces: reserve(plot.traces.0.len())?,
        points: Vec::new(),
        point_visits: 0,
    };
    let mut buffers = [None; MAX_TRACES];
    let mut reserved_visits: usize = 0;
    if observations
        .epoch()
        .is_some_and(|snapshot| snapshot != epoch)
    {
        return Err(ProjectionError::InvalidOccurrence);
    }
    for (index, definition) in plot.traces.0.iter().enumerate() {
        let (data_index, binding) = document
            .dto()
            .data_bindings
            .0
            .iter()
            .enumerate()
            .find(|(_, data)| data.id == definition.data_id)
            .expect("admitted trace binding");
        let availability = match occurrence.availability {
            v2::StaticAvailability::Disabled(reason) => v2::StaticAvailability::Disabled(reason),
            _ => document.summary().data[data_index],
        };
        let mut trace = TraceView {
            identity: TraceIdentity {
                occurrence: result.identity.clone(),
                plot_id: &plot.id,
                trace_id: &definition.id,
                index,
            },
            definition,
            binding,
            availability,
            state: CurveState::Ready,
            kind: None,
            freshness: observations.display_freshness(),
            quality: None,
            runtime_unit_id: Text::default(),
            runtime_unit_symbol: Text::default(),
            legend: Text::default(),
            window: None,
            source_points: 0,
            window_points: 0,
            buffer_dropped: None,
            decimated_points: 0,
            points: 0..0,
        };
        write!(
            trace.legend,
            "{}: {}",
            index + 1,
            definition.display_label.as_str()
        )
        .map_err(|_| ProjectionError::Budget)?;
        if let v2::StaticAvailability::Disabled(reason) = availability {
            trace.state = CurveState::Disabled(reason);
        } else if binding.selector != v2::Selector::SignalSample {
            trace.state = CurveState::Disabled(v2::StaticBlocker::IncompatibleTarget);
        } else if let Some(target) = signal_target(&binding.target)? {
            observations.with_selected(&target, ReadBinding::SignalSample, &mut |selected| {
                let Some(selected) = selected else {
                    trace.state = CurveState::Absent(ReadAbsence::MissingObservation);
                    return;
                };
                let fields = selected.fields;
                trace.freshness = match observations.display_freshness() {
                    Freshness::Unknown | Freshness::Rebuilding => observations.display_freshness(),
                    Freshness::Stale if selected.display_freshness() == Freshness::Fresh => {
                        Freshness::Stale
                    }
                    _ => selected.display_freshness(),
                };
                trace.kind = fields.kind;
                trace.quality = fields.quality;
                trace.state = match fields.scalar {
                    Scalar::Absent(reason) => CurveState::Absent(reason),
                    Scalar::Text(s) | Scalar::Enum(s) if s.len() > 512 => {
                        CurveState::OversizedValue
                    }
                    _ if binding
                        .expect_kind
                        .is_some_and(|expect| Some(read::kind(expect)) != trace.kind) =>
                    {
                        CurveState::ExpectKindMismatch
                    }
                    Scalar::Number(_) => CurveState::Ready,
                    _ => CurveState::UnsupportedKind,
                };
                // Codec and parent typed decoder already validate unit byte bounds.
                if let Some(unit) = fields.unit
                    && (trace.runtime_unit_id.write_str(unit.id).is_err()
                        || trace.runtime_unit_symbol.write_str(unit.symbol).is_err())
                {
                    trace.state = CurveState::OversizedValue;
                }
                if trace.state == CurveState::Ready
                    && !matches!(trace.freshness, Freshness::Fresh | Freshness::Stale)
                {
                    trace.state = CurveState::Unconfirmed;
                }
            });
            if trace.state == CurveState::Ready && definition.visible {
                if let Some(buffer) = observations.trace(&target) {
                    trace.source_points = buffer.len();
                    trace.buffer_dropped = buffer.dropped();
                    // Reserve BOTH scans, including repeated selections, before
                    // visiting a source. LRD1 can carry more than a parent ring.
                    reserved_visits = buffer
                        .len()
                        .checked_mul(2)
                        .and_then(|n| reserved_visits.checked_add(n))
                        .filter(|n| *n <= MAX_POINT_VISITS)
                        .ok_or(ProjectionError::Budget)?;
                    if let Some(last) = buffer.last() {
                        let cutoff =
                            (last.time_seconds - plot.time_window_seconds.0).max(-f64::MAX);
                        trace.window = Some((cutoff, last.time_seconds));
                        trace.window_points = buffer
                            .points()
                            .filter(|point| eligible(point, cutoff))
                            .count();
                        result.point_visits += buffer.len();
                        if trace.window_points != 0 {
                            buffers[index] = Some(buffer);
                        }
                    }
                    if trace.window_points == 0 {
                        trace.state = CurveState::EmptyWindow;
                    }
                } else {
                    trace.state = CurveState::MissingBuffer;
                }
            }
        } else {
            trace.state = CurveState::Disabled(v2::StaticBlocker::IncompatibleTarget);
        }
        result.traces.push(trace);
    }
    let needs = std::array::from_fn(|index| {
        result
            .traces
            .get(index)
            .map_or(0, |trace| trace.window_points)
    });
    let quotas = point_quotas(needs);
    let count = quotas.iter().sum();
    result.points = reserve(count)?;
    for ((trace, buffer), quota) in result.traces.iter_mut().zip(buffers).zip(quotas) {
        trace.points.start = result.points.len();
        if let Some(buffer) = buffer {
            let cutoff = trace.window.expect("counted source window").0;
            let points = buffer.points().filter(|point| eligible(point, cutoff));
            decimate(points, trace.window_points, quota, &mut result.points);
            result.point_visits += buffer.len();
        }
        trace.points.end = result.points.len();
        trace.decimated_points = trace.window_points - trace.points.len();
    }
    debug_assert!(result.points.len() <= MAX_POINTS && result.point_visits <= MAX_POINT_VISITS);
    Ok(result)
}

/// Integer water filling over 32 fixed slots, with no point scans or allocation.
/// Both scans have already reserved the fixed MAX_POINT_VISITS, so the sum
/// of source lengths is <= 32 * 4096 even for repeated LRD1 selections. At most
/// 15 binary-search passes over a <=16384-point need find the highest common
/// level whose capped sum fits; fewer than 32 residual slots go in trace order.
fn point_quotas(needs: [usize; MAX_TRACES]) -> [usize; MAX_TRACES] {
    if needs.iter().sum::<usize>() <= MAX_POINTS {
        return needs;
    }
    let (mut low, mut high) = (0, needs.into_iter().max().unwrap_or(0));
    while low < high {
        let level = low + (high - low).div_ceil(2);
        let used: usize = needs.iter().map(|&need| need.min(level)).sum();
        if used <= MAX_POINTS {
            low = level;
        } else {
            high = level - 1;
        }
    }
    let mut quotas = needs.map(|need| need.min(low));
    let mut remaining = MAX_POINTS - quotas.iter().sum::<usize>();
    for (quota, need) in quotas.iter_mut().zip(needs) {
        if remaining != 0 && *quota < need {
            *quota += 1;
            remaining -= 1;
        }
    }
    // The level is at least MAX_POINTS / MAX_TRACES = 512: every nonempty
    // trace survives, and every decimated trace has room for both endpoints.
    debug_assert_eq!(remaining, 0);
    quotas
}

fn eligible(point: &LivePoint, cutoff: f64) -> bool {
    point.time_seconds.is_finite() && point.value.is_finite() && point.time_seconds >= cutoff
}

/// Equal-count, contiguous interior buckets. Keep first/last and each bucket's
/// min/max in original index order (first wins ties). No averaging or new x/y.
fn decimate(
    input: impl Iterator<Item = impl std::borrow::Borrow<LivePoint>>,
    count: usize,
    quota: usize,
    output: &mut Vec<PlotPoint>,
) {
    let mut input = input.map(|point| *point.borrow());
    let push = |out: &mut Vec<PlotPoint>, point: LivePoint| {
        out.push(PlotPoint::new(point.time_seconds, point.value))
    };
    if count <= quota {
        for point in input {
            push(output, point);
        }
        return;
    }
    push(output, input.next().expect("nonempty decimation"));
    let buckets = (quota - 2) / 2;
    let interior = count - 2;
    for bucket in 0..buckets {
        let start = bucket * interior / buckets;
        let end = (bucket + 1) * interior / buckets;
        let first = input.next().expect("bucket is nonempty");
        let mut min = (start, first);
        let mut max = (start, first);
        for index in start + 1..end {
            let point = input.next().expect("counted source");
            if point.value < min.1.value {
                min = (index, point);
            }
            if point.value > max.1.value {
                max = (index, point);
            }
        }
        if min.0 <= max.0 {
            push(output, min.1);
            if min.0 != max.0 {
                push(output, max.1);
            }
        } else {
            push(output, max.1);
            push(output, min.1);
        }
    }
    push(output, input.next().expect("last point"));
}

pub(crate) fn elapsed_time_label(seconds: f64) -> Option<Text<48>> {
    let duration = Duration::try_from_secs_f64(seconds).ok()?;
    let milliseconds = (duration.as_nanos() + 500_000) / 1_000_000;
    let mut text = Text::default();
    write!(
        text,
        "T+{:02}:{:02}:{:02}.{:03}",
        milliseconds / 3_600_000,
        (milliseconds / 60_000) % 60,
        (milliseconds / 1_000) % 60,
        milliseconds % 1_000
    )
    .ok()?;
    Some(text)
}

/// Format a curve position, not evidence that this position was sampled. B2 may
/// supply an interpolated segment position; it must not label that a measurement.
pub(crate) fn hover(trace: &TraceView<'_>, position: PlotPoint) -> Option<Text<4096>> {
    if !trace.definition.visible || trace.points.is_empty() || !position.y.is_finite() {
        return None;
    }
    let time = elapsed_time_label(position.x)?;
    let mut text = Text::default();
    write!(
        text,
        "{}\nCurve position: {}, {}",
        trace.legend.as_str(),
        time.as_str(),
        position.y
    )
    .ok()?;
    let unit = if !trace.runtime_unit_symbol.as_str().is_empty() {
        trace.runtime_unit_symbol.as_str()
    } else {
        trace.runtime_unit_id.as_str()
    };
    if !unit.is_empty() {
        write!(text, " {unit}").ok()?;
    } else {
        text.write_str(" (unit unknown)").ok()?;
    }
    if let Some(unit) = &trace.definition.display_unit {
        write!(text, "\nDisplay label: {} (no conversion)", unit.as_str()).ok()?;
    }
    write!(
        text,
        "\nSource cache: {:?}; current quality: {:?}; {:?}\nDisplay points omitted: {}; buffer evictions: {}",
        trace.freshness, trace.quality, trace.state, trace.decimated_points, dropped_label(trace.buffer_dropped).as_str()
    )
    .ok()?;
    Some(text)
}

/// LRD1 carries no dropped-count; unavailable is never represented by zero.
pub(super) fn dropped_label(count: Option<u64>) -> Text<32> {
    let mut text = Text::default();
    match count {
        Some(count) => write!(text, "{count}"),
        None => text.write_str("unavailable"),
    }
    .expect("bounded dropped label");
    text
}
