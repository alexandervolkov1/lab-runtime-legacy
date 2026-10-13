//! Deterministic research fixtures shared by headless tests and the native meter.
//! Nothing in this module is used by the production Preview.
use super::plot_fixtures as base;
use crate::{
    model::{LivePoint, RuntimeObservations},
    presentation::v2,
};
use eframe::egui;
use serde_json::json;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Geometry {
    Horizontal,
    Vertical,
    Zigzag,
    Steps,
    Coincident,
    NearlyCoincident,
    Crossing,
    Irregular,
    NonMonotonic,
    Subnormal,
    SignedZero,
    HugeY,
    HugeX,
}
#[derive(Clone, Debug)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) traces: usize,
    pub(crate) source_points: usize,
    pub(crate) lengths: Option<[usize; 32]>,
    pub(crate) width: f64,
    pub(crate) geometry: Geometry,
    pub(crate) shared: bool,
    pub(crate) hidden: bool,
    pub(crate) unicode: bool,
    pub(crate) window: f64,
    pub(crate) axes: Option<(f64, f64)>,
}
impl Case {
    /// Configuration semantics, independent of the renderer's reported reason:
    /// ordinary curves must actually render, not pass by diagnosing everything.
    pub(crate) fn accepts_output(&self, stats: GeometryStats) -> bool {
        let axis_refusal = self
            .axes
            .is_some_and(|(lo, hi)| lo.abs().max(hi.abs()) > f64::MAX / 4096.0);
        let numeric_refusal = stats.points > 0
            && (self.width as f32 == 0.0
                || matches!(
                    self.geometry,
                    Geometry::HugeX | Geometry::HugeY | Geometry::Subnormal
                ));
        if axis_refusal || numeric_refusal {
            stats.segments == 0 && stats.unavailable > 0
        } else {
            stats.unavailable == 0 && (stats.points == 0 || stats.segments > 0)
        }
    }

