//! Current-frame finite screen geometry, with no retained sample/segment cache.
use super::{
    egui,
    plot_bounds::{self, GeometryUnavailable},
    plot_projection::{MAX_POINTS, TraceView},
};
use egui_plot::{
    ClosestElem, Cursor, PlotBounds, PlotConfig, PlotGeometry, PlotItem, PlotItemBase, PlotPoint,
    PlotTransform,
};
use std::{cell::Cell, ops::RangeInclusive, sync::Arc};

/// Output payload cap across all 32 traces, inside the single Preview allowance.
/// Library output copies/capacity/allocator overhead are measured separately.
pub(super) const MAX_MESH_BYTES: usize =
    MAX_POINTS * (4 * std::mem::size_of::<egui::epaint::Vertex>() + 6 * std::mem::size_of::<u32>());
const _: () = assert!(MAX_MESH_BYTES == 1_703_936);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TraceGeometry {
    pub(super) fault: Option<GeometryUnavailable>,
    pub(super) segments: usize,
}

/// One item per original trace, borrowing its full identity/binding and B1 slice.
/// Segment indices are local display bookkeeping, never Runtime provenance.
pub(super) struct FiniteTraceItem<'a, 'doc> {
    base: PlotItemBase,
    trace: &'a TraceView<'doc>,
    points: &'a [PlotPoint],
    stroke: egui::Stroke,
    status: &'a Cell<TraceGeometry>,
}

impl<'a, 'doc> FiniteTraceItem<'a, 'doc> {
    pub(super) fn new(
        trace: &'a TraceView<'doc>,
        points: &'a [PlotPoint],
        stroke: egui::Stroke,
        status: &'a Cell<TraceGeometry>,
    ) -> Self {
        Self {
            base: PlotItemBase::new(super::plot_text::alias(trace.identity.index).to_owned()),
            trace,
            points,
            stroke,
            status,
        }
    }

    fn segment(
        &self,
        index: usize,
        transform: &PlotTransform,
    ) -> Result<Option<[egui::Pos2; 2]>, GeometryUnavailable> {
        let a = *self.points.get(index).ok_or(GeometryUnavailable::Segment)?;
        let b = self.points.get(index + 1).copied().unwrap_or(a);
        screen_segment(a, b, transform)
    }
}

