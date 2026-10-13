//! Pre-axis viewport boundary. Library interactions cannot bypass this policy.
use super::{egui, plot_projection::PlotView};
use egui_plot::{PlotBounds, PlotPoint};

/// Leaves headroom for the pinned remap/grid arithmetic, including a 4096-pixel
/// frame and its reciprocal. This is a display arithmetic limit, not a schema
/// limit or a replacement for extreme measurements.
const MAX: f64 = f64::MAX / 4096.0;
const MIN_SPAN: f64 = f64::MIN_POSITIVE * 4096.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GeometryUnavailable {
    Bounds,
    AxisBounds,
    Transform,
    Segment,
    Allocation,
    Stroke,
}

pub(super) fn valid(bounds: PlotBounds) -> bool {
    (0..2).all(|axis| {
        let lo = bounds.min()[axis];
        let hi = bounds.max()[axis];
        let span = hi - lo;
        lo.is_finite()
            && hi.is_finite()
            && lo.abs() <= MAX
            && hi.abs() <= MAX
            && span.is_finite()
            && (MIN_SPAN..=MAX).contains(&span)
    })
}

fn expand(mut lo: f64, mut hi: f64, margin: bool) -> Option<(f64, f64)> {
    if !lo.is_finite() || !hi.is_finite() || lo.abs() > MAX || hi.abs() > MAX {
        return None;
    }
    // A constant trace still needs a viewport, not fabricated data points.
    if lo == hi {
        let radius = lo.abs().max(1.0) * 0.05;
        lo -= radius;
        hi += radius;
    } else if margin {
        let pad = (hi - lo) * 0.05;
        lo -= pad;
        hi += pad;
    }
    valid(PlotBounds::from_min_max([lo; 2], [hi; 2])).then_some((lo, hi))
}

pub(super) fn trace_bounds(points: &[PlotPoint]) -> Option<PlotBounds> {
    let first = points.first()?;
    let mut lo = [first.x, first.y];
    let mut hi = lo;
    for p in points {
        for (a, value) in [p.x, p.y].into_iter().enumerate() {
            if !value.is_finite() {
                return None;
            }
            lo[a] = lo[a].min(value);
            hi[a] = hi[a].max(value);
        }
    }
    for a in 0..2 {
        (lo[a], hi[a]) = expand(lo[a], hi[a], true)?;
    }
    Some(PlotBounds::from_min_max(lo, hi))
}

/// Fixed slots; one unsuitable trace never contaminates the other contributions.
pub(super) struct Preflight {
    pub(super) faults: [Option<GeometryUnavailable>; 32],
    pub(super) bounds: PlotBounds,
}
impl Preflight {
    pub(super) fn new(view: &PlotView<'_>, hidden: u32) -> Result<Self, GeometryUnavailable> {
        let mut result = Self {
            faults: [None; 32],
            bounds: PlotBounds::NOTHING,
        };
        for (i, trace) in view.traces.iter().enumerate() {
            let points = view.points(i);
            if !trace.definition.visible || hidden & (1 << i) != 0 || points.is_empty() {
                continue;
            }
            let Some(bounds) = trace_bounds(points) else {
                result.faults[i] = Some(GeometryUnavailable::Bounds);
                continue;
            };
            let mut merged = result.bounds;
            merged.merge(&bounds);
            if valid(merged) {
                result.bounds = merged;
            } else {
                result.faults[i] = Some(GeometryUnavailable::Bounds);
            }
        }
        if !valid(result.bounds) {
            // An empty, explicitly diagnosed plot has ordinary empty axes.
            result.bounds = PlotBounds::new_symmetrical(1.0);
        }
        let mut lo = result.bounds.min();
        let mut hi = result.bounds.max();
        let axes = &view.definition.axes;
        // Preserve include_y semantics; reject an unsafe explicit request instead
        // of silently showing a different axis range.
        for endpoint in [axes.y_min, axes.y_max].into_iter().flatten() {
            lo[1] = lo[1].min(endpoint.0);
            hi[1] = hi[1].max(endpoint.0);
        }
        let candidate = PlotBounds::from_min_max(lo, hi);
        if !valid(candidate) {
            return Err(GeometryUnavailable::AxisBounds);
        }
        result.bounds = candidate;
        Ok(result)
    }
}

/// Checked interaction candidate, using this frame's response rectangle. This
/// does not prepare drawing geometry or rely on last-frame PlotUi::transform.
pub(super) fn interact(
    bounds: PlotBounds,
    response: &egui::Response,
    ctx: &egui::Context,
) -> Result<Option<PlotBounds>, GeometryUnavailable> {
    let rect = response.rect;
    if !rect.is_finite() || rect.width() < 1.0 || rect.height() < 1.0 {
        return Err(GeometryUnavailable::Transform);
    }
    let (zoom, scroll, pointer) = ctx.input(|input| {
        (
            input.zoom_delta_2d(),
            input.smooth_scroll_delta,
            input.pointer.hover_pos(),
        )
    });
    let mut lo = bounds.min();
    let mut hi = bounds.max();
    let mut changed = false;
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = response.drag_delta();
        for (a, pixels) in [-delta.x, delta.y].into_iter().enumerate() {
            let size = if a == 0 { rect.width() } else { rect.height() };
            let shift = f64::from(pixels) / f64::from(size) * (hi[a] - lo[a]);
            lo[a] += shift;
            hi[a] += shift;
        }
        changed = true;
    }
    if response.contains_pointer()
        && let Some(pointer) = pointer
    {
        for a in 0..2 {
            let (size, fraction, z, delta) = if a == 0 {
                (
                    rect.width(),
                    (pointer.x - rect.left()) / rect.width(),
                    zoom.x,
                    -scroll.x,
                )
            } else {
                (
                    rect.height(),
                    (rect.bottom() - pointer.y) / rect.height(),
                    zoom.y,
                    scroll.y,
                )
            };
            if z != 1.0 {
                if !z.is_finite() || z <= 0.0 {
                    return Err(GeometryUnavailable::Bounds);
                }
                let center = lo[a] + f64::from(fraction) * (hi[a] - lo[a]);
                lo[a] = center + (lo[a] - center) / f64::from(z);
                hi[a] = center + (hi[a] - center) / f64::from(z);
                changed = true;
            }
            if delta != 0.0 {
                let shift = f64::from(delta) / f64::from(size) * (hi[a] - lo[a]);
                lo[a] += shift;
                hi[a] += shift;
                changed = true;
            }
        }
    }
    let candidate = PlotBounds::from_min_max(lo, hi);
    if !valid(candidate) {
        return Err(GeometryUnavailable::Bounds);
    }
    Ok(changed.then_some(candidate))
}