    pub(crate) fn combined() -> Self {
        Self {
            name: "combined".into(),
            traces: 32,
            source_points: 512,
            lengths: None,
            width: 32.0,
            geometry: Geometry::Zigzag,
            shared: false,
            hidden: false,
            unicode: true,
            window: 604800.0,
            axes: None,
        }
    }
}
pub(crate) fn matrix() -> Vec<Case> {
    let mut cases = Vec::with_capacity(96);
    for traces in [0, 1, 2, 16, 32] {
        for points in [0, 1, 2, 512, 4096] {
            cases.push(Case {
                name: format!("count-{traces}-{points}"),
                traces,
                source_points: points,
                unicode: false,
                ..Case::combined()
            });
        }
    }
    for geometry in [
        Geometry::Horizontal,
        Geometry::Vertical,
        Geometry::Zigzag,
        Geometry::Steps,
        Geometry::Coincident,
        Geometry::NearlyCoincident,
        Geometry::Crossing,
        Geometry::Irregular,
        Geometry::NonMonotonic,
    ] {
        for width in [f64::from_bits(1), 0.9, 1.0, 32.0] {
            cases.push(Case {
                name: format!("{geometry:?}-{width:e}"),
                geometry,
                width,
                unicode: false,
                ..Case::combined()
            });
        }
    }
    for geometry in [
        Geometry::Subnormal,
        Geometry::SignedZero,
        Geometry::HugeY,
        Geometry::HugeX,
    ] {
        cases.push(Case {
            name: format!("numeric-{geometry:?}"),
            traces: 2,
            source_points: 512,
            geometry,
            ..Case::combined()
        });
    }
    for (name, axes) in [
        ("axes-wide", (-f64::MAX, f64::MAX)),
        ("axes-tiny", (-f64::from_bits(1), f64::from_bits(1))),
        ("axes-positive", (f64::MAX / 2.0, f64::MAX)),
    ] {
        cases.push(Case {
            name: name.into(),
            axes: Some(axes),
            ..Case::combined()
        });
    }
    cases.push(Case {
        name: "shared".into(),
        shared: true,
        ..Case::combined()
    });
    cases.push(Case {
        name: "hidden".into(),
        hidden: true,
        ..Case::combined()
    });
    cases.push(Case {
        name: "smallest-window".into(),
        window: f64::from_bits(1),
        ..Case::combined()
    });
    cases.push(Case {
        name: "combined-decimated".into(),
        source_points: 4096,
        ..Case::combined()
    });
    // Identical total point counts can grow tessellation Vec capacities
    // differently depending on the order and size of each line's reservation.
    for large in 1..=4 {
        let small = (16384 - large * 4096) / 512;
        let traces = large + small;
        for reverse in [false, true] {
            let mut lengths = [0; 32];
            lengths[..large].fill(4096);
            lengths[large..traces].fill(512);
            if reverse {
                lengths[..traces].reverse();
            }
            cases.push(Case {
                name: format!("mixed-{large}-large-reverse-{reverse}"),
                traces,
                source_points: 4096,
                lengths: Some(lengths),
                ..Case::combined()
            });
        }
    }
    cases.push(Case::combined());
    assert!(cases.len() <= 96);
    cases
}
pub(crate) fn scene(case: &Case, phase: usize) -> (v2::AdmittedV2Document, RuntimeObservations) {
    let mut syntax = base::syntax(case.traces, case.shared);
    syntax["plots"][0]["time_window_seconds"] = json!(case.window);
    syntax["plots"][0]["axes"] = case.axes.map_or(
        json!({"y_min":null,"y_max":null}),
        |(min, max)| json!({"y_min":min,"y_max":max}),
    );
    if case.unicode {
        syntax["plots"][0]["title"] = json!(base::unicode_text(37, 0, 512));
    }
    let sources = if case.shared {
        case.traces.min(1)
    } else {
        case.traces
    };
    let mut obs = base::observations(sources, 0);
    for i in 0..case.traces {
        let trace = &mut syntax["plots"][0]["traces"][i];
        trace["style"]["width"] = json!(case.width);
        trace["visible"] = json!(!case.hidden);
        if case.unicode {
            trace["display_label"] = json!(base::unicode_text(i, 0, 512));
            trace["display_unit"] = json!(base::unicode_text(i + 32, 2, 512));
        }
    }
    for source in 0..sources {
        let unit = json!({"id":base::unicode_text(source+phase,0,32),
            "symbol":base::unicode_text(source+phase,2,16)});
        obs.observe_read_descriptor(
            base::target(source),
            &json!({"value_type":"float","generation":"1","unit":unit}),
        );
        obs.entities.get_mut(&base::target(source)).unwrap().value["unit"] = unit;
        let buffer = obs.live.entry(base::target(source)).or_default();
        let points = case.lengths.map_or(case.source_points, |lens| lens[source]);
        for i in 0..points {
            let offset = source as f64 / 64.0;
            let sign = if (i + phase).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            let (x, y) = match case.geometry {
                Geometry::Horizontal => (i as f64, offset),
                Geometry::Vertical => (1.0, sign * 100.0 + offset),
                Geometry::Zigzag => (i as f64, sign * 100.0 + offset),
                Geometry::Steps => ((i / 2) as f64, ((i / 2) % 2) as f64 * 100.0 + offset),
                Geometry::Coincident => (1.0, 1.0),
                Geometry::NearlyCoincident => (
                    1.0 + i as f64 * f64::EPSILON,
                    1.0 + (i % 3) as f64 * f64::EPSILON,
                ),
                Geometry::Crossing => (
                    i as f64,
                    (if source % 2 == 0 { 1.0 } else { -1.0 }) * (i as f64 - 256.0),
                ),
                Geometry::Irregular => ((i * i) as f64 / 32.0, sign * 100.0 + offset),
                Geometry::NonMonotonic => (((i * 43) % 512) as f64, sign * 100.0 + offset),
                Geometry::Subnormal => (i as f64 * f64::from_bits(1), sign * f64::from_bits(1)),
                Geometry::SignedZero => (0.0 * sign, 0.0 * sign),
                Geometry::HugeY => (i as f64, sign * f64::MAX),
                // Final -MAX keeps earlier points eligible in B1's lower-cutoff
                // filter. LiveTraceBuffer admits this order; no sorting is done.
                Geometry::HugeX => (
                    if i + 1 == points {
                        -f64::MAX
                    } else {
                        sign * f64::MAX
                    },
                    sign,
                ),
            };
            buffer
                .push(LivePoint {
                    time_seconds: x,
                    value: y,
                })
                .unwrap();
        }
    }
    (base::admit(&syntax), obs)
}