impl PlotItem for FiniteTraceItem<'_, '_> {
    fn base(&self) -> &PlotItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut PlotItemBase {
        &mut self.base
    }
    fn id(&self) -> egui::Id {
        super::plot_widget::PlotState::trace_id(self.trace)
    }
    fn color(&self) -> egui::Color32 {
        self.stroke.color
    }
    fn initialize(&mut self, _: RangeInclusive<f64>) {}
    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }
    fn bounds(&self) -> PlotBounds {
        plot_bounds::trace_bounds(self.points).unwrap_or(PlotBounds::NOTHING)
    }

    fn shapes(&self, _: &egui::Ui, transform: &PlotTransform, shapes: &mut Vec<egui::Shape>) {
        if self.points.is_empty() || self.status.get().fault.is_some() {
            return;
        }
        let mut status = TraceGeometry::default();
        if self.stroke.width == 0.0 {
            status.fault = Some(GeometryUnavailable::Stroke);
            self.status.set(status);
            return;
        }
        let mut mesh = egui::Mesh::default();
        let count = self.points.len().saturating_sub(1).max(1);
        // All substantial output reservations precede publication. One Mesh per
        // trace, not an allocation or PlotItem per segment. No segment arena.
        let reserved = count
            .checked_mul(4)
            .zip(count.checked_mul(6))
            .filter(|&(vertices, indices)| {
                self.points.len() <= MAX_POINTS
                    && vertices
                        .checked_mul(std::mem::size_of::<egui::epaint::Vertex>())
                        .zip(indices.checked_mul(std::mem::size_of::<u32>()))
                        .and_then(|(v, i)| v.checked_add(i))
                        .is_some_and(|bytes| bytes <= MAX_MESH_BYTES)
            })
            .is_some_and(|(vertices, indices)| {
                mesh.vertices.try_reserve_exact(vertices).is_ok()
                    && mesh.indices.try_reserve_exact(indices).is_ok()
                    && shapes.try_reserve(1).is_ok()
            });
        if !reserved {
            status.fault = Some(GeometryUnavailable::Allocation);
            self.status.set(status);
            return;
        }
        let width = self.stroke.width * if self.highlighted() { 2.0 } else { 1.0 };
        let mut previous_singleton = None;
        for index in 0..count {
            match self.segment(index, transform) {
                Ok(Some([a, b])) => {
                    // Equal screen positions do not imply equal measurements.
                    // Collapse only repeated visual markers, retaining B1 intact.
                    if a == b && previous_singleton == Some(a) {
                        continue;
                    }
                    previous_singleton = (a == b).then_some(a);
                    if quad(&mut mesh, [a, b], width, self.stroke.color).is_some() {
                        status.segments += 1;
                    } else {
                        status.fault = Some(GeometryUnavailable::Segment);
                    }
                }
                Ok(None) => {
                    previous_singleton = None;
                }
                Err(reason) => {
                    status.fault = Some(reason);
                    previous_singleton = None;
                }
            }
        }
        self.status.set(status);
        if !mesh.indices.is_empty() {
            shapes.push(egui::Shape::Mesh(Arc::new(mesh)));
        }
    }

    fn find_closest(&self, point: egui::Pos2, transform: &PlotTransform) -> Option<ClosestElem> {
        if self.status.get().segments == 0 || !transform.frame().contains(point) {
            return None;
        }
        let mut best: Option<ClosestElem> = None;
        for i in 0..self.points.len().saturating_sub(1).max(1) {
            let Some(segment) = self.segment(i, transform).ok().flatten() else {
                continue;
            };
            let Some((_, dist_sq)) = closest(point, segment) else {
                continue;
            };
            if best.as_ref().is_none_or(|b| dist_sq < b.dist_sq) {
                best = Some(ClosestElem { index: i, dist_sq });
            }
        }
        best
    }

    fn on_hover(
        &self,
        response: &egui::Response,
        elem: ClosestElem,
        shapes: &mut Vec<egui::Shape>,
        _: &mut Vec<Cursor>,
        plot: &PlotConfig<'_>,
        formatter: Option<&egui_plot::LabelFormatterFn<'_>>,
    ) {
        let Some(pointer) = response.hover_pos() else {
            return;
        };
        let Some(segment) = self.segment(elem.index, plot.transform).ok().flatten() else {
            return;
        };
        let Some((position, _)) = closest(pointer, segment) else {
            return;
        };
        let value = plot.transform.value_from_position(position);
        if !value.x.is_finite() || !value.y.is_finite() {
            return;
        }
        let Some(text) = formatter.and_then(|f| {
            f(&egui_plot::HoverPosition::NearDataPoint {
                plot_name: self.name(),
                position: value,
                index: elem.index,
            })
        }) else {
            return;
        };
        shapes.push(egui::Shape::circle_filled(position, 3.0, self.stroke.color));
        if plot.show_crosshair {
            let frame = plot.transform.frame();
            let stroke = egui::Stroke::new(1.0, egui::Color32::GRAY);
            if plot.show_x {
                shapes.push(egui::Shape::line_segment(
                    [
                        egui::pos2(position.x, frame.top()),
                        egui::pos2(position.x, frame.bottom()),
                    ],
                    stroke,
                ));
            }
            if plot.show_y {
                shapes.push(egui::Shape::line_segment(
                    [
                        egui::pos2(frame.left(), position.y),
                        egui::pos2(frame.right(), position.y),
                    ],
                    stroke,
                ));
            }
        }
        let mut tooltip = egui::Tooltip::always_open(
            response.ctx.clone(),
            response.layer_id,
            response.id,
            egui::PopupAnchor::Pointer,
        );
        let width = response.ctx.global_style().spacing.tooltip_width;
        tooltip.popup = tooltip.popup.width(width);
        tooltip.gap(12.0).show(|ui| {
            ui.set_max_width(width);
            ui.label(text);
        });
    }
}

