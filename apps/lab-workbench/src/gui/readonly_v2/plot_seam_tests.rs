//! Public-API integration evidence, not an implementation of safe geometry.
//! The witness delegates unchanged stock Line behavior and exists only in tests.
use std::{cell::Cell, ops::RangeInclusive};

use eframe::egui;
use egui_plot::{
    ClosestElem, Line, Plot, PlotBounds, PlotGeometry, PlotItem, PlotItemBase, PlotPoint,
    PlotTransform,
};

struct WitnessLine<'a> {
    line: Line<'a>,
    draw: &'a Cell<Option<PlotTransform>>,
    hover: &'a Cell<Option<PlotTransform>>,
}
impl PlotItem for WitnessLine<'_> {
    fn shapes(&self, ui: &egui::Ui, transform: &PlotTransform, shapes: &mut Vec<egui::Shape>) {
        self.draw.set(Some(*transform));
        self.line.shapes(ui, transform, shapes);
    }
    fn find_closest(&self, point: egui::Pos2, transform: &PlotTransform) -> Option<ClosestElem> {
        self.hover.set(Some(*transform));
        self.line.find_closest(point, transform)
    }
    fn initialize(&mut self, range: RangeInclusive<f64>) {
        self.line.initialize(range);
    }
    fn color(&self) -> egui::Color32 {
        PlotItem::color(&self.line)
    }
    fn geometry(&self) -> PlotGeometry<'_> {
        self.line.geometry()
    }
    fn bounds(&self) -> PlotBounds {
        self.line.bounds()
    }
    fn base(&self) -> &PlotItemBase {
        self.line.base()
    }
    fn base_mut(&mut self) -> &mut PlotItemBase {
        self.line.base_mut()
    }
}

struct Frame {
    build: PlotTransform,
    draw: PlotTransform,
    hover: Option<PlotTransform>,
    returned: PlotTransform,
    output: egui::FullOutput,
}
impl Drop for Frame {
    fn drop(&mut self) {
        // Preserve an assertion failure without a second TexturesDelta drop panic.
        std::mem::take(&mut self.output).drop_without_applying_deltas();
    }
}
fn frame(
    ctx: &egui::Context,
    points: &[PlotPoint],
    events: Vec<egui::Event>,
    requested: Option<PlotBounds>,
    auto: bool,
) -> Frame {
    let build = Cell::new(None);
    let draw = Cell::new(None);
    let hover = Cell::new(None);
    let returned = Cell::new(None);
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 960.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let response = Plot::new("transform-witness")
                .width(1200.0)
                .height(540.0)
                .x_grid_spacer(|_| Vec::new())
                .y_grid_spacer(|_| Vec::new())
                .show(ui, |plot| {
                    build.set(Some(*plot.transform()));
                    if let Some(bounds) = requested {
                        plot.set_plot_bounds(bounds);
                        // This setter queues work; even the next read is old.
                        same(*plot.transform(), build.get().unwrap());
                    }
                    if auto {
                        plot.set_auto_bounds(true);
                    }
                    plot.add(WitnessLine {
                        line: Line::new("T01", points).width(32.0),
                        draw: &draw,
                        hover: &hover,
                    });
                });
            returned.set(Some(response.transform));
        },
    );
    Frame {
        build: build.get().unwrap(),
        draw: draw.get().unwrap(),
        hover: hover.get(),
        returned: returned.get().unwrap(),
        output,
    }
}
fn discard(frame: Frame) {
    drop(frame);
}
fn same(a: PlotTransform, b: PlotTransform) {
    assert_eq!(a.bounds(), b.bounds());
    assert_eq!(a.frame(), b.frame());
    assert_eq!(a.dpos_dvalue(), b.dpos_dvalue());
}

#[test]
fn initial_auto_bounds_and_queued_restore_are_not_the_build_transform() {
    let ctx = egui::Context::default();
    let points = [PlotPoint::new(10.0, 20.0), PlotPoint::new(30.0, 40.0)];
    let first = frame(&ctx, &points, vec![], None, false);
    assert_ne!(first.build.bounds(), first.draw.bounds());
    same(first.draw, first.returned);
    discard(first);
    let requested = PlotBounds::from_min_max([-100.0, -200.0], [100.0, 200.0]);
    let restored = frame(&ctx, &points, vec![], Some(requested), false);
    assert_ne!(restored.build.bounds(), &requested);
    assert_eq!(restored.draw.bounds(), &requested);
    same(restored.draw, restored.returned);
    discard(restored);
    let reset = frame(&ctx, &points, vec![], None, true);
    assert_eq!(reset.build.bounds(), &requested);
    assert_ne!(reset.draw.bounds(), &requested);
    same(reset.draw, reset.returned);
    discard(reset);
}

