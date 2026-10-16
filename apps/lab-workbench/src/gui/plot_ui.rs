//! Shared standard egui_plot painting, without document or Runtime ownership.
use eframe::egui;
use egui_plot::{HoverPosition, Line, Plot, PlotPoint, PlotPoints};

/// Borrowed display inputs; the caller owns points, identity and tooltip policy.
pub(crate) struct Curve<'a> {
    pub(crate) id: egui::Id,
    pub(crate) label: &'a str,
    pub(crate) points: &'a [PlotPoint],
    pub(crate) width: f32,
    pub(crate) color: Option<egui::Color32>,
}

/// Paint with the standard plot implementation. Retain no model or GPU state.
pub(crate) fn show<'a>(
    ui: &mut egui::Ui,
    view: Plot<'a>,
    curves: &'a [Curve<'a>],
    tooltip: impl Fn(&str, usize, PlotPoint) -> Option<String> + 'a,
) {
    // HoverPosition carries a name, not an item ID. Preserve unique legend
    // names so the caller resolves units/freshness against the selected curve.
    let mut names: Vec<String> = Vec::with_capacity(curves.len());
    for curve in curves {
        let mut name = curve.label.to_owned();
        let mut suffix = 2;
        while names.contains(&name) {
            name = format!("{} ({suffix})", curve.label);
            suffix += 1;
        }
        names.push(name);
    }
    view.label_formatter(|hover| {
        let HoverPosition::NearDataPoint {
            plot_name,
            position,
            ..
        } = hover
        else {
            return None;
        };
        let index = names.iter().position(|name| name == plot_name)?;
        tooltip(plot_name, index, *position)
    })
    .show(ui, |plot_ui| {
        for (name, curve) in names
            .iter()
            .zip(curves)
            .filter(|(_, c)| !c.points.is_empty())
        {
            let mut line = Line::new(name, PlotPoints::Borrowed(curve.points))
                .id(curve.id)
                .width(curve.width);
            if let Some(color) = curve.color {
                line = line.color(color);
            }
            plot_ui.line(line);
        }
    });
}