/// Map to f64 normalized viewport coordinates first; clip before any f32 cast.
/// Overflow or unresolved arithmetic explicitly breaks this segment, never
/// reconnecting its neighbors. No intersection is represented as a sample.
fn screen_segment(
    a: PlotPoint,
    b: PlotPoint,
    transform: &PlotTransform,
) -> Result<Option<[egui::Pos2; 2]>, GeometryUnavailable> {
    let bounds = *transform.bounds();
    let frame = *transform.frame();
    if !plot_bounds::valid(bounds)
        || !frame.is_finite()
        || frame.width() < 1.0
        || frame.height() < 1.0
        || frame
            .min
            .x
            .abs()
            .max(frame.min.y.abs())
            .max(frame.max.x.abs())
            .max(frame.max.y.abs())
            > 4096.0
    {
        return Err(GeometryUnavailable::Transform);
    }
    let mut p = [0.0; 2];
    let mut q = [0.0; 2];
    for axis in 0..2 {
        let lo = bounds.min()[axis];
        let span = bounds.max()[axis] - lo;
        p[axis] = ([a.x, a.y][axis] - lo) / span;
        q[axis] = ([b.x, b.y][axis] - lo) / span;
        if !p[axis].is_finite() || !q[axis].is_finite() {
            return Err(GeometryUnavailable::Segment);
        }
    }
    // Cohen-Sutherland with a fixed iteration limit. Each successful clip pins
    // one coordinate exactly to a face. Scaled weights avoid endpoint-difference
    // overflow, including far-away opposing points. Failure is not continuity.
    for _ in 0..8 {
        let cp = code(p);
        let cq = code(q);
        if cp & cq != 0 {
            return Ok(None);
        }
        if cp | cq == 0 {
            let map = |r: [f64; 2]| {
                egui::pos2(
                    (f64::from(frame.left()) + r[0] * f64::from(frame.width())) as f32,
                    (f64::from(frame.bottom()) - r[1] * f64::from(frame.height())) as f32,
                )
            };
            return Ok(Some([map(p), map(q)]));
        }
        let out = if cp != 0 { cp } else { cq };
        let axis = usize::from(out & 3 == 0);
        let edge = if out & (1 << (axis * 2)) != 0 {
            0.0
        } else {
            1.0
        };
        let scale = p[axis].abs().max(q[axis].abs()).max(1.0);
        let denominator = q[axis] / scale - p[axis] / scale;
        let t = (edge / scale - p[axis] / scale) / denominator;
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return Err(GeometryUnavailable::Segment);
        }
        let other = 1 - axis;
        let mut clipped = [0.0; 2];
        clipped[axis] = edge;
        let left = (1.0 - t) * p[other];
        let right = t * q[other];
        // Finite cancellation is not necessarily a resolved intersection. Do
        // not turn huge opposite coordinates into an invented on-screen zero.
        // This conservative guard rejects uncertainty above 1/4 UI point in a
        // <=4096-point frame. It is not a scientific error estimate.
        let uncertainty = 16.0 * f64::EPSILON * (p[other].abs() + q[other].abs());
        if left.signum() != right.signum() && uncertainty > 0.25 / 4096.0 {
            return Err(GeometryUnavailable::Segment);
        }
        clipped[other] = left + right;
        if !clipped[other].is_finite() {
            return Err(GeometryUnavailable::Segment);
        }
        if cp != 0 {
            p = clipped;
        } else {
            q = clipped;
        }
    }
    Err(GeometryUnavailable::Segment)
}

fn code(p: [f64; 2]) -> u8 {
    u8::from(p[0] < 0.0)
        | (u8::from(p[0] > 1.0) << 1)
        | (u8::from(p[1] < 0.0) << 2)
        | (u8::from(p[1] > 1.0) << 3)
}

/// Independent square-capped rectangles avoid all join-normal arithmetic in
/// epaint. Coincident geometry gets one square marker. This CPU preview does
/// not claim stock Line's antialiasing/miter appearance.
fn quad(
    mesh: &mut egui::Mesh,
    [a, b]: [egui::Pos2; 2],
    width: f32,
    color: egui::Color32,
) -> Option<()> {
    if !width.is_finite() || !(0.0..=64.0).contains(&width) {
        return None;
    }
    let dx = f64::from(b.x) - f64::from(a.x);
    let dy = f64::from(b.y) - f64::from(a.y);
    let length = dx.hypot(dy);
    let (tx, ty) = if length == 0.0 {
        (1.0, 0.0)
    } else {
        (dx / length, dy / length)
    };
    let t = egui::vec2(tx as f32, ty as f32) * (width / 2.0);
    let n = egui::vec2(-t.y, t.x);
    let positions = [a - t - n, a - t + n, b + t + n, b + t - n];
    if !positions.iter().all(|p| p.is_finite()) {
        return None;
    }
    let start = u32::try_from(mesh.vertices.len()).ok()?;
    for pos in positions {
        mesh.colored_vertex(pos, color);
    }
    mesh.indices
        .extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    Some(())
}