#[test]
fn real_zoom_and_pan_change_transform_after_build_before_draw_and_hover() {
    let ctx = egui::Context::default();
    let points = [PlotPoint::new(0.0, 0.0), PlotPoint::new(10.0, 10.0)];
    let mut previous = None;
    for _ in 0..3 {
        let settled = frame(&ctx, &points, vec![], None, false);
        previous = Some(settled.returned);
        discard(settled);
    }
    let previous = previous.unwrap();
    let center = previous.frame().center();
    let zoomed = frame(
        &ctx,
        &points,
        vec![egui::Event::PointerMoved(center), egui::Event::Zoom(2.0)],
        None,
        false,
    );
    same(zoomed.build, previous);
    assert_ne!(zoomed.build.bounds(), zoomed.draw.bounds());
    same(zoomed.draw, zoomed.returned);
    same(zoomed.hover.unwrap(), zoomed.draw);
    discard(zoomed);
    discard(frame(
        &ctx,
        &points,
        vec![egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        None,
        false,
    ));
    let panned = frame(
        &ctx,
        &points,
        vec![egui::Event::PointerMoved(center + egui::vec2(90.0, 40.0))],
        None,
        false,
    );
    assert_ne!(panned.build.bounds(), panned.draw.bounds());
    same(panned.draw, panned.returned);
    same(panned.hover.unwrap(), panned.draw);
    discard(panned);
}

#[test]
fn last_transform_screen_dedup_can_be_invalidated_by_one_accepted_zoom_event() {
    let ctx = egui::Context::default();
    let points = [
        PlotPoint::new(0.5, 0.5),
        PlotPoint::new(0.5 + 1e-7, 0.5),
        PlotPoint::new(0.5 + 2e-7, 0.5),
    ];
    let bounds = PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]);
    for _ in 0..3 {
        discard(frame(&ctx, &points, vec![], Some(bounds), false));
    }
    let settled = frame(&ctx, &points, vec![], None, false);
    let center = settled.returned.frame().center();
    discard(settled);
    let zoomed = frame(
        &ctx,
        &points,
        vec![egui::Event::PointerMoved(center), egui::Event::Zoom(0.1)],
        None,
        false,
    );
    let old = points.map(|p| zoomed.build.position_from_point(&p));
    let actual = points.map(|p| zoomed.draw.position_from_point(&p));
    assert!(old.windows(2).all(|w| w[0] != w[1]));
    assert_eq!(actual[0], actual[1]);
    assert_eq!(actual[1], actual[2]);
    same(zoomed.draw, zoomed.returned);
    // Inspect actual pre-tessellation output, not just our witness. No geometry
    // is rewritten here, and this evidence is not a passing qualification gate.
    assert!(
        zoomed
            .output
            .shapes
            .iter()
            .any(|shape| { matches!(&shape.shape, egui::Shape::Path(p) if p.points == actual) })
    );
    discard(zoomed);
}

#[test]
fn finite_screen_positions_can_still_overflow_stock_hover_distance_arithmetic() {
    let transform = PlotTransform::new(
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 540.0)),
        PlotBounds::from_min_max([-1.0, -1.0], [1.0, 1.0]),
        false,
    );
    let points = [PlotPoint::new(-1e30, 0.0), PlotPoint::new(1e30, 0.0)];
    assert!(
        points
            .iter()
            .all(|p| transform.position_from_point(p).is_finite())
    );
    let line = Line::new("T01", points.as_slice());
    let closest = line
        .find_closest(egui::pos2(600.0, 270.0), &transform)
        .unwrap();
    assert!(!closest.dist_sq.is_finite());
    // A replacement shapes() alone would not repair this independent hover
    // path. A reviewed item needs shared checked segment arithmetic for both.
}