/// Actual paint output inspected without allocating another geometry buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GeometryStats {
    pub(crate) points: usize,
    pub(crate) traces: usize,
    pub(crate) segments: usize,
    pub(crate) unavailable: usize,
    pub(crate) policy_failures: usize,
    pub(crate) shapes: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    pub(crate) bad_shape_positions: usize,
    pub(crate) bad_mesh_positions: usize,
    pub(crate) invalid_meshes: usize,
    pub(crate) unsupported_shapes: usize,
    pub(crate) panics: usize,
    pub(crate) allocation_faults: usize,
    pub(crate) curve_vertices: usize,
    pub(crate) curve_indices: usize,
    pub(crate) curve_vertex_capacity: usize,
    pub(crate) curve_index_capacity: usize,
}
impl GeometryStats {
    pub(crate) fn max_assign(&mut self, other: Self) {
        self.points = self.points.max(other.points);
        self.traces = self.traces.max(other.traces);
        self.segments = self.segments.max(other.segments);
        self.unavailable = self.unavailable.max(other.unavailable);
        self.policy_failures += other.policy_failures;
        self.shapes = self.shapes.max(other.shapes);
        self.vertices = self.vertices.max(other.vertices);
        self.indices = self.indices.max(other.indices);
        self.bad_shape_positions = self.bad_shape_positions.max(other.bad_shape_positions);
        self.bad_mesh_positions = self.bad_mesh_positions.max(other.bad_mesh_positions);
        self.invalid_meshes = self.invalid_meshes.max(other.invalid_meshes);
        self.unsupported_shapes = self.unsupported_shapes.max(other.unsupported_shapes);
        self.panics += other.panics;
        self.allocation_faults += other.allocation_faults;
        self.curve_vertices = self.curve_vertices.max(other.curve_vertices);
        self.curve_indices = self.curve_indices.max(other.curve_indices);
        self.curve_vertex_capacity = self.curve_vertex_capacity.max(other.curve_vertex_capacity);
        self.curve_index_capacity = self.curve_index_capacity.max(other.curve_index_capacity);
    }
    pub(crate) fn finite(self) -> bool {
        self.bad_shape_positions == 0
            && self.bad_mesh_positions == 0
            && self.invalid_meshes == 0
            && self.unsupported_shapes == 0
            && self.panics == 0
            && self.policy_failures == 0
            && self.points <= 16_384
            && self.traces <= 32
            && self.segments <= 16_384
            && self.curve_vertices <= 65_536
            && self.curve_indices <= 98_304
            // Measured capacities for the pinned native allocator/toolchain;
            // try_reserve_exact is not a portable exact-capacity guarantee.
            && self.curve_vertex_capacity <= 65_536
            && self.curve_index_capacity <= 98_304
            && self.vertices <= 4 * self.points + 32_768
            && self.indices <= 6 * self.points + 131_072
    }
    fn shape(&mut self, shape: &egui::Shape) {
        self.shapes += 1;
        match shape {
            egui::Shape::Vec(v) => {
                for shape in v {
                    self.shape(shape);
                }
            }
            egui::Shape::Path(p) => {
                self.bad_shape_positions += p.points.iter().filter(|p| !p.is_finite()).count();
                self.bad_shape_positions += usize::from(!p.stroke.width.is_finite());
            }
            egui::Shape::LineSegment { points, stroke } => {
                self.bad_shape_positions += points.iter().filter(|p| !p.is_finite()).count();
                self.bad_shape_positions += usize::from(!stroke.width.is_finite());
            }
            egui::Shape::Circle(c) => {
                self.bad_shape_positions += usize::from(
                    !c.center.is_finite() || !c.radius.is_finite() || !c.stroke.width.is_finite(),
                );
            }
            egui::Shape::Ellipse(e) => {
                self.bad_shape_positions += usize::from(
                    !e.center.is_finite() || !e.radius.is_finite() || !e.stroke.width.is_finite(),
                );
            }
            egui::Shape::Rect(r) => {
                self.bad_shape_positions += usize::from(
                    !r.rect.is_finite()
                        || !r.angle.is_finite()
                        || !r.stroke.width.is_finite()
                        || !r.blur_width.is_finite(),
                );
            }
            egui::Shape::Text(t) => {
                self.bad_shape_positions += usize::from(!t.pos.is_finite() || !t.angle.is_finite());
            }
            egui::Shape::Mesh(m) => {
                self.curve_vertices += m.vertices.len();
                self.curve_indices += m.indices.len();
                self.curve_vertex_capacity += m.vertices.capacity();
                self.curve_index_capacity += m.indices.capacity();
                self.bad_shape_positions += m
                    .vertices
                    .iter()
                    .filter(|v| !v.pos.is_finite() || !v.uv.is_finite())
                    .count();
                self.invalid_meshes += usize::from(!m.is_valid());
            }
            egui::Shape::QuadraticBezier(b) => {
                self.bad_shape_positions += b.points.iter().filter(|p| !p.is_finite()).count();
                self.bad_shape_positions += usize::from(!b.stroke.width.is_finite());
            }
            egui::Shape::CubicBezier(b) => {
                self.bad_shape_positions += b.points.iter().filter(|p| !p.is_finite()).count();
                self.bad_shape_positions += usize::from(!b.stroke.width.is_finite());
            }
            // Backend callbacks cannot be CPU-qualified by this inspector.
            // The isolated preview never emits them. Treat one as a blocker.
            egui::Shape::Callback(_) => self.unsupported_shapes += 1,
            egui::Shape::Noop => {}
        }
    }
    pub(crate) fn inspect(
        preview: &crate::gui::readonly_v2::Preview<'_>,
        output: &mut egui::FullOutput,
        report: crate::gui::readonly_v2::RenderReport,
    ) -> Self {
        let mut stats = Self {
            points: report.plot_points,
            traces: report.plot_traces,
            segments: report.geometry_segments,
            unavailable: report.geometry_unavailable,
            policy_failures: report.geometry_failures,
            allocation_faults: report.allocation_faults,
            ..Self::default()
        };
        for shape in &output.shapes {
            stats.shape(&shape.shape);
        }
        // Keep FullOutput alive across unwinding so its TexturesDelta can still
        // be explicitly discarded. Otherwise its debug Drop assertion produces
        // a second panic and aborts the research process. A caught library panic
        // is a BLOCKED result, never successful geometry or a production guard.
        let meshes = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            preview.tessellate(output)
        })) {
            Ok(meshes) => meshes,
            Err(payload) => {
                drop(payload);
                stats.panics += 1;
                return stats;
            }
        };
        stats.policy_failures += usize::from(stats.shapes > 0 && meshes.is_empty());
        for primitive in &meshes {
            if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                stats.vertices += mesh.vertices.len();
                stats.indices += mesh.indices.len();
                stats.bad_mesh_positions += mesh
                    .vertices
                    .iter()
                    .filter(|v| !v.pos.is_finite() || !v.uv.is_finite())
                    .count();
                stats.invalid_meshes += usize::from(!mesh.is_valid());
            }
        }
        stats
    }
}