fn closest(pointer: egui::Pos2, [a, b]: [egui::Pos2; 2]) -> Option<(egui::Pos2, f32)> {
    if !pointer.is_finite() || !a.is_finite() || !b.is_finite() {
        return None;
    }
    let dx = f64::from(b.x) - f64::from(a.x);
    let dy = f64::from(b.y) - f64::from(a.y);
    let px = f64::from(pointer.x) - f64::from(a.x);
    let py = f64::from(pointer.y) - f64::from(a.y);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq == 0.0 {
        0.0
    } else {
        ((px * dx + py * dy) / length_sq).clamp(0.0, 1.0)
    };
    let position = egui::pos2(
        (f64::from(a.x) + t * dx) as f32,
        (f64::from(a.y) + t * dy) as f32,
    );
    let distance = (px - t * dx).powi(2) + (py - t * dy).powi(2);
    (position.is_finite() && distance.is_finite() && distance <= f64::from(f32::MAX))
        .then_some((position, distance as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_and_closest_are_finite_even_where_stock_distance_overflows() {
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(1100.0, 600.0));
        let transform = PlotTransform::new(rect, PlotBounds::new_symmetrical(1.0), false);
        for magnitude in [1.0, 1e30, 1e300] {
            for sign in [-1.0, 1.0] {
                let segment = screen_segment(
                    PlotPoint::new(-sign * magnitude, 0.0),
                    PlotPoint::new(sign * magnitude, 0.0),
                    &transform,
                )
                .unwrap()
                .unwrap();
                assert_eq!(segment[0].y, rect.center().y);
                assert_eq!(segment[1].y, rect.center().y);
                assert_eq!(
                    segment[0].x,
                    if sign > 0.0 {
                        rect.left()
                    } else {
                        rect.right()
                    }
                );
                assert_eq!(
                    segment[1].x,
                    if sign > 0.0 {
                        rect.right()
                    } else {
                        rect.left()
                    }
                );
                let (point, distance) = closest(rect.center(), segment).unwrap();
                assert_eq!(point, rect.center());
                assert_eq!(distance, 0.0);
            }
        }
        assert!(
            screen_segment(
                PlotPoint::new(2.0, 2.0),
                PlotPoint::new(3.0, 3.0),
                &transform
            )
            .unwrap()
            .is_none()
        );
        assert!(closest(egui::pos2(f32::NAN, 0.0), [rect.min, rect.max]).is_none());
        assert_eq!(
            screen_segment(
                PlotPoint::new(-1e300, -1e300),
                PlotPoint::new(1e300, 1e300),
                &transform
            ),
            Err(GeometryUnavailable::Segment)
        );
    }

    #[test]
    fn clipped_vertical_and_coincident_signed_zero_have_safe_primitives() {
        let transform = PlotTransform::new(
            egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(1010.0, 510.0)),
            PlotBounds::new_symmetrical(1.0),
            false,
        );
        for (a, b) in [
            (PlotPoint::new(0.0, -1e300), PlotPoint::new(0.0, 1e300)),
            (PlotPoint::new(-0.0, 0.0), PlotPoint::new(0.0, -0.0)),
            (
                PlotPoint::new(0.0, 0.0),
                PlotPoint::new(f64::from_bits(1), f64::from_bits(1)),
            ),
        ] {
            let segment = screen_segment(a, b, &transform).unwrap().unwrap();
            for width in [0.9, 1.0, 32.0, 64.0] {
                let mut mesh = egui::Mesh::default();
                quad(&mut mesh, segment, width, egui::Color32::RED).unwrap();
                assert!(mesh.is_valid());
                assert!(
                    mesh.vertices
                        .iter()
                        .all(|v| v.pos.is_finite() && v.uv.is_finite())
                );
                assert_eq!((mesh.vertices.len(), mesh.indices.len()), (4, 6));
            }
            assert!(
                closest(transform.frame().center(), segment)
                    .unwrap()
                    .1
                    .is_finite()
            );
        }
    }
}
